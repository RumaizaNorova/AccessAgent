# AccessPilot Demo Script

## Setup

1. Build and load the extension (see [README](README.md)).
2. Pick 3 sites for the demo:
   - **Content:** e.g. Wikipedia or a news site
   - **E‑commerce:** Product/cart flow (no purchase)
   - **Portal:** e.g. University of Alberta site

---

## Demo Flow (~5 min)

### 1. Open the palette

- Navigate to the first site
- Press **Ctrl+Shift+Space** (Mac: Cmd+Shift+Space)
- Palette appears centered with suggestions

### 2. “find requirements”

- Type: `find requirements`
- Press Enter
- **Expected:** Page scrolls to the first match and briefly highlights it
- **Log:** “Step 1/1 — Found and highlighted”

### 3. “search for X”

- Type: `search for scholarships` (or a site-appropriate query)
- Press Enter
- **Expected:** Search box is found, query is typed, form is submitted
- **Log:** “Step 1/N — Typing query…”, then “Searched for …”

### 4. “click continue / next”

- Type: `click continue` or `click next`
- Press Enter
- **Expected:** Best matching button/link is clicked
- **Log:** “Step 1/N — Clicked …”

### 5. Panic key

- Start any command
- Press **Esc** or click **Stop**
- **Expected:** Palette closes, any overlay or automation stops

### 6. Access mode (optional)

- Type: `one-hand mode on`
- **Expected:** Bottom rail appears; buttons/links have larger min targets
- Type: `one-hand mode off` or press Esc
- Overlay disappears

---

## Troubleshooting

| Issue | Action |
|------|--------|
| Palette doesn’t open | Reload the extension, refresh the page |
| “No search box found” | Use a page that has a visible search input |
| “No matching element” | Try a clearer label (e.g. “next” instead of “proceed”) |
| Highlight looks odd | Some sites use layout that makes the highlight clip |

---

## Success Metrics to Highlight

- Fewer clicks and keystrokes for common tasks
- Clear step-by-step log
- Immediate stop via Esc/Stop
