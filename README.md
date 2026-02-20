# AccessPilot

**Accessibility-first agent.** For people who cannot move freely — a talking desktop agent that opens sites, searches the web, and speaks back.

## Desktop Agent (start here)

```bash
cd desktop
node server.js
```

Opens in your browser. Say or type: *"open wikipedia"*, *"search for scholarships"*, *"what time is it"*. The agent speaks back. Bookmark the URL for quick access.

See [desktop/README.md](desktop/README.md).

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
