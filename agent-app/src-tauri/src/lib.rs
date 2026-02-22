use base64::{
    engine::general_purpose::{GeneralPurpose, GeneralPurposeConfig},
    Engine,
};
use futures_util::{SinkExt, StreamExt};
use reqwest::multipart;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
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
}

#[tauri::command]
async fn parse_intent(command: String, api_key_override: Option<String>) -> Result<ParsedIntent, String> {
    let api_key = api_key_override
        .filter(|k| !k.is_empty())
        .or_else(|| std::env::var("OPENAI_API_KEY").ok())
        .ok_or("No API key. Set OPENAI_API_KEY env var or add your key in Settings.")?;

    let system_prompt = r#"You are an intelligent assistant that turns natural speech into executable plans. The user speaks freely—understand intent and break compound requests into steps.

Return ONLY valid JSON (no markdown, no explanation):
{"steps": [{"action": string, "payload": string, "target_type": "website"|"native_app"|null}, ...]}

AVAILABLE ACTIONS (use whichever fit the intent):
- open: Go to a website or launch app. payload = site name (youtube, wikipedia, amazon...). target_type: "website" (default) or "native_app" for desktop apps only.
- search: Web search (DuckDuckGo). payload = query
- time, date, stop: payload = ""
- click: Click/play/select on the CURRENT PAGE. payload = what to click (preserve ordinals: first video, second result)
- find: Find and highlight text on page. payload = phrase to find
- find_and_read: Find text, scroll to it, read it aloud. payload = what to find (e.g. "price", "requirements", "deadline")
- page_search: Type into search box ON the current page. payload = query
- scroll: payload = "up"|"down"|"top"|"bottom" or "down:3pages"
- go_to: Scroll to a section by meaning. payload = their phrase (e.g. "his education", "pricing")
- access_mode: payload = "on"|"off"
- close_popup: payload = ""
- open_and_search: Open site THEN search on it. payload = "site|query"

CRITICAL—PLANNING:
- "Find info on Wikipedia about X and scroll to it" → steps: [open wikipedia] → [page_search X] → [find_and_read X]
- "Open Amazon, search for shoes, click the first one" → steps: [open amazon] → [page_search shoes] → [click first result]
- "Scroll down and show me the price" → steps: [scroll down] → [find_and_read price]
- Single requests = one step. Compound requests = multiple steps in logical order.
- Infer context: "search for it" on a product page = page_search. "search the web" = search.
- Understand synonyms: "look up", "find", "get info on", "go to section" — map to the right action."#;

    let client = reqwest::Client::new();
    let res = client
        .post("https://api.openai.com/v1/chat/completions")
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&serde_json::json!({
            "model": "gpt-4o",
            "messages": [
                {"role": "system", "content": system_prompt},
                {"role": "user", "content": command}
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

    // Support both formats: {"steps": [...]} or legacy {"action", "payload"}
    let value: serde_json::Value = serde_json::from_str(content).map_err(|e| format!("Parse error: {}", e))?;
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
        return Ok(ParsedIntent {
            steps: vec![Step {
                action: "search".to_string(),
                payload: command,
                target_type: None,
            }],
        });
    };

    let valid = ["open", "search", "time", "date", "stop", "open_and_search", "click", "find", "find_and_read", "page_search", "scroll", "access_mode", "close_popup", "go_to"];
    steps.retain(|s| valid.contains(&s.action.as_str()));
    if steps.is_empty() {
        steps = vec![Step {
            action: "search".to_string(),
            payload: command,
            target_type: None,
        }];
    }

    Ok(ParsedIntent { steps })
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
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_stt::init())
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
            send_to_extension
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

type WsWriter = futures_util::stream::SplitSink<
    tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>,
    Message,
>;

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
                    tauri::async_runtime::spawn(async move {
                        while let Some(Ok(msg)) = read.next().await {
                            if let Message::Text(text) = msg {
                                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
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
