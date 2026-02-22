# AccessPilot Agent

A voice-controlled desktop agent — small floating icon. Click to talk, speak your command, click again to run it. Uses OpenAI Whisper for transcription and GPT-4o for natural-language understanding.

## Prerequisites

1. **Rust** — [Install Rust](https://www.rust-lang.org/learn/get-started)
2. **macOS** — Built for macOS
3. **OpenAI API key** — Required. Set `OPENAI_API_KEY` in `agent-app/.env`. Get one at https://platform.openai.com/api-keys

## Run

```bash
cd agent-app
npm install
npm run tauri dev
```

## Use

1. A small blue icon appears in the corner
2. **Click** the icon to start recording — speak your command
3. **Click again** to stop and run the command
4. Examples: `open wikipedia`, `search scholarships`, `open Chrome`, `what time is it`

## Commands

| Say | Does |
|-----|------|
| open wikipedia | Opens in browser |
| open Chrome | Opens the app |
| search for X | DuckDuckGo search |
| what time is it | Speaks time |
| what's the date | Speaks date |
| stop | Stops / cancels |

## Build

```bash
cd agent-app
npm run tauri build
```

Output in `src-tauri/target/release/bundle/`.

## Development

HMR is disabled. After changing frontend code, fully restart the app (Ctrl+C, then `npm run tauri dev` again) for changes to take effect.
