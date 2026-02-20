# Implementation Plan + Milestones

## Permissions (Justified)

| Permission | Purpose |
|------------|---------|
| `activeTab` | Access current tab when user invokes the extension |
| `storage` | Store settings locally (confirm destructive, access mode) |
| `scripting` | Reserved for future script injection |
| `host_permissions: <all_urls>` | Run content script on any webpage the user visits |

**No network calls** for page content. All processing is local.

---

## Milestones

### M1: Scaffold (Done)
- Repo structure
- Manifest V3
- Vite + TypeScript build
- Icons, popup shell

### M2: Core Engine (Done)
- Detection: collect + rank candidates
- Execution: click, focus, type, scroll
- Verifier: URL/DOM/text checks
- Command parser

### M3: Agent Loop (Done)
- Plan-act-verify loop
- OPEN, SEARCH, FIND, CLICK, ACCESS_MODE
- Step logging
- Panic / Stop

### M4: UI (Done)
- Command palette (Shadow DOM overlay)
- Hotkey: Ctrl+Shift+Space
- Suggestions, execution log
- Popup settings

### M5: Safety & Docs (Done)
- Local storage settings
- Panic key (Esc / Stop button)
- README, ARCHITECTURE, PRIVACY, DEMO_SCRIPT

---

## Acceptance Checklist

- [ ] Build succeeds (`npm run build`)
- [ ] Load in Chrome without errors
- [ ] Hotkey opens palette on a regular webpage
- [ ] `open ualberta.ca` navigates
- [ ] `search for X` works on a page with a search box
- [ ] `find requirements` scrolls and highlights
- [ ] `click continue` clicks the best match
- [ ] Esc / Stop closes palette and stops automation
- [ ] Settings persist in popup
