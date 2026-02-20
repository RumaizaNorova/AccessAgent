# AccessPilot Desktop

**Talking agent for your desktop.** For people who cannot move freely — speak or type, and the agent opens sites, searches the web, and speaks back.

## Run (Node server — recommended)

```bash
cd desktop
node server.js
```

Opens in your default browser. Bookmark the URL for quick access.

## Run (Electron — if it works on your system)

```bash
cd desktop
npm install
npm start
```

**Hotkey:** Ctrl+Shift+Space — summons the agent from anywhere.

## Commands (voice or type)

| Say or type | What it does |
|-------------|--------------|
| "open wikipedia" | Opens wikipedia.org |
| "open github" | Opens github.com |
| "search for scholarships" | Opens Google search |
| "what time is it" | Speaks the time |
| "what's the date" | Speaks the date |
| "stop" | Stops speaking, closes |

## Voice

- **Mic button**: Hold to speak your command
- **TTS**: Agent speaks responses aloud (accessibility-first)
- **Typed fallback**: Always works if voice isn't available
