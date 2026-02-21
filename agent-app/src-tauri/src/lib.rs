use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use reqwest::multipart;
use std::process::Command;

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

    let system_prompt = r#"You are a command parser for a desktop accessibility agent. The user speaks or types a command. Your job is to extract the intent.

Return ONLY valid JSON with exactly this shape (no markdown, no explanation):
{"action": "open"|"search"|"time"|"date"|"stop", "payload": "string"}

Rules:
- "open X" → action: "open", payload: X (URL, app name, or site like "wikipedia")
- "search for X" or "look up X" → action: "search", payload: X
- "what time is it" / "time" → action: "time", payload: ""
- "what's the date" / "date" → action: "date", payload: ""
- "stop" / "cancel" → action: "stop", payload: ""
- If unclear but seems like opening something → "open"
- If unclear but seems like search → "search"
- Default for ambiguous: "search" with full input as payload"#;

    let client = reqwest::Client::new();
    let res = client
        .post("https://api.openai.com/v1/chat/completions")
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&serde_json::json!({
            "model": "gpt-4o-mini",
            "messages": [
                {"role": "system", "content": system_prompt},
                {"role": "user", "content": command}
            ],
            "response_format": {"type": "json_object"},
            "max_tokens": 128,
            "temperature": 0.1
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

/// Check if Whisper transcription is available (API key set)
#[tauri::command]
async fn whisper_available() -> bool {
    std::env::var("OPENAI_API_KEY").map(|k| !k.is_empty()).unwrap_or(false)
}

/// Transcribe audio using OpenAI Whisper API. Accepts base64-encoded audio.
/// Supported formats: webm, mp3, mp4, mpeg, mpga, m4a, wav
#[tauri::command]
async fn transcribe_audio(base64_audio: String, mime_type: Option<String>) -> Result<String, String> {
    let api_key = std::env::var("OPENAI_API_KEY")
        .map_err(|_| "No API key. Set OPENAI_API_KEY env var.")?
        .trim()
        .to_string();
    if api_key.is_empty() {
        return Err("No API key. Set OPENAI_API_KEY env var.".into());
    }

    let audio_bytes = BASE64
        .decode(&base64_audio)
        .map_err(|e| format!("Invalid base64 audio: {}", e))?;

    let (file_name, mime) = match mime_type.as_deref() {
        Some("audio/webm") | None => ("audio.webm", "audio/webm"),
        Some("audio/mpeg") => ("audio.mp3", "audio/mpeg"),
        Some("audio/mp4") => ("audio.m4a", "audio/mp4"),
        Some("audio/wav") => ("audio.wav", "audio/wav"),
        Some(m) => ("audio.webm", m),
    };

    let form = multipart::Form::new()
        .part(
            "file",
            multipart::Part::bytes(audio_bytes)
                .file_name(file_name)
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
        return Err(format!("Whisper API error ({}): {}", status, text));
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
    // Load .env from agent-app/ (parent of src-tauri)
    if let Some(manifest_dir) = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent() {
        let env_path = manifest_dir.join(".env");
        let _ = dotenvy::from_path(env_path);
    }
    let _ = dotenvy::dotenv();
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
