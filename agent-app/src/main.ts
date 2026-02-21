import { invoke } from "@tauri-apps/api/core";
import {
  startListening as sttStart,
  stopListening as sttStop,
  onResult,
  onStateChange,
  onError,
  requestPermission,
  isAvailable,
} from "tauri-plugin-stt-api";

const icon = document.getElementById("icon")!;

function speak(text: string) {
  if (window.speechSynthesis) {
    window.speechSynthesis.cancel();
    const u = new SpeechSynthesisUtterance(text);
    window.speechSynthesis.speak(u);
  }
}

/** Fallback when OpenAI is unavailable */
function parseCommandFallback(raw: string): { type: string; payload: string } {
  const t = raw.trim().toLowerCase().replace(/\s+/g, " ");
  if (!t) return { type: "unknown", payload: "" };

  if (/open\s+(.+)/.test(t)) return { type: "open", payload: t.replace(/.*?open\s+/, "").trim() };
  if (/search(?:\s+for)?\s+(.+)/.test(t)) return { type: "search", payload: t.replace(/.*?search(?:\s+for)?\s+/, "").trim() };
  if (/\b(time|what'?s?\s+the\s+time)\b/.test(t)) return { type: "time", payload: "" };
  if (/\b(date|what'?s?\s+the\s+date)\b/.test(t)) return { type: "date", payload: "" };
  if (/^(stop|cancel)$/.test(t)) return { type: "stop", payload: "" };

  return { type: "search", payload: t };
}

async function parseCommand(text: string): Promise<{ type: string; payload: string }> {
  try {
    const intent = await invoke<{ action: string; payload: string }>("parse_intent", {
      command: text,
      apiKeyOverride: null,
    });
    return { type: intent.action, payload: intent.payload || "" };
  } catch {
    return parseCommandFallback(text);
  }
}

function resolveUrl(payload: string): string {
  const p = payload.trim();
  if (/^https?:\/\//i.test(p)) return p;
  if (/^[a-z0-9-]+\.[a-z]{2,}$/i.test(p)) return `https://${p}`;
  return `https://duckduckgo.com/?q=${encodeURIComponent(p)}`;
}

async function runCommand(text: string) {
  const parsed = await parseCommand(text);

  if (parsed.type === "time") {
    const now = new Date();
    const time = now.toLocaleTimeString([], { hour: "numeric", minute: "2-digit" });
    speak(`The time is ${time}`);
    return;
  }
  if (parsed.type === "date") {
    const now = new Date();
    const date = now.toLocaleDateString([], { weekday: "long", month: "long", day: "numeric" });
    speak(`Today is ${date}`);
    return;
  }
  if (parsed.type === "stop") {
    speak("Stopped");
    return;
  }
  if (parsed.type === "open") {
    const p = parsed.payload;
    const knownApps = ["chrome", "safari", "firefox", "spotify", "slack", "mail", "notes"];
    if (!p.includes(".") && (knownApps.includes(p.toLowerCase()) || /^[A-Z]/.test(p))) {
      try {
        await invoke("open_app", { name: p });
        speak(`Opening ${p}`);
      } catch {
        const url = resolveUrl(p);
        await invoke("open_url", { url });
        speak(`Opening ${p}`);
      }
    } else {
      const url = resolveUrl(p);
      await invoke("open_url", { url });
      speak(`Opening ${p}`);
    }
    return;
  }
  if (parsed.type === "search") {
    const url = `https://duckduckgo.com/?q=${encodeURIComponent(parsed.payload)}`;
    await invoke("open_url", { url });
    speak(`Searching for ${parsed.payload}`);
    return;
  }

  speak("I didn't understand. Try: open wikipedia, or search for something.");
}

let isListening = false;
let fullText = "";
let lastProcessed = "";
let unlistenResult: (() => void) | null = null;
let unlistenState: (() => void) | null = null;
let unlistenError: (() => void) | null = null;

function cleanupListeners() {
  unlistenResult?.();
  unlistenState?.();
  unlistenError?.();
  unlistenResult = null;
  unlistenState = null;
  unlistenError = null;
}

async function startListening() {
  try {
    const available = await isAvailable();
    if (!available.available) {
      speak(available.reason || "Voice not supported. Install Vosk: see README.");
      return;
    }

    const perm = await requestPermission();
    if (perm.microphone !== "granted") {
      speak("Allow microphone in System Settings.");
      return;
    }

    fullText = "";
    isListening = true;
    icon.classList.add("listening");

    unlistenResult = await onResult((result) => {
      if (result.isFinal && result.transcript?.trim()) {
        fullText = (fullText ? fullText + " " : "") + result.transcript.trim();
      }
    });

    unlistenState = await onStateChange((event) => {
      if (event.state === "idle" && isListening) {
        isListening = false;
        icon.classList.remove("listening");
        cleanupListeners();
        const txt = fullText.trim();
        if (txt && txt !== lastProcessed) {
          lastProcessed = txt;
          runCommand(txt);
          setTimeout(() => { lastProcessed = ""; }, 2000);
        }
      }
    });

    unlistenError = await onError((err) => {
      isListening = false;
      icon.classList.remove("listening");
      cleanupListeners();
      if (err.code === "PERMISSION_DENIED" || err.code === "SPEECH_PERMISSION_DENIED") {
        speak("Allow microphone in System Settings.");
      } else if (err.code === "NO_SPEECH") {
        speak("Speak, then click again to stop.");
      } else if (err.code === "CANCELLED") {
        // User stopped, ignore
      } else {
        speak(err.message || "Voice error.");
      }
    });

    await sttStart({
      language: navigator.language?.startsWith("en") ? "en-US" : "en-US",
      interimResults: true,
      continuous: true,
    });
  } catch (e) {
    isListening = false;
    icon.classList.remove("listening");
    cleanupListeners();
    const msg = e instanceof Error ? e.message : String(e);
    if (msg.includes("library") && msg.includes("vosk")) {
      speak("Install Vosk first. Run: curl -LO https://github.com/alphacep/vosk-api/releases/download/v0.3.42/vosk-osx-0.3.42.zip && unzip vosk-osx-0.3.42.zip && sudo cp vosk-osx-0.3.42/libvosk.dylib /usr/local/lib/");
    } else {
      speak("Voice error: " + msg);
    }
  }
}

async function stopListening() {
  if (!isListening) return;
  try {
    await sttStop();
  } catch {
    // Ignore
  }
}

const typePanel = document.getElementById("type-panel")!;
const typeInput = document.getElementById("type-input") as HTMLInputElement;
const typeGo = document.getElementById("type-go")!;

icon.addEventListener("click", async (e) => {
  e.preventDefault();
  if (isListening) {
    await stopListening();
  } else {
    await startListening();
  }
});

icon.addEventListener("dblclick", async (e) => {
  e.preventDefault();
  await stopListening();
  const show = typePanel.style.display === "none";
  typePanel.style.display = show ? "flex" : "none";
  if (show) typeInput.focus();
});

typeGo.addEventListener("click", () => {
  const cmd = typeInput.value.trim();
  if (cmd) {
    runCommand(cmd);
    typeInput.value = "";
    typePanel.style.display = "none";
  }
});

typeInput.addEventListener("keydown", (e) => {
  if (e.key === "Enter") typeGo.click();
});
