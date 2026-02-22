use base64::{
    engine::general_purpose::{GeneralPurpose, GeneralPurposeConfig},
    Engine,
};
use futures_util::{SinkExt, StreamExt};
use reqwest::multipart;
use rdev::{Key, simulate, EventType};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use tauri::{Emitter, Manager};
use tokio::sync::{mpsc, Mutex};
use tokio_tungstenite::tungstenite::Message;

fn decode_base64_audio(s: &str) -> Result<Vec<u8>, String> {
    let engine = GeneralPurpose::new(
        &base64::alphabet::STANDARD,
        GeneralPurposeConfig::new()
            .with_decode_padding_mode(base64::engine::DecodePaddingMode::Indifferent)
            .with_decode_allow_trailing_bits(true),
    );
    engine.decode(s).map_err(|e| format!("Invalid base64: {}", e))
}

#[derive(Debug, serde::Deserialize)]
struct OpenAIMessage {
    content: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct OpenAIChoice {
    message: OpenAIMessage,
}

#[derive(Debug, serde::Deserialize)]
struct OpenAIResponse {
    choices: Vec<OpenAIChoice>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Step {
    pub action: String,
    pub payload: String,
    #[serde(default)]
    pub target_type: Option<String>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct ParsedIntent {
    /// Plan: one or more steps. Compound commands ("open Wikipedia and search for X") = multiple steps.
    pub steps: Vec<Step>,
    /// When present, the user intent is conversational (not a command). Speak this reply instead of executing steps.
    #[serde(default)]
    pub chat_reply: Option<String>,
}

#[tauri::command]
async fn parse_intent(
    command: String,
    api_key_override: Option<String>,
    app: tauri::AppHandle,
) -> Result<ParsedIntent, String> {
    let api_key = api_key_override
        .filter(|k| !k.is_empty())
        .or_else(|| std::env::var("OPENAI_API_KEY").ok())
        .ok_or("No API key. Set OPENAI_API_KEY env var or add your key in Settings.")?;

    let page_context = if let Some(bridge) = app.try_state::<ExtensionBridge>() {
        bridge.last_tab_url.lock().await.as_ref().cloned().unwrap_or_default()
    } else {
        String::new()
    };

    let system_prompt = r#"You are a smart assistant for users who cannot use mouse/keyboard. Classify intent: CONVERSATIONAL or COMMAND.

FIRST: Is this conversational (small talk, greetings, feelings, or questions that don't map to actions)?
Use chat_reply ONLY when:
- Greetings: "hi", "hello", "how are you", "good morning"
- Small talk: "what's your name", "tell me about yourself", "that's funny"
- Feelings/venting: "I'm frustrated", "this is confusing", "I'm tired"
- Open questions without action: "what do you think", "why is that"
- Filler: "ok", "umm", "thanks" (brief friendly acknowledgment)

Use steps (COMMAND) when the user wants to DO something:
- Navigate: "open X", "go to Y", "search for Z"
- On-page: "find X", "click Y", "scroll down", "type Z"
- System: "what time is it", "what's the date", "stop"
- Keyboard: "save", "close this app", "press Enter"

Output EITHER chat_reply OR steps, NEVER both.

If CONVERSATIONAL → return: {"chat_reply": "your brief, friendly, natural reply (1-2 sentences max)"}
If COMMAND → return: {"steps": [{"action": string, "payload": string, "target_type": "website"|"native_app"|null}, ...]}

ACTIONS: open, search, time, date, stop, click, find, find_and_read, find_next, find_prev, page_search, scroll, go_to, access_mode, close_popup, open_and_search, press_keys.

NATIVE APPS (target_type: "native_app"): Slack, Mail, Notes, Messages, Finder, Terminal, System Settings, Calendar, Reminders, Safari, Discord, Zoom, Spotify, Microsoft Teams, Outlook, OneNote, Notion, Visual Studio Code, Xcode. Map: "email"→Mail, "messages"→Messages, "notes"→Notes, "finder"→Finder, "settings"→System Settings, "calendar"→Calendar, "reminders"→Reminders, "vs code"→Visual Studio Code. Payload = exact app name.

KEYBOARD (press_keys): "press Command S", "save", "press Enter", "press Tab" = press_keys. Payload: "Command+S", "Enter", "Tab", "Escape", "Command+Q".
- QUIT APP: "close [App]", "quit [App]", "exit Mail" = press_keys "Command+Q". NEVER use "stop" for quit.
- CLOSE DIALOG: "close", "dismiss" = close_popup or press_keys "Escape".
- Other: save=Command+S, undo=Command+Z, redo=Command+Shift+Z.

USE PAGE CONTEXT (you get current URL when available):
- youtube.com: "first video", "click the second one" = click "first video", "second video". NEVER "youtube home" or "logo".
- wikipedia.org or articles: "find X", "scroll to X" = find_and_read X.
- "Type X", "search here for X" = page_search X. "Send" = click send/submit.
- On-page locate = find, find_and_read, go_to. Web search = search. Ambiguous → on-page.
- Extract concepts: "the thing about money" → "price". "When it's due" → "deadline"."#;

    let user_content = if page_context.is_empty() {
        command.clone()
    } else {
        format!("[Page: {}]\nUser: {}", page_context, command)
    };

    let client = reqwest::Client::new();
    let res = client
        .post("https://api.openai.com/v1/chat/completions")
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&serde_json::json!({
            "model": "gpt-4o",
            "messages": [
                {"role": "system", "content": system_prompt},
                {"role": "user", "content": user_content}
            ],
            "response_format": {"type": "json_object"},
            "max_tokens": 512,
            "temperature": 0.05
        }))
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !res.status().is_success() {
        let status = res.status();
        let text = res.text().await.unwrap_or_default();
        return Err(format!("OpenAI API error ({}): {}", status, text));
    }

    let body: OpenAIResponse = res.json().await.map_err(|e| e.to_string())?;
    let content = body
        .choices
        .first()
        .and_then(|c| c.message.content.as_ref())
        .ok_or("No response from OpenAI")?;

    // Support: {"chat_reply": "..."} (conversational) OR {"steps": [...]} / {"action", "payload"} (commands)
    let value: serde_json::Value = serde_json::from_str(content).map_err(|e| format!("Parse error: {}", e))?;

    // Conversational intent: return immediately, no steps
    if let Some(reply) = value.get("chat_reply").and_then(|v| v.as_str()) {
        let reply = reply.trim().to_string();
        if !reply.is_empty() {
            return Ok(ParsedIntent {
                steps: vec![],
                chat_reply: Some(reply),
            });
        }
    }

    let mut steps = if let Some(arr) = value.get("steps").and_then(|v| v.as_array()) {
        arr.iter()
            .filter_map(|s| {
                let obj = s.as_object()?;
                Some(Step {
                    action: obj.get("action")?.as_str()?.to_string(),
                    payload: obj.get("payload").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                    target_type: obj.get("target_type").and_then(|v| v.as_str()).map(String::from),
                })
            })
            .collect::<Vec<_>>()
    } else if let (Some(action), payload) = (
        value.get("action").and_then(|v| v.as_str()),
        value.get("payload").and_then(|v| v.as_str()).unwrap_or(""),
    ) {
        vec![Step {
            action: action.to_string(),
            payload: payload.to_string(),
            target_type: value.get("target_type").and_then(|v| v.as_str()).map(String::from),
        }]
    } else {
        vec![Step {
            action: "search".to_string(),
            payload: command,
            target_type: None,
        }]
    };

    let valid = ["open", "search", "time", "date", "stop", "open_and_search", "click", "find", "find_and_read", "find_next", "find_prev", "page_search", "scroll", "access_mode", "close_popup", "go_to", "press_keys"];
    steps.retain(|s| valid.contains(&s.action.as_str()));
    if steps.is_empty() {
        steps = vec![Step {
            action: "search".to_string(),
            payload: command,
            target_type: None,
        }];
    }

    // Correct GPT mistake: "youtube home" / "home" / "logo" on YouTube feed → "first video"
    for step in &mut steps {
        if step.action.eq_ignore_ascii_case("click") {
            step.payload = correct_youtube_click_payload(&step.payload, &page_context);
        }
    }

    // GPT sometimes outputs [click first video, open youtube]. The open would navigate to home
    // and undo the video — remove the redundant open when we have both and we're on YouTube.
    let on_youtube = page_context.contains("youtube.com") || page_context.contains("youtu.be");
    let has_video_click = steps.iter().any(|s| {
        s.action.eq_ignore_ascii_case("click")
            && (s.payload.to_lowercase().contains("video") || s.payload.to_lowercase().contains("first") || s.payload.to_lowercase().contains("second"))
    });
    let opens_youtube = |s: &Step| {
        if !s.action.eq_ignore_ascii_case("open") {
            return false;
        }
        let p = s.payload.trim().to_lowercase();
        p == "youtube" || p == "youtube.com" || p == "the youtube" || p.starts_with("youtube ")
    };
    if on_youtube && has_video_click {
        steps.retain(|s| !opens_youtube(s));
    }

    // "close Slack", "quit the app", "exit Mail" etc. → press_keys Command+Q (GPT sometimes misses these)
    correct_quit_app(&command, &mut steps);

    Ok(ParsedIntent {
        steps,
        chat_reply: None,
    })
}

/// GPT picks the best-matching section from page headings given user intent.
#[tauri::command]
async fn resolve_section(intent: String, headings: Vec<String>) -> Result<String, String> {
    if headings.is_empty() {
        return Err("No sections found on this page".into());
    }
    let api_key = std::env::var("OPENAI_API_KEY")
        .map_err(|_| "No API key".to_string())?;
    let headings_str = headings.join("\n");
    let prompt = format!(
        r#"The user wants to go to a part of the page. They said: "{}"

These are the section headings on the page (one per line):
{}
{}

Return ONLY the exact heading text that best matches what the user wants, or "NONE" if none match. No explanation, no quotes."#,
        intent,
        headings_str,
        if headings.len() > 1 { "Pick the single best match. Return the heading exactly as shown above." } else { "" }
    );
    let client = reqwest::Client::new();
    let res = client
        .post("https://api.openai.com/v1/chat/completions")
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&serde_json::json!({
            "model": "gpt-4o",
            "messages": [{"role": "user", "content": prompt}],
            "max_tokens": 64,
            "temperature": 0.1
        }))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !res.status().is_success() {
        let text = res.text().await.unwrap_or_default();
        return Err(format!("API error: {}", text));
    }
    let body: OpenAIResponse = res.json().await.map_err(|e| e.to_string())?;
    let content = body
        .choices
        .first()
        .and_then(|c| c.message.content.as_ref())
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    let section = content.trim_matches('"').trim();
    if section.eq_ignore_ascii_case("none") || section.is_empty() {
        return Err("Couldn't find a matching section".into());
    }
    Ok(section.to_string())
}

/// Extract error message from OpenAI API response. Surfaces the real error.
fn extract_openai_error_message(body: &str, status: u16) -> String {
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(body) {
        if let Some(msg) = v
            .get("error")
            .and_then(|e| e.get("message"))
            .and_then(|m| m.as_str())
        {
            return msg.to_string();
        }
    }
    format!("Whisper API error: status {}", status)
}

/// Check if Whisper transcription is available (API key set)
#[tauri::command]
async fn whisper_available() -> bool {
    std::env::var("OPENAI_API_KEY").map(|k| !k.is_empty()).unwrap_or(false)
}

/// Transcribe audio from base64-encoded bytes (frontend sends directly over IPC).
#[tauri::command]
async fn transcribe_audio(base64_audio: String, mime_type: Option<String>) -> Result<String, String> {
    let base64_trimmed = base64_audio.trim().replace('\r', "").replace('\n', "");
    let audio_bytes = decode_base64_audio(&base64_trimmed)
        .map_err(|e| format!("Invalid audio data: {}", e))?;
    send_audio_to_whisper(&audio_bytes, mime_type.as_deref()).await
}

async fn send_audio_to_whisper(
    audio_bytes: &[u8],
    mime_type: Option<&str>,
) -> Result<String, String> {
    let api_key = std::env::var("OPENAI_API_KEY")
        .map_err(|_| "No API key. Set OPENAI_API_KEY in .env.".to_string())?
        .trim()
        .to_string();
    if api_key.is_empty() {
        return Err("No API key. Set OPENAI_API_KEY in .env.".into());
    }

    let (file_name, mime) = match mime_type {
        Some("audio/webm") | None => ("audio.webm", "audio/webm"),
        Some("audio/mpeg") => ("audio.mp3", "audio/mpeg"),
        Some("audio/mp4") => ("audio.m4a", "audio/mp4"),
        Some("audio/wav") => ("audio.wav", "audio/wav"),
        Some(m) => ("audio.webm", m),
    };

    let form = multipart::Form::new()
        .part(
            "file",
            multipart::Part::bytes(audio_bytes.to_vec())
                .file_name(file_name.to_string())
                .mime_str(mime)
                .map_err(|e| format!("Invalid MIME: {}", e))?,
        )
        .text("model", "whisper-1")
        .text("response_format", "json");

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;
    let res = client
        .post("https://api.openai.com/v1/audio/transcriptions")
        .header("Authorization", format!("Bearer {}", api_key))
        .multipart(form)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !res.status().is_success() {
        let status = res.status();
        let text = res.text().await.unwrap_or_default();
        eprintln!("[transcribe] Whisper API error {}: {}", status, text);
        return Err(extract_openai_error_message(&text, status.as_u16()));
    }

    let text = res.text().await.map_err(|e| e.to_string())?;
    let transcript = if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&text) {
        parsed
            .get("text")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string()
    } else {
        text.trim().to_string()
    };
    Ok(transcript)
}

#[tauri::command]
async fn open_url(url: String) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        // Open in Chrome so the extension can run (search, click, find on page)
        Command::new("open")
            .args(["-a", "Google Chrome", &url])
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(not(target_os = "macos"))]
    {
        open::that(&url).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
async fn open_app(name: String) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        Command::new("open").args(["-a", &name]).spawn().map_err(|e| e.to_string())?;
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = name;
        return Err("Opening apps supported on macOS".into());
    }
    Ok(())
}

