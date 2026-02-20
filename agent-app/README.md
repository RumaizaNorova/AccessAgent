# AccessPilot Agent

A **native voice agent** — small icon in the corner of your screen. Click it, speak, and it opens apps, searches the web, tells the time.

## Prerequisites

1. **Rust** — [Install Rust](https://www.rust-lang.org/learn/get-started)
2. **macOS** — Built for macOS (other platforms: add support)

## Run

```bash
cd agent-app
npm install
npm run tauri dev
```

## Use

1. A small blue mic icon appears (you can drag it)
2. **Click** → starts listening (turns red, pulses)
3. **Say** — "open wikipedia", "search for scholarships", "open Chrome", "what time is it"
4. **Click again** to stop, or it stops when you pause

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
