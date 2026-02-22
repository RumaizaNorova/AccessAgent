# AccessPilot Agent

A voice-controlled desktop agent for accessibility. Small floating icon — click to talk, speak your command, click again. Uses OpenAI Whisper + GPT-4o.

---

## One-Time Setup

Do this once. After that, the agent works whenever you run it.

### 1. Prerequisites

- **Rust** — [Install Rust](https://www.rust-lang.org/learn/get-started)
- **macOS**
- **Chrome** — For in-page actions (search on sites, click, find)

### 2. OpenAI API Key

1. Get a key at https://platform.openai.com/api-keys  
2. Open `agent-app/.env`  
3. Add: `OPENAI_API_KEY=sk-proj-your-key-here`  
4. Save the file

### 3. Install the Agent App

```bash
cd agent-app
npm install
npm run tauri dev
```

A small blue mic icon appears. Leave it running.

### 4. Install the Chrome Extension (for in-page actions)

Required for: search on Amazon/sites, click, find, scroll.

1. Build the extension (from project root): `npm run build`
2. Open Chrome → `chrome://extensions`
3. Turn on **Developer mode** → **Load unpacked**
4. Select the `extension/dist` folder (not `extension` — Chrome needs the built .js files)
5. The AccessPilot extension appears. Leave it enabled.

Done. The extension connects to the agent automatically.

---

## Use

1. **Option+Space** — Global hotkey to summon agent and start listening (from any app)
2. **Click** the blue icon → speak
3. **Click again** → agent runs your command
4. Use Chrome for web (the extension runs there)

**Examples**

| Say | Does |
|-----|------|
| Open Amazon and search for candles | Opens Amazon, then searches on it |
| Open wikipedia | Opens Wikipedia |
| Open Slack / Open Mail / Open Notes | Opens native app (Slack, Mail, Notes) |
| Search for shoes | Searches on the current page |
| Click the buy button | Clicks it |
| What time is it | Speaks the time |
| Press Command S / Save | Simulates Cmd+S in the frontmost app |
| Scroll down | Scrolls the page |

**Press keys:** Commands like "press Command S", "save", "press Enter" simulate keystrokes. Requires **Accessibility** permission. Use `npm run tauri:dev:ax` instead of `npm run tauri dev` (add AccessPilot-debug.app to Accessibility first; run `./scripts/make-debug-app.sh` once after first build).

---

## Build for Distribution

```bash
cd agent-app
npm run tauri build
```

Output: `src-tauri/target/release/bundle/`

---

## Development

HMR is disabled. Restart the app (Ctrl+C, then `npm run tauri dev`) after code changes.
