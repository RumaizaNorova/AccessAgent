# AccessPilot

**Voice agent for your desktop.** Small icon in the corner — click, speak, and it opens apps, searches the web, tells the time. For people who cannot move freely.

## Voice Agent (native app — preferred)

```bash
cd agent-app
npm install
npm run tauri dev
```

Requires [Rust](https://rustup.rs). A small mic icon appears; click to talk.

See [agent-app/README.md](agent-app/README.md).

## Web fallback

```bash
cd desktop
node server.js
```

Opens in browser. Bookmark the URL.

## Chrome Extension (in-page control)

For find/click/type on the current webpage:

```bash
npm install && npm run build
```

Load `extension/dist` in Chrome → [chrome://extensions](chrome://extensions).

## Docs

- [Desktop Plan](docs/DESKTOP_AGENT_PLAN.md)
- [Architecture](docs/ARCHITECTURE.md)
- [Privacy](docs/PRIVACY.md)