/// "first video", "second one", "the third result" = in-page click, not navigation
fn looks_like_in_page_click(payload: &str) -> bool {
    let p = payload.to_lowercase();
    (p.contains("first") || p.contains("second") || p.contains("third") || p.contains("fourth") || p.contains("fifth") || p.contains("this") || p.contains("that"))
        && (p.contains("video") || p.contains("one") || p.contains("result") || p.contains("item") || p.len() < 25)
        || p.contains("the video")
        || p == "video"
}

/// On YouTube feed/search, GPT wrongly outputs "youtube home" / "home" / "logo" (matches logo aria-label).
/// Correct to "first video" so we click a video, not the homepage link.
fn correct_youtube_click_payload(payload: &str, page_url: &str) -> String {
    if !page_url.contains("youtube.com") && !page_url.contains("youtu.be") {
        return payload.to_string();
    }
    if page_url.contains("/watch") || page_url.contains("youtu.be/") {
        return payload.to_string(); // already on watch page, don't correct
    }
    let p = payload.trim().to_lowercase();
    let wrong = p == "youtube home" || p == "youtube  home" || p == "logo"
        || p == "home"
        || p == "youtube";
    if wrong {
        "first video".to_string()
    } else {
        payload.to_string()
    }
}

/// "close Slack", "quit the app", "exit Mail" → press_keys Command+Q. GPT wrongly outputs "stop" for these.
fn correct_quit_app(command: &str, steps: &mut Vec<Step>) {
    let c = command.trim().to_lowercase();
    if c.is_empty() {
        return;
    }
    // Don't touch "close popup", "close the popup", "dismiss" — those are close_popup
    if c.contains("popup") || c.contains("dialog") || c.contains("modal") || c == "dismiss" {
        return;
    }
    // GPT outputs "stop" for "close the notion app" etc. — convert to Command+Q
    let has_quit_word = c.contains("close") || c.contains("quit") || c.contains("exit") || c.contains("kill");
    let has_app_word = c.contains(" app") || c.ends_with(" app")
        || ["slack", "mail", "messages", "notion", "chrome", "safari", "notes", "discord", "zoom", "spotify", "teams", "outlook", "finder", "terminal"]
            .iter()
            .any(|w| c.contains(w));
    let looks_like_quit_app = has_quit_word && (has_app_word || c == "quit" || c == "exit");
    if !looks_like_quit_app {
        return;
    }
    let already_has_quit = steps.iter().any(|s| {
        s.action.eq_ignore_ascii_case("press_keys")
            && (s.payload.contains("Command+Q") || s.payload.contains("Meta+Q"))
    });
    if already_has_quit {
        return;
    }
    // Replace wrong "stop" (or any wrong step) with Command+Q
    *steps = vec![Step {
        action: "press_keys".to_string(),
        payload: "Command+Q".to_string(),
        target_type: None,
    }];
}

