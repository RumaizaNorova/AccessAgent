# "Invalid argspace 64" Error — Root Cause Analysis

## Where the error comes from

### 1. **Most likely source: Base64 decode failure (Rust)**

- **Location:** `agent-app/src-tauri/src/lib.rs` → `decode_base64_audio()` → base64 crate
- **Error format:** `"Invalid symbol 64, offset X."` or `"Invalid last symbol 64, offset X."`
- **Cause:** The Rust base64 crate reports an invalid byte (ASCII 64 = `@`) at some offset.
- **Flow:** `transcribe_audio` receives base64 from the frontend → decodes → fails → returns `Err(...)` → frontend catches → passes to `speak()`.
- **Why it happens:** Safari/WebKit `MediaRecorder` + `FileReader.readAsDataURL()` can produce slightly different base64/encoding. Rare edge cases (empty chunks, short recordings, timing) can corrupt the stream.

### 2. **Alternative source: Whisper API error**

- **Location:** Same `transcribe_audio`, when `res.status().is_success()` is false.
- **Possible messages:** `"Invalid file format"`, `"Unrecognized file format"`, JSON with `"error":{"message":"..."}`.
- **Flow:** Same as above; friendly messages are already mapped in Rust, but the raw body could still be exposed if parsing fails.

### 3. **Alternative source: Vosk / tauri-plugin-stt**

- **Location:** `main.ts` lines 324–335 (onError) and 345–359 (catch).
- **When:** Only if `whisper_available` is **false** (no API key or `.env` not loaded).
- **Flow:** App uses Vosk instead of Whisper → `onError` or `sttStart` catch receives `err.message` → passed to `speak()`.
- **Why it matters:** If `.env` isn’t loaded correctly at startup, the app falls back to Vosk and its native errors can be spoken raw.

### 4. **Alternative source: “Searching for X”**

- **Location:** `main.ts` line 135: `speak(\`Searching for ${parsed.payload}\`)`
- **Flow:** Whisper returns garbage (e.g. `"invalid argspace 64"`) → treated as transcript → parsed as search → `speak("Searching for invalid argspace 64")`.
- **Mitigation:** `transcriptLooksLikeError()` filters obvious error-like transcripts before running commands.

---

## Why previous fixes didn’t stick

1. **HMR disabled** (`vite.config.ts`, `hmr: false`): The WebView does not hot-reload on save. A full restart of `npm run tauri dev` is required for frontend changes to take effect.
2. **Two error paths bypass friendly mapping:**  
   - `onError` (line 335): `speak(err.message)` without `toFriendlyTranscriptionError()`.  
   - `startListening` catch (line 359): `speak("Voice error: " + msg)` without `toFriendlyTranscriptionError()`.
3. **Filters may miss variations:** Different error formats or word order may slip through the current checks.

---

## Fix checklist

- [x] Map base64 errors in Rust (done)
- [x] Map Whisper API errors in Rust (done)
- [x] Filter in `speak()` (done)
- [x] `transcriptLooksLikeError()` for Whisper output (done)
- [x] Pass Vosk `onError` and `startListening` catch through `toFriendlyTranscriptionError()` (done)

## How to ensure your changes run

1. **HMR is disabled** — the WebView does not auto-reload on file save.
2. **Fully restart the app:**
   - Stop `npm run tauri dev` (Ctrl+C).
   - Run `npm run tauri dev` again.
3. **If it still fails, do a clean rebuild:**
   ```bash
   cd agent-app
   rm -rf node_modules/.vite
   npm run tauri dev
   ```
