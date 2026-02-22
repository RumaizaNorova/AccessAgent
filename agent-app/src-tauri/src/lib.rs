use base64::{
    engine::general_purpose::{GeneralPurpose, GeneralPurposeConfig},
    Engine,
};
use reqwest::multipart;
use std::process::Command;
use tauri::Manager;

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
- search: Look something up on the web. payload = search query
  Examples: "search for restaurants nearby", "look up python tutorials", "find hotels in Paris", "google best headphones", "what is the capital of France", "how do I fix a flat tire"
- time: Get the current time. payload = ""
  Examples: "what time is it", "time", "current time", "what's the time"
- date: Get today's date. payload = ""
  Examples: "what's the date", "date", "what day is it", "today's date"
- stop: Cancel or stop. payload = ""
  Examples: "stop", "cancel", "never mind", "forget it"

Rules:
- Understand synonyms: "launch", "go to", "open up", "start", "take me to" → open. "look up", "find", "google", "search for" → search.
- For questions like "what is X" or "how do I X" → search with the full question as payload.
- Be generous with understanding: "play some music" → open spotify. "check my email" → open mail.
- If the user names a website (e.g. "reddit", "github") → open with that as payload.
- If truly ambiguous, prefer "search" with full input as payload.
- Preserve the user's exact words in payload when relevant (e.g. search query)."#;

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

    // Validate action
    let valid = ["open", "search", "time", "date", "stop"];
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
        Command::new("open").arg(&url).spawn().map_err(|e| e.to_string())?;
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Load .env from agent-app/ — use _override so .env wins over any stale shell env var
    if let Some(manifest_dir) = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent() {
        let env_path = manifest_dir.join(".env");
        if env_path.exists() {
            let _ = dotenvy::from_path_override(&env_path);
        }
    }
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_stt::init())
        .invoke_handler(tauri::generate_handler![
            parse_intent,
            whisper_available,
            transcribe_audio,
            open_url,
            open_app
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
