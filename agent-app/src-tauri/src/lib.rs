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

/// Strip trailing/leading punctuation so "Hi!", "Hello.", "hey?" all normalize to the base word.
fn normalize_for_check(s: &str) -> String {
    let t = s.trim()
        .trim_start_matches(|c: char| c.is_ascii_punctuation())
        .trim_end_matches(|c: char| c.is_ascii_punctuation());
    t.trim().to_lowercase()
}

/// Fast-path: obvious conversational input — NEVER treat as search/command.
/// Catches greetings, small talk, thanks, etc. so we never accidentally search for "hey".
fn is_likely_conversation(s: &str) -> bool {
    let t = s.trim();
    if t.is_empty() || t.len() > 80 {
        return false;
    }
    let normalized = normalize_for_check(t);
    let lower = t.to_lowercase();
    // Must NOT contain command verbs — "search for hey" or "write how are you in here" is a command
    let has_command_verb = ["open ", "search ", "search for", "click ", "find ", "find and ", "scroll ", "go to ", "go to the ", "close the ", "close slack", "open gmail", "open amazon", "write ", "type "].iter().any(|v| lower.contains(v));
    if has_command_verb {
        return false;
    }
    // Greetings — normalized "Hi!" -> "hi", "Hello." -> "hello"
    let greetings = ["hey", "hi", "hello", "howdy", "yo", "hiya", "hey there", "hi there", "hello there"];
    if greetings.iter().any(|g| normalized == *g || normalized.starts_with(&format!("{} ", g))) {
        return true;
    }
    // "good morning" etc.
    if lower.starts_with("good morning") || lower.starts_with("good afternoon") || lower.starts_with("good evening") || lower.starts_with("good night") {
        return true;
    }
    // How are you, what's up (short phrases only)
    if (lower.contains("how are you") || lower.contains("how're you")
        || lower.contains("hows it going") || lower.contains("how's it going"))
        && t.len() < 50
    {
        return true;
    }
    if (lower.contains("whats up") || lower.contains("what's up") || lower.contains("whats up")) && t.len() < 35 {
        return true;
    }
    if (lower.contains("lets talk") || lower.contains("let's talk")) && t.len() < 30 {
        return true;
    }
    // Thanks (normalized handles "Thanks!")
    if normalized == "thanks" || normalized.starts_with("thank you") || normalized.starts_with("thanks ") {
        return true;
    }
    // Bye
    if normalized == "bye" || normalized == "goodbye" || normalized.starts_with("bye ") || normalized.starts_with("goodbye ") {
        return true;
    }
    // Brief acknowledgments
    if ["yes", "no", "ok", "okay", "sure", "yep", "nope", "alright", "cool"].contains(&normalized.as_str()) {
        return true;
    }
    // "say hi", "just wanted to say hi"
    if normalized == "say hi" || normalized.starts_with("say hi ") || normalized.starts_with("just saying hi") {
        return true;
    }
    false
}

/// Bundle ID → human-readable name for context. Helps GPT understand "user is in Notion".
fn frontmost_app_display(bundle_id: Option<&str>) -> String {
    let bid = match bundle_id {
        Some(b) if !b.is_empty() => b.to_lowercase(),
        _ => return "unknown".to_string(),
    };
    if bid.contains("chrome") || bid.contains("chromium") {
        "Chrome"
    } else if bid.contains("notion") {
        "Notion"
    } else if bid.contains("slack") {
        "Slack"
    } else if bid.contains("mail") && !bid.contains("gmail") {
        "Mail"
    } else if bid.contains("messages") {
        "Messages"
    } else if bid.contains("notes") {
        "Notes"
    } else if bid.contains("safari") {
        "Safari"
    } else if bid.contains("firefox") {
        "Firefox"
    } else if bid.contains("code") || bid.contains("cursor") {
        "Editor"
    } else if bid.contains("outlook") {
        "Outlook"
    } else if bid.contains("finder") {
        "Finder"
    } else {
        "other app"
    }
    .to_string()
}

