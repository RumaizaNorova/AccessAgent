# AccessPilot Architecture

## Overview

AccessPilot follows a **plan–act–verify** loop. The content script runs in the page context to read the DOM and perform actions; the background service worker handles shortcuts and storage.

```
┌─────────────────┐     ┌──────────────────┐     ┌─────────────────┐
│  Command        │     │  Content Script   │     │  Background      │
│  Palette (UI)   │────▶│  (agent loop)     │◀────│  (shortcuts,     │
│  Shadow DOM     │     │  detect/execute   │     │   storage)       │
└─────────────────┘     └──────────────────┘     └──────────────────┘
```

## Components

### Content Script (`src/content/`)

| File | Role |
|------|------|
| `index.ts` | Entry; injects palette overlay, handles messages |
| `agent.ts` | Plan-act-verify loop; routes commands to skills |
| `parser.ts` | Parses raw input into `{ type, payload }` |
| `detect.ts` | Collects and ranks interactive candidates |
| `execute.ts` | Safe click, focus, type, scroll, back |
| `verify.ts` | Verifies success after each action |

### Detection + Ranking

1. **Collect** interactive elements: `button`, `a`, `input`, `[role=button]`, etc.
2. **Labels** from: `aria-label`, `aria-labelledby`, `<label for>`, `placeholder`, `title`, `innerText`
3. **Filter** visible + enabled + non-zero bounding box
4. **Rank** by intent:
   - Search: `type=search`, `role=searchbox`, label match
   - Primary action: submit, continue, next, add to cart, etc.
   - Click: text similarity + visibility + size + position

### Action Execution

- `click`, `focus`, `type`, `pressEnter`, `scrollTo`, `scrollBy`, `back`
- Elements scrolled into view before interaction
- Events dispatched for frameworks (e.g. `input`, `change`)

### Verifier

After each action:

- URL change
- DOM mutation in target region
- Appearance of expected text
- `activeElement` matches expected

On failure: fallback to next candidate or stop with an explanation.

## Permissions

| Permission | Purpose |
|------------|---------|
| `activeTab` | Access current tab for injection |
| `storage` | Local settings (confirm destructive, access mode) |
| `scripting` | (Reserved for future use) |
| `<all_urls>` | Run content script on any site |

All processing is local; no page content is sent over the network.

## Data Flow

1. User presses Ctrl+Shift+Space → background handles command → sends `OPEN_PALETTE` to content
2. Content shows palette overlay (Shadow DOM)
3. User types command → parser → agent
4. Agent: parse → detect → execute → verify → log steps
5. Log displayed in palette; user can Stop at any time
