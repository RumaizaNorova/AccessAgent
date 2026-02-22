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

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct ParsedIntent {
    pub action: String,
    pub payload: String,
}

#[tauri::command]
async fn parse_intent(command: String, api_key_override: Option<String>) -> Result<ParsedIntent, String> {
    let api_key = api_key_override
        .filter(|k| !k.is_empty())
        .or_else(|| std::env::var("OPENAI_API_KEY").ok())
        .ok_or("No API key. Set OPENAI_API_KEY env var or add your key in Settings.")?;

    let system_prompt = r#"You are a smart command parser for a voice-controlled desktop accessibility agent. The user speaks naturally—your job is to understand intent, not just match keywords.

Return ONLY valid JSON (no markdown, no explanation): {"action": string, "payload": string}

Actions and payloads:
- open: Launch an app, open a URL, or go to a website. payload = app name, URL, or site (e.g. "chrome", "spotify", "wikipedia", "youtube.com", "amazon")
  Examples: "open chrome", "launch spotify", "go to wikipedia", "open up youtube", "start slack", "take me to google"
- search: Web search (opens search engine). Use ONLY when user says "google", "search the web", "look up on the internet", or asks a general knowledge question. payload = search query
  Examples: "google best headphones", "search the web for restaurants", "look up python tutorials online", "what is the capital of France"
- time: Get the current time. payload = ""
  Examples: "what time is it", "time", "current time", "what's the time"
- date: Get today's date. payload = ""
  Examples: "what's the date", "date", "what day is it", "today's date"
- stop: Cancel or stop. payload = ""
  Examples: "stop", "cancel", "never mind", "forget it"
- click: Click a button, link, or element on the current webpage. payload = what to click (e.g. "buy button", "submit", "login", "continue", "next")
  Examples: "click the buy button", "click submit", "click continue", "press next", "hit the login button"
- find: Find and highlight text or element on the page (scroll to it). payload = what to find
  Examples: "find login", "find the requirements", "scroll to contact", "show me the price"
- page_search: Search box ON THE CURRENT PAGE (e.g. Amazon, any site). Use when user says "search for X", "look for X", "find X", or "on this page/search here/on the site I'm on" + search. payload = search query only (e.g. "candles")
  Examples: "search for candles", "search for shoes", "on this page search for candles", "on the website I'm on search for candles", "see the page I'm at and search for X" → page_search with payload "X"
- scroll: Scroll the page. payload = "up", "down", "top", or "bottom"
  Examples: "scroll down", "scroll up", "go to top", "scroll to bottom"
- access_mode: Toggle one-hand / target boost mode. payload = "on" or "off"
  Examples: "access mode on", "target boost off"
- open_and_search: Open a website THEN search on it. payload = "site|query" (e.g. "amazon|candles", "wikipedia|Albert Einstein"). Site can be "amazon", "amazon.com", "wikipedia", etc.
  Examples: "open Amazon and search for candles", "go to Amazon and look for shoes", "open Wikipedia and search for Einstein", "take me to eBay and search for headphones"

Rules:
- open vs click: open = navigate to new URL or launch app. click = interact with element on current page.
- search vs page_search: DEFAULT to page_search for "search for X" — user is usually on a site. Use search (web) only when they say "google", "on the web", or ask a knowledge question.
- open_and_search: Use when user says "open X and search for Y" or "go to X and look for Y" — one command does both.
- Understand synonyms: "launch", "go to" → open. "press", "hit" → click. "find", "show me", "scroll to" → find.
- Preserve the user's exact words in payload when relevant."#;

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
            "max_tokens": 256,
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

    let intent: ParsedIntent = serde_json::from_str(content).map_err(|e| format!("Parse error: {}", e))?;

    // Validate action (agent handles some, extension handles in-page)
    let agent_actions = ["open", "search", "time", "date", "stop"];
    let extension_actions = ["click", "find", "page_search", "scroll", "access_mode", "open_and_search"];
    let valid: Vec<&str> = agent_actions.iter().chain(extension_actions.iter()).copied().collect();
    if !valid.contains(&intent.action.as_str()) {
        return Ok(ParsedIntent {
            action: "search".to_string(),
            payload: command,
        });
    }

    Ok(intent)
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