/// Format a step for the extension (matches main.ts formatExtensionCommand).
fn format_step_for_extension(step: &Step, page_url: &str) -> Option<String> {
    let action = step.action.to_lowercase();
    let payload = if action == "click" {
        correct_youtube_click_payload(step.payload.trim(), page_url)
    } else {
        step.payload.trim().to_string()
    };
    let cmd = match action.as_str() {
        "open" => {
            if looks_like_in_page_click(&payload) {
                format!("click {payload}")
            } else {
                format!("open {payload}")
            }
        }
        "search" => format!("search for {payload}"),
        "click" => format!("click {payload}"),
        "find" => format!("find {payload}"),
        "find_and_read" => format!("find_and_read {payload}"),
        "find_next" => "find next match".into(),
        "find_prev" => "find prev match".into(),
        "page_search" => format!("search for {payload}"),
        "scroll" => format!("scroll {}", payload.to_lowercase()),
        "access_mode" => {
            if payload.to_lowercase() == "on" {
                "one-hand mode on".into()
            } else {
                "one-hand mode off".into()
            }
        }
        "close_popup" => "close popup".into(),
        "go_to" => format!("get_headings:{payload}"),
        _ => return None,
    };
    Some(cmd)
}

/// Steps that the extension can execute (open, search navigate from content script).
const EXTENSION_EXECUTABLE: &[&str] = &[
    "open", "search", "click", "find", "find_and_read", "find_next", "find_prev",
    "page_search", "scroll", "access_mode", "close_popup", "go_to",
];

