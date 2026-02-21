# AccessPilot Agent

A **typing-first desktop agent** — small icon in the corner. Type commands: open apps, search the web, get the time. Uses OpenAI for natural-language understanding (fallback to rules when offline).

## Prerequisites

1. **Rust** — [Install Rust](https://www.rust-lang.org/learn/get-started)
2. **macOS** — Built for macOS
3. **Vosk library** (required for voice) — Install the native speech recognition library:
   ```bash
   curl -LO https://github.com/alphacep/vosk-api/releases/download/v0.3.42/vosk-osx-0.3.42.zip
   unzip vosk-osx-0.3.42.zip
   sudo cp vosk-osx-0.3.42/libvosk.dylib /usr/local/lib/
   ```
   On first use, the English model (~40 MB) downloads automatically.
4. **OpenAI API key** (optional) — For AI intent parsing. Set `OPENAI_API_KEY` in your environment. Without it, a simple rule-based parser is used.

## Run

```bash
cd agent-app
npm install
npm run tauri dev
```

## Use

1. A small blue icon + input field appears
2. **Click** the icon to start voice — speak, then click again to stop and run your command
3. **Double-click** the icon to show the type panel for typed commands
4. Examples: `open wikipedia`, `search scholarships`, `open Chrome`, `what time is it`

## Commands

| Say | Does |
|-----|------|
| open wikipedia | Opens in browser |
| open Chrome | Opens the app |
| search for X | Google search |
| what time is it | Speaks time |
| what's the date | Speaks date |

## Build

```bash
npm run tauri build
```

Output in `src-tauri/target/release/bundle/`.