#[tauri::command]
async fn parse_intent(
    command: String,
    api_key_override: Option<String>,
    frontmost_app: Option<String>,
    app: tauri::AppHandle,
) -> Result<ParsedIntent, String> {
    let trimmed = command.trim();
    if trimmed.is_empty() {
        return Ok(ParsedIntent {
            steps: vec![],
            chat_reply: Some("What can I help you with?".into()),
        });
    }

    // Fast-path: never search for greetings/small talk — always reply conversationally
    if is_likely_conversation(trimmed) {
        let reply = match trimmed.to_lowercase().as_str() {
            s if s.starts_with("how are you") || s.contains("how are you") => "I'm doing well, thanks for asking! How can I help you today?",
            s if s.contains("whats up") || s.contains("what's up") => "Not much! What can I do for you?",
            s if s.starts_with("good morning") => "Good morning! How can I help?",
            s if s.starts_with("good afternoon") => "Good afternoon! What would you like to do?",
            s if s.starts_with("good evening") || s.starts_with("good night") => "Good evening! How can I assist you?",
            s if s == "thanks" || s.starts_with("thank you") || s.starts_with("thanks ") => "You're welcome! Anything else?",
            s if s == "bye" || s.starts_with("bye ") || s.starts_with("goodbye") => "Bye! Take care.",
            s if s.contains("lets talk") || s.contains("let's talk") => "Sure! I'm here to chat. What's on your mind?",
            s if s == "say hi" || s.starts_with("say hi") => "Hi there! Great to hear from you. What can I help with?",
            _ => "Hey! What can I do for you?",
        };
        return Ok(ParsedIntent {
            steps: vec![],
            chat_reply: Some(reply.into()),
        });
    }

    let api_key = api_key_override
        .filter(|k| !k.is_empty())
        .or_else(|| std::env::var("OPENAI_API_KEY").ok())
        .ok_or("No API key. Set OPENAI_API_KEY env var or add your key in Settings.")?;

    let page_context = if let Some(bridge) = app.try_state::<ExtensionBridge>() {
        bridge.last_tab_url.lock().await.as_ref().cloned().unwrap_or_default()
    } else {
        String::new()
    };

    let frontmost_display = frontmost_app_display(frontmost_app.as_deref());

    let system_prompt = r#"You are a friendly assistant. Users talk to you OR give commands. You MUST decide which.

CRITICAL: CONVERSATION vs COMMAND
- CONVERSATION = greetings (hi, hello, hey — with or without ! or .), small talk, venting, thanks, questions about you, chitchat, reactions. NEVER search for these. NEVER output search/find with the user's words as payload.
- "Hi!", "Hello.", "hey" = ALWAYS conversation. Never search for "Hi" or "Hello".
- Page URL does NOT change this. User on Google saying "hi" = CONVERSATION.
- When unsure → CONVERSATION. Err on the side of talking, not searching. Single greetings = always conversation.

Output ONLY one:
1. CONVERSATION: {"intent":"conversation","chat_reply":"your friendly reply"}
2. COMMAND: {"intent":"command","steps":[{"action","payload","target_type"}]}

CONVERSATION: hey, hi, hello, how are you, what's up, let's talk, I had a rough day, thanks, good morning, bye, can you believe X, what do you think.
COMMAND: open gmail, search for laptops, find the price, click submit.

PAYLOAD: Extract concrete concepts. "find the eligibility" → payload "eligibility". "where does it say the deadline" → "deadline". "show me his parents" → "parents". Never use vague payloads like "the information" or "it".
WRONG: User says "hey" → search. NEVER do that. RIGHT: {"intent":"conversation","chat_reply":"Hey! What can I do for you?"}

ACTIONS: open, search, time, date, stop, click, find, find_and_read, find_next, find_prev, page_search, scroll, go_to, go_to_page, access_mode, close_popup, open_and_search, press_keys, type.
PDF/DOCUMENT (when on PDF or doc viewer): "go to page 50", "page 50", "open page 50" = go_to_page 50. "scroll 5 pages down", "scroll down 5 pages" = scroll down:5. "find phony", "find the word X", "go to chapter 3" = find X (or go_to if sections exist).
COMPOUND: "Go to X and search/look for Y", "look for Y in X", "open X and go to Y" = open_and_search "X|Y" (site|query). "look for imagenet in wikipedia" → "wikipedia|imagenet". Never add site:.org — we type into the site's search box.

NATIVE APPS — ALWAYS use target_type: "native_app" for these. NEVER open them as web URLs:
Slack, Mail, Notes, Messages, Finder, Terminal, System Settings, Calendar, Reminders, Safari, Discord, Zoom, Spotify, Microsoft Teams, Outlook, OneNote, Notion, Visual Studio Code, Xcode.
Mappings: "email" or "mail"→Mail, "messages" or "imessage"→Messages, "notes"→Notes, "finder"→Finder, "settings" or "system preferences"→System Settings.
Web-only (target_type null): Gmail, Amazon, Wikipedia, YouTube, Google, etc. — open in browser.

TYPE: "type hello", "write how are you", "type john at gmail dot com" = type payload into focused text field. "write X in here" / "type X here" = type X. Voice substitutions: "at"→@, "dot"→. Preserve literal text. No Enter/submit unless user says "and submit".
KEYBOARD: "save"→press_keys Command+S, "close [app]"→Command+Q, "close dialog"→close_popup or Escape.
CONTEXT: User's frontmost app (what they're looking at) and Chrome tab URL. "in here" = the app they're focused on. If frontmost is Chrome → type/search/click apply to the Chrome tab. If frontmost is Notion/Slack/Mail/etc → user wants to type there; we can only type in Chrome for now—output type anyway.
PAGE CONTEXT (when URL given): youtube.com→click "first video" not logo; wikipedia→find_and_read; on-page locate=find/find_and_read; web search=search. NEVER use page_search for conversational text like "how are you" when user said "write X in here"—that's type, not search. Ambiguous→on-page."#;

    // Put user message first; add context so GPT knows what the user is looking at
    let user_content = {
        let mut ctx = String::new();
        ctx.push_str(&format!("Frontmost app: {}. ", frontmost_display));
        if !page_context.is_empty() {
            ctx.push_str(&format!("Chrome tab: {}", page_context));
        } else {
            ctx.push_str("No Chrome tab.");
        }
        format!("User: {}\n({})", command, ctx.trim())
    };

    let messages: Vec<serde_json::Value> = vec![
        serde_json::json!({"role": "system", "content": system_prompt}),
        serde_json::json!({"role": "user", "content": "Let's talk with you. How are you doing?"}),
        serde_json::json!({"role": "assistant", "content": r#"{"intent":"conversation","chat_reply":"I'm doing well, thanks! Always happy to chat. What's on your mind?"}"#}),
        serde_json::json!({"role": "user", "content": "hey"}),
        serde_json::json!({"role": "assistant", "content": r#"{"intent":"conversation","chat_reply":"Hey! What can I do for you?"}"#}),
        serde_json::json!({"role": "user", "content": "how are you"}),
        serde_json::json!({"role": "assistant", "content": r#"{"intent":"conversation","chat_reply":"I'm doing well, thanks for asking! How can I help?"}"#}),
        serde_json::json!({"role": "user", "content": "open gmail"}),
        serde_json::json!({"role": "assistant", "content": r#"{"intent":"command","steps":[{"action":"open","payload":"gmail","target_type":null}]}"#}),
        serde_json::json!({"role": "user", "content": "open Slack"}),
        serde_json::json!({"role": "assistant", "content": r#"{"intent":"command","steps":[{"action":"open","payload":"Slack","target_type":"native_app"}]}"#}),
        serde_json::json!({"role": "user", "content": "open Mail"}),
        serde_json::json!({"role": "assistant", "content": r#"{"intent":"command","steps":[{"action":"open","payload":"Mail","target_type":"native_app"}]}"#}),
        serde_json::json!({"role": "user", "content": "open Notes"}),
        serde_json::json!({"role": "assistant", "content": r#"{"intent":"command","steps":[{"action":"open","payload":"Notes","target_type":"native_app"}]}"#}),
        serde_json::json!({"role": "user", "content": "open my email"}),
        serde_json::json!({"role": "assistant", "content": r#"{"intent":"command","steps":[{"action":"open","payload":"Mail","target_type":"native_app"}]}"#}),
        serde_json::json!({"role": "user", "content": "search for best laptops 2024"}),
        serde_json::json!({"role": "assistant", "content": r#"{"intent":"command","steps":[{"action":"search","payload":"best laptops 2024","target_type":null}]}"#}),
        serde_json::json!({"role": "user", "content": "go to Wikipedia and look for Geoffrey Hinton"}),
        serde_json::json!({"role": "assistant", "content": r#"{"intent":"command","steps":[{"action":"open_and_search","payload":"wikipedia|Geoffrey Hinton","target_type":null}]}"#}),
        serde_json::json!({"role": "user", "content": "look for imagenet in wikipedia"}),
        serde_json::json!({"role": "assistant", "content": r#"{"intent":"command","steps":[{"action":"open_and_search","payload":"wikipedia|imagenet","target_type":null}]}"#}),
        serde_json::json!({"role": "user", "content": "open wikipedia and go to imagenet"}),
        serde_json::json!({"role": "assistant", "content": r#"{"intent":"command","steps":[{"action":"open_and_search","payload":"wikipedia|imagenet","target_type":null}]}"#}),
        serde_json::json!({"role": "user", "content": "User: hey\n(Current page: https://www.google.com)"}),
        serde_json::json!({"role": "assistant", "content": r#"{"intent":"conversation","chat_reply":"Hey! What can I do for you?"}"#}),
        serde_json::json!({"role": "user", "content": "go to page 50"}),
        serde_json::json!({"role": "assistant", "content": r#"{"intent":"command","steps":[{"action":"go_to_page","payload":"50","target_type":null}]}"#}),
        serde_json::json!({"role": "user", "content": "scroll 5 pages down"}),
        serde_json::json!({"role": "assistant", "content": r#"{"intent":"command","steps":[{"action":"scroll","payload":"down 5","target_type":null}]}"#}),
        serde_json::json!({"role": "user", "content": "find the word phony"}),
        serde_json::json!({"role": "assistant", "content": r#"{"intent":"command","steps":[{"action":"find","payload":"phony","target_type":null}]}"#}),
        serde_json::json!({"role": "user", "content": "type hello"}),
        serde_json::json!({"role": "assistant", "content": r#"{"intent":"command","steps":[{"action":"type","payload":"hello","target_type":null}]}"#}),
        serde_json::json!({"role": "user", "content": "type john at gmail dot com"}),
        serde_json::json!({"role": "assistant", "content": r#"{"intent":"command","steps":[{"action":"type","payload":"john at gmail dot com","target_type":null}]}"#}),
        serde_json::json!({"role": "user", "content": "User: write how are you in here\n(Frontmost app: Notion. Chrome tab: https://google.com)"}),
        serde_json::json!({"role": "assistant", "content": r#"{"intent":"command","steps":[{"action":"type","payload":"how are you","target_type":null}]}"#}),
        serde_json::json!({"role": "user", "content": user_content}),
    ];

    let client = reqwest::Client::new();
    let res = client
        .post("https://api.openai.com/v1/chat/completions")
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&serde_json::json!({
            "model": "gpt-4o",
            "messages": messages,
            "response_format": {"type": "json_object"},
            "max_tokens": 512,
            "temperature": 0.4
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

    // Parse: intent-first schema or legacy chat_reply/steps
    let value: serde_json::Value = serde_json::from_str(content).map_err(|e| format!("Parse error: {}", e))?;

    let intent = value.get("intent").and_then(|v| v.as_str());
    let chat_reply_str = value.get("chat_reply").and_then(|v| v.as_str());

    // Conversational intent: intent=conversation OR has chat_reply (legacy) with no command steps
    if intent == Some("conversation") || (intent != Some("command") && chat_reply_str.is_some()) {
        if let Some(reply) = chat_reply_str {
            let reply = reply.trim().to_string();
            if !reply.is_empty() {
                return Ok(ParsedIntent {
                    steps: vec![],
                    chat_reply: Some(reply),
                });
            }
        }
        // intent=conversation but empty reply → generic friendly response
        if intent == Some("conversation") {
            return Ok(ParsedIntent {
                steps: vec![],
                chat_reply: Some("What can I help you with?".into()),
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
        vec![] // No search fallback—ask for clarification instead
    };

    let valid = ["open", "search", "time", "date", "stop", "open_and_search", "click", "find", "find_and_read", "find_next", "find_prev", "page_search", "scroll", "go_to_page", "access_mode", "close_popup", "go_to", "press_keys", "type"];
    steps.retain(|s| valid.contains(&s.action.as_str()));

    // When parsing failed or no valid steps: conversational clarification, never default to search
    if steps.is_empty() {
        return Ok(ParsedIntent {
            steps: vec![],
            chat_reply: Some("I didn't quite get that. Could you say more, or try a command like: open Gmail, search for something, find text on the page.".into()),
        });
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

    // Last-resort: model wrongly returned search/find with user's exact words — that's almost always convo
    let single_action = steps.len() == 1;
    let payload_matches_user = single_action
        && normalize_for_check(&steps[0].payload) == normalize_for_check(command);
    let action_is_search_or_find = single_action
        && (steps[0].action.eq_ignore_ascii_case("search") || steps[0].action.eq_ignore_ascii_case("find"));
    if payload_matches_user && action_is_search_or_find {
        // User said "Hi!" and we got search "Hi!" — clearly wrong. Reply conversationally.
        let norm = normalize_for_check(command);
        let reply = if norm.contains("how are you") || norm.contains("how're you") {
            "I'm doing well, thanks for asking! How can I help?"
        } else if norm.contains("whats up") || norm.contains("what's up") {
            "Not much! What can I do for you?"
        } else if norm.starts_with("thanks") || norm.starts_with("thank you") {
            "You're welcome! Anything else?"
        } else if norm == "bye" || norm.starts_with("goodbye") {
            "Bye! Take care."
        } else {
            "Hey! What can I do for you?"
        };
        return Ok(ParsedIntent {
            steps: vec![],
            chat_reply: Some(reply.into()),
        });
    }

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

/// Strip site:.org etc from search payloads — GPT adds web-search modifiers; in-page search doesn't use them.
fn strip_site_modifier(payload: &str) -> String {
    payload
        .trim()
        .split_whitespace()
        .filter(|w| !w.to_lowercase().starts_with("site:"))
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string()
}

/// Format a step for the extension (matches main.ts formatExtensionCommand).
fn format_step_for_extension(step: &Step, page_url: &str) -> Option<String> {
    let action = step.action.to_lowercase();
    let payload = if action == "click" {
        correct_youtube_click_payload(step.payload.trim(), page_url)
    } else if action == "search" || action == "page_search" {
        strip_site_modifier(&step.payload)
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
        "go_to_page" => format!("go_to_page:{payload}"),
        "type" => format!("type {payload}"),
        _ => return None,
    };
    Some(cmd)
}

/// Steps that the extension can execute (open, search navigate from content script).
const EXTENSION_EXECUTABLE: &[&str] = &[
    "open", "search", "click", "find", "find_and_read", "find_next", "find_prev",
    "page_search", "scroll", "go_to_page", "access_mode", "close_popup", "go_to", "type",
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
    let intent = match parse_intent(command.to_string(), None, None, app.clone()).await {
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
