# Desktop Talking Agent — Plan

## Vision

A **desktop agent** for people who cannot move freely (mobility, dexterity, one-hand users). The agent:
- **Appears on the desktop** — summonable from anywhere via hotkey
- **Talks** — listens to voice, speaks responses (hands-free possible)
- **Does whatever you ask** — open sites, search web, open apps, tell time

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│  AccessPilot Desktop (Electron)                              │
│  ┌─────────────────┐  ┌─────────────────┐  ┌─────────────┐  │
│  │  Voice Input    │  │  Agent (parse +  │  │  Voice Out   │  │
│  │  Speech-to-Text│→ │  execute)       │→ │  TTS         │  │
│  └─────────────────┘  └─────────────────┘  └─────────────┘  │
│           ↑                     │                           │
│  ┌────────┴────────┐   ┌───────┴───────┐                   │
│  │  Mic button /   │   │  OPEN: open    │                   │
│  │  Global hotkey  │   │  SEARCH: open  │                   │
│  └─────────────────┘   │  browser      │                   │
│                        └───────────────┘                   │
└─────────────────────────────────────────────────────────────┘
```

## MVP Actions (desktop-only)

| Command | Example | Action |
|---------|---------|--------|
| open | "open wikipedia" | Open URL in default browser |
| search | "search for scholarships" | Open Google search in browser |
| time | "what time is it" | Speak current time |
| date | "what's the date" | Speak current date |
| stop | "stop" | Stop speaking, close window |

## Voice

- **Input**: Web Speech API `SpeechRecognition` (Chrome/Chromium)
- **Output**: `speechSynthesis` for agent responses
- **Fallback**: Typed input always available (accessibility)

## UX

- Floating compact window (or always-on-top panel)
- Large mic button (44px+ target)
- Global hotkey: `Ctrl+Shift+Space` or `Option+Space`
- Shows transcript + response for verification
- Esc to close / stop

## Relation to Chrome Extension

| Desktop Agent | Chrome Extension |
|---------------|------------------|
| Opens URLs, searches | Runs on current page |
| Speaks responses | Runs find/click/type on DOM |
| Voice-first | Typed commands |
| System-wide hotkey | Browser-only |

**Use both**: Desktop agent for "open X" / "search Y"; extension for "find Z" / "click next" when on a page.