type WsWriter = futures_util::stream::SplitSink<
    tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>,
    tokio_tungstenite::tungstenite::Message,
>;

/// Parse key string like "Command+S", "Enter", "Tab" into (modifiers, main_key).
fn parse_key_combo(payload: &str) -> Result<(Vec<Key>, Key), String> {
    let s = payload.trim().replace([' ', '\t'], "");
    if s.is_empty() {
        return Err("Empty key combo".into());
    }
    let parts: Vec<&str> = s.split('+').collect();
    let mut modifiers = Vec::new();
    let mut main_key = None;
    for p in parts {
        let p_lower = p.to_lowercase();
        match p_lower.as_str() {
            "command" | "cmd" | "meta" => modifiers.push(Key::MetaLeft),
            "control" | "ctrl" => modifiers.push(Key::ControlLeft),
            "option" | "alt" => modifiers.push(Key::Alt),
            "shift" => modifiers.push(Key::ShiftLeft),
            _ => {
                if main_key.is_some() {
                    return Err(format!("Multiple main keys in '{}'", payload));
                }
                main_key = Some(parse_single_key(p));
            }
        }
    }
    let main = main_key.ok_or_else(|| format!("No main key in '{}'", payload))?;
    Ok((modifiers, main))
}

fn parse_single_key(s: &str) -> Key {
    match s.to_lowercase().as_str() {
        "enter" | "return" => Key::Return,
        "tab" => Key::Tab,
        "escape" | "esc" => Key::Escape,
        "space" => Key::Space,
        "backspace" => Key::Backspace,
        "delete" => Key::Delete,
        "home" => Key::Home,
        "end" => Key::End,
        "pageup" | "pgup" => Key::PageUp,
        "pagedown" | "pgdown" => Key::PageDown,
        "left" | "leftarrow" => Key::LeftArrow,
        "right" | "rightarrow" => Key::RightArrow,
        "up" | "uparrow" => Key::UpArrow,
        "down" | "downarrow" => Key::DownArrow,
        c if c.len() == 1 => {
            let ch = c.chars().next().unwrap();
            match ch {
                'a' => Key::KeyA, 'b' => Key::KeyB, 'c' => Key::KeyC, 'd' => Key::KeyD,
                'e' => Key::KeyE, 'f' => Key::KeyF, 'g' => Key::KeyG, 'h' => Key::KeyH,
                'i' => Key::KeyI, 'j' => Key::KeyJ, 'k' => Key::KeyK, 'l' => Key::KeyL,
                'm' => Key::KeyM, 'n' => Key::KeyN, 'o' => Key::KeyO, 'p' => Key::KeyP,
                'q' => Key::KeyQ, 'r' => Key::KeyR, 's' => Key::KeyS, 't' => Key::KeyT,
                'u' => Key::KeyU, 'v' => Key::KeyV, 'w' => Key::KeyW, 'x' => Key::KeyX,
                'y' => Key::KeyY, 'z' => Key::KeyZ,
                '0' => Key::Num0, '1' => Key::Num1, '2' => Key::Num2, '3' => Key::Num3,
                '4' => Key::Num4, '5' => Key::Num5, '6' => Key::Num6, '7' => Key::Num7,
                '8' => Key::Num8, '9' => Key::Num9,
                '-' => Key::Minus, '=' => Key::Equal, ',' => Key::Comma, '.' => Key::Dot,
                '/' => Key::Slash, ';' => Key::SemiColon, '\'' => Key::Quote,
                '[' => Key::LeftBracket, ']' => Key::RightBracket, '`' => Key::BackQuote,
                _ => Key::Space,
            }
        }
        _ => Key::Space,
    }
}

