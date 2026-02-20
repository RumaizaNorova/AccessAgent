# AccessPilot

**Accessibility-first web agent.** Control websites with simple commands—no precise clicking, minimal navigation.

## Install (Developer Mode)

1. **Build the extension**
   ```bash
   npm install
   npm run build
   ```

2. **Load in Chrome**
   - Open `chrome://extensions`
   - Enable **Developer mode** (top right)
   - Click **Load unpacked**
   - Select the `extension/dist` folder

3. **Use it**
   - Press **Ctrl+Shift+Space** (Mac: Cmd+Shift+Space) on any page
   - Type a command and press Enter

## Commands

| Command | Example |
|--------|---------|
| **open** | `open ualberta.ca` • `open github` |
| **search** | `search for scholarships` |
| **find** | `find requirements` |
| **click** | `click continue` • `click next` |
| **access mode** | `one-hand mode on` • `target boost off` |

## Demo Sites

Works best on:

- **Content:** Wikipedia, news sites
- **E‑commerce:** Product pages, add-to-cart (no purchase)
- **Portals:** University sites, public services

## Safety

- **Stop:** Press **Esc** or click **Stop** to cancel automation
- **Confirm:** Destructive actions (checkout, submit) require confirmation (when enabled)
- **Local only:** No page content sent to external servers

## Tech

- Chrome Extension Manifest V3
- TypeScript
- Vite + vite-plugin-web-extension
