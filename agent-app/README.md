# AccessPilot Agent

A **typing-first desktop agent** — small icon in the corner. Type commands: open apps, search the web, get the time. Works offline, no voice needed.

## Prerequisites

1. **Rust** — [Install Rust](https://www.rust-lang.org/learn/get-started)
2. **macOS** — Built for macOS

## Run

```bash
cd agent-app
npm install
npm run tauri dev
```

## Use

1. A small blue icon + input field appears
2. **Type** a command and press Enter or click Go
3. Examples: `open wikipedia`, `search scholarships`, `open Chrome`, `what time is it`

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