/// Get the bundle ID of the frontmost application (before we steal focus).
#[tauri::command]
fn get_frontmost_app() -> Result<Option<String>, String> {
    #[cfg(target_os = "macos")]
    {
        let output = Command::new("osascript")
            .args(["-e", "tell application \"System Events\" to get bundle identifier of (first process whose frontmost is true)"])
            .output()
            .map_err(|e| e.to_string())?;
        if output.status.success() {
            let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !s.is_empty() && !s.contains("error") {
                return Ok(Some(s));
            }
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = ();
    }
    Ok(None)
}

/// Activate (bring to front) an application by bundle ID.
#[tauri::command]
fn activate_app(bundle_id: String) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let script = format!(
            "tell application \"System Events\" to set frontmost of first process whose bundle identifier is \"{}\" to true",
            bundle_id.replace('"', "\\\"")
        );
        Command::new("osascript").args(["-e", &script]).output().map_err(|e| e.to_string())?;
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = bundle_id;
    }
    Ok(())
}

/// Press key combo (e.g. "Command+S", "Enter"). Requires Accessibility permission on macOS.
#[tauri::command]
fn press_keys(payload: String) -> Result<(), String> {
    let (modifiers, main) = parse_key_combo(&payload)?;
    // Press modifiers, then main key, then release in reverse order.
    for m in &modifiers {
        simulate(&EventType::KeyPress(*m)).map_err(|e| e.to_string())?;
        thread::sleep(Duration::from_millis(10));
    }
    simulate(&EventType::KeyPress(main)).map_err(|e| e.to_string())?;
    thread::sleep(Duration::from_millis(20));
    simulate(&EventType::KeyRelease(main)).map_err(|e| e.to_string())?;
    for m in modifiers.iter().rev() {
        thread::sleep(Duration::from_millis(10));
        simulate(&EventType::KeyRelease(*m)).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Send command to extension via WebSocket bridge. Returns true if sent to at least one connected extension.
#[tauri::command]
async fn send_to_extension(command: String, app: tauri::AppHandle) -> Result<bool, String> {
    let state = app.state::<ExtensionBridge>();
    if state.client_count.load(Ordering::SeqCst) == 0 {
        return Ok(false);
    }
    let tx = state.tx.lock().await;
    if let Some(sender) = tx.as_ref() {
        sender
            .send(command)
            .await
            .map_err(|e| format!("Bridge send error: {}", e))?;
        Ok(true)
    } else {
        Ok(false)
    }
}

struct ExtensionBridge {
    tx: Arc<tokio::sync::Mutex<Option<mpsc::Sender<String>>>>,
    client_count: Arc<AtomicUsize>,
    pub last_tab_url: Arc<tokio::sync::Mutex<Option<String>>>,
}

const WS_PORT: u16 = 8765;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Load .env from agent-app/ — use _override so .env wins over any stale shell env var
    if let Some(manifest_dir) = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent() {
        let env_path = manifest_dir.join(".env");
        if env_path.exists() {
            let _ = dotenvy::from_path_override(&env_path);
        }
    }

    let (cmd_tx, cmd_rx) = mpsc::channel::<String>(32);
    let client_count = Arc::new(AtomicUsize::new(0));
    let bridge = ExtensionBridge {
        tx: Arc::new(tokio::sync::Mutex::new(Some(cmd_tx))),
        client_count: Arc::clone(&client_count),
        last_tab_url: Arc::new(tokio::sync::Mutex::new(None)),
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_stt::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .manage(bridge)
        .setup(move |app| {
            let app_handle = app.handle().clone();
            let mut cmd_rx = cmd_rx;
            let client_count = Arc::clone(&client_count);
            tauri::async_runtime::spawn(async move {
                if let Err(e) = run_ws_bridge(&app_handle, &mut cmd_rx, client_count).await {
                    eprintln!("[bridge] ws server error: {}", e);
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            parse_intent,
            resolve_section,
            whisper_available,
            transcribe_audio,
            open_url,
            open_app,
            send_to_extension,
            press_keys,
            get_frontmost_app,
            activate_app
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

const PAGE_LOAD_DELAY_SECS: u64 = 3;

async fn handle_parse_and_run(
    app: &tauri::AppHandle,
    command: &str,
    clients: &Arc<tokio::sync::Mutex<Vec<WsWriter>>>,
) {
    let intent = match parse_intent(command.to_string(), None, app.clone()).await {
        Ok(i) => i,
        Err(e) => {
            let _ = app.emit("extension-result", format!("Parse error: {}", e));
            return;
        }
    };

    // Conversational intent: speak reply and send to extension for palette display
    if let Some(ref reply) = intent.chat_reply {
        let reply = reply.trim();
        if !reply.is_empty() {
            let _ = app.emit("extension-result", reply);
            let msg = tokio_tungstenite::tungstenite::Message::Text(
                serde_json::json!({ "type": "CHAT_REPLY", "message": reply }).to_string(),
            );
            let mut cl = clients.lock().await;
            for mut sender in std::mem::take(&mut *cl) {
                if sender.send(msg.clone()).await.is_ok() {
                    cl.push(sender);
                }
            }
            return;
        }
    }

    let mut just_opened_url = false;
    for step in &intent.steps {
        let action = step.action.to_lowercase();
        if just_opened_url && EXTENSION_EXECUTABLE.iter().any(|a| *a == action) {
            tokio::time::sleep(std::time::Duration::from_secs(PAGE_LOAD_DELAY_SECS)).await;
            just_opened_url = false;
        }
        if !EXTENSION_EXECUTABLE.iter().any(|a| *a == action) {
            continue;
        }
        let page_url = if let Some(bridge) = app.try_state::<ExtensionBridge>() {
            bridge.last_tab_url.lock().await.clone().unwrap_or_default()
        } else {
            String::new()
        };
        let Some(cmd) = format_step_for_extension(step, &page_url) else { continue };
        if step.action.to_lowercase() == "open" || step.action.to_lowercase() == "search" {
            just_opened_url = true;
        }
        let msg = tokio_tungstenite::tungstenite::Message::Text(cmd);
        let mut cl = clients.lock().await;
        let mut ok = Vec::new();
        for mut sender in std::mem::take(&mut *cl) {
            if sender.send(msg.clone()).await.is_ok() {
                ok.push(sender);
            }
        }
        *cl = ok;
    }
}

async fn run_ws_bridge(
    app: &tauri::AppHandle,
    cmd_rx: &mut mpsc::Receiver<String>,
    client_count: Arc<AtomicUsize>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use tokio::net::TcpListener;
    use tokio_tungstenite::accept_async;

    let listener = TcpListener::bind(("127.0.0.1", WS_PORT)).await?;
    eprintln!("[bridge] WebSocket server on ws://127.0.0.1:{}", WS_PORT);

    let clients: Arc<Mutex<Vec<WsWriter>>> = Arc::new(Mutex::new(Vec::new()));
    let clients_send = Arc::clone(&clients);

    loop {
        tokio::select! {
            cmd = cmd_rx.recv() => {
                if let Some(c) = cmd {
                    let msg = Message::Text(c);
                    let mut cl = clients_send.lock().await;
                    let mut ok = Vec::new();
                    for mut sender in std::mem::take(&mut *cl) {
                        if sender.send(msg.clone()).await.is_ok() {
                            ok.push(sender);
                        }
                        // Don't decrement here — read loop already does when client disconnects
                    }
                    *cl = ok;
                }
            }
            Ok((stream, _)) = listener.accept() => {
                if let Ok(ws) = accept_async(stream).await {
                    let (write, mut read) = ws.split();
                    clients_send.lock().await.push(write);
                    client_count.fetch_add(1, Ordering::SeqCst);
                    eprintln!("[bridge] Extension connected ({} total)", client_count.load(Ordering::SeqCst));
                    let app = app.clone();
                    let client_count = Arc::clone(&client_count);
                    let clients_for_broadcast = Arc::clone(&clients_send);
                    tauri::async_runtime::spawn(async move {
                        while let Some(Ok(msg)) = read.next().await {
                            if let Message::Text(text) = msg {
                                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
                                    if let Some(t) = v.get("type").and_then(|x| x.as_str()) {
                                        if t == "TAB_CONTEXT" {
                                            if let Some(url) = v.get("url").and_then(|x| x.as_str()) {
                                                if let Some(bridge) = app.try_state::<ExtensionBridge>() {
                                                    let _ = bridge.last_tab_url.lock().await.insert(url.to_string());
                                                }
                                            }
                                            continue;
                                        }
                                        if t == "PARSE_AND_RUN"
                                            && v.get("command").and_then(|c| c.as_str()).is_some()
                                        {
                                            let command = v["command"].as_str().unwrap_or("").to_string();
                                            if let Some(url) = v.get("url").and_then(|x| x.as_str()) {
                                                if let Some(bridge) = app.try_state::<ExtensionBridge>() {
                                                    let _ = bridge.last_tab_url.lock().await.insert(url.to_string());
                                                }
                                            }
                                            let app_clone = app.clone();
                                            let clients_clone = Arc::clone(&clients_for_broadcast);
                                            tauri::async_runtime::spawn(async move {
                                                handle_parse_and_run(&app_clone, &command, &clients_clone).await;
                                            });
                                            continue;
                                        }
                                    }
                                    let m = v.get("message").and_then(|x| x.as_str()).unwrap_or("");
                                    let _ = app.emit("extension-result", m);
                                }
                            }
                        }
                        client_count.fetch_sub(1, Ordering::SeqCst);
                        eprintln!("[bridge] Extension disconnected");
                    });
                }
            }
        }
    }
}
