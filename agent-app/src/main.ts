import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
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
let isStarting = false;
let fullText = "";
let lastInterim = "";
let lastProcessed = "";
let unlistenResult: (() => void) | null = null;
let unlistenState: (() => void) | null = null;
let unlistenError: (() => void) | null = null;
let unlistenDownload: (() => void) | null = null;
let downloadProgressSpoken = false;

let unlistenBackup: (() => void) | null = null;

// Whisper path: MediaRecorder for accurate transcription when API key is set
let mediaRecorder: MediaRecorder | null = null;
let audioChunks: Blob[] = [];
let useWhisperMode = false;
let whisperMimeType = "audio/webm";

function cleanupListeners() {
  unlistenResult?.();
  unlistenState?.();
  unlistenError?.();
  unlistenDownload?.();
  unlistenBackup?.();
  unlistenResult = null;
  unlistenState = null;
  unlistenError = null;
  unlistenDownload = null;
  unlistenBackup = null;
}

function runCapturedCommand() {
  const txt = (fullText || lastInterim).trim();
  if (txt && txt !== lastProcessed) {
    lastProcessed = txt;
    runCommand(txt);
    setTimeout(() => { lastProcessed = ""; }, 2000);
  } else if (!txt) {
    speak("No speech detected. Speak clearly, pause, then click to stop.");
  }
}

async function startListeningWhisper() {
  const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
  // Safari/WebKit (Tauri on macOS) supports audio/mp4, not webm
  const mimeType = MediaRecorder.isTypeSupported("audio/mp4")
    ? "audio/mp4"
    : MediaRecorder.isTypeSupported("audio/webm;codecs=opus")
      ? "audio/webm;codecs=opus"
      : "audio/webm";
  const recorder = new MediaRecorder(stream, { mimeType });
  whisperMimeType = recorder.mimeType.startsWith("audio/mp4") ? "audio/mp4" : "audio/webm";
  audioChunks = [];
  recorder.ondataavailable = (e) => {
    if (e.data.size > 0) audioChunks.push(e.data);
  };
  recorder.start(100);
  mediaRecorder = recorder;
  useWhisperMode = true;
}

function stopListeningWhisper(): Promise<string> {
  return new Promise((resolve) => {
    const recorder = mediaRecorder;
    mediaRecorder = null;
    if (!recorder || recorder.state === "inactive") {
      resolve("");
      return;
    }
    recorder.onstop = () => {
      recorder.stream.getTracks().forEach((t) => t.stop());
      if (audioChunks.length === 0) {
        resolve("");
        return;
      }
      const blob = new Blob(audioChunks, { type: whisperMimeType });
      const reader = new FileReader();
      reader.onloadend = () => {
        const base64 = (reader.result as string).split(",")[1] ?? "";
        resolve(base64);
      };
      reader.readAsDataURL(blob);
    };
    recorder.stop();
  });
}

async function startListening() {
  if (isStarting) {
    speak("Please wait.");
    return;
  }
  if (isListening) {
    speak("Already recording. Click again to stop.");
    return;
  }
  try {
    isStarting = true;
    speak("Starting.");
    const perm = await requestPermission();
    if (perm.microphone !== "granted") {
      isStarting = false;
      speak("Allow microphone in System Settings.");
      return;
    }

    const whisperAvail = await invoke<boolean>("whisper_available");
    if (whisperAvail) {
      await startListeningWhisper();
      isListening = true;
      icon.classList.add("listening");
      isStarting = false;
      return;
    }

    const available = await isAvailable();
    if (!available.available) {
      isStarting = false;
      speak(available.reason || "Voice not ready.");
      return;
    }

    fullText = "";
    lastInterim = "";
    downloadProgressSpoken = false;
    isListening = true;
    icon.classList.add("listening");

    // Listen for model download (first run)
    unlistenDownload = await listen<{ status: string; progress: number }>("stt://download-progress", (event) => {
      if (!downloadProgressSpoken) {
        speak("Downloading voice model, one moment.");
        downloadProgressSpoken = true;
      }
    });

    unlistenResult = await onResult((result) => {
      const t = result.transcript?.trim() || "";
      if (!t) return;
      if (result.isFinal) {
        fullText = (fullText ? fullText + " " : "") + t;
        lastInterim = "";
      } else {
        lastInterim = t;
      }
    });
    unlistenBackup = await listen("stt://result", (e: { payload: unknown }) => {
      const r = e.payload as { transcript?: string; isFinal?: boolean; is_final?: boolean };
      const t = r?.transcript?.trim() || "";
      if (!t) return;
      const fin = r.isFinal ?? r.is_final ?? false;
      if (fin) { fullText = (fullText ? fullText + " " : "") + t; lastInterim = ""; }
      else { lastInterim = t; }
    });

    unlistenState = await onStateChange((event) => {
      if (event.state === "idle" && isListening) {
        isListening = false;
        icon.classList.remove("listening");
        cleanupListeners();
        setTimeout(runCapturedCommand, 800);
      }
    });

    unlistenError = await onError((err) => {
      isListening = false;
      icon.classList.remove("listening");
      cleanupListeners();
      if (err.code === "PERMISSION_DENIED" || err.code === "SPEECH_PERMISSION_DENIED") {
        speak("Allow microphone.");
      } else if (err.code === "CANCELLED") {
        // User stopped
      } else if (err.code === "ALREADY_LISTENING") {
        speak("Try again.");
      } else {
        speak(err.message || "Voice error.");
      }
    });

    await sttStart({
      language: "en-US",
      interimResults: true,
      continuous: true,
    });
  } catch (e) {
    isListening = false;
    icon.classList.remove("listening");
    cleanupListeners();
    const msg = e instanceof Error ? e.message : String(e);
    if (msg.includes("Already listening") || msg.includes("ALREADY")) {
      try {
        await sttStop();
      } catch {
        // Ignore
      }
      speak("Try again.");
    } else if (msg.includes("library") && msg.includes("vosk")) {
      speak("Install Vosk: see README.");
    } else {
      speak("Voice error: " + msg);
    }
  } finally {
    isStarting = false;
  }
}

// Focus window on load so first click hits the icon (not consumed by OS for activation)
getCurrentWindow()
  .setFocus()
  .catch(() => {});

// Warm-up: check availability on load
isAvailable().catch(() => {});

icon.addEventListener("pointerdown", async (e) => {
  e.preventDefault();
  e.stopPropagation();
  if (isStarting) return;
  if (isListening) {
    isListening = false;
    icon.classList.remove("listening");
    if (useWhisperMode) {
      useWhisperMode = false;
      speak("Transcribing.");
      try {
        const base64 = await stopListeningWhisper();
        if (base64) {
          const txt = await invoke<string>("transcribe_audio", {
            base64_audio: base64,
            mime_type: whisperMimeType,
          });
          if (txt && txt !== lastProcessed) {
            lastProcessed = txt;
            runCommand(txt);
            setTimeout(() => { lastProcessed = ""; }, 2000);
          } else if (!txt) {
            speak("No speech detected. Speak clearly, then click to stop.");
          }
        } else {
          speak("No speech detected. Speak clearly, then click to stop.");
        }
      } catch (err: unknown) {
        const msg = err instanceof Error ? err.message : String(err);
        console.error("Transcription error:", msg);
        const short = msg.length > 60 ? msg.slice(0, 60) + "…" : msg;
        speak(short.includes("API key") ? "Check your API key in .env" : short || "Transcription failed.");
      }
    } else {
      cleanupListeners();
      try {
        await sttStop();
      } catch {
        // Ignore
      }
      setTimeout(runCapturedCommand, 800);
    }
  } else {
    await startListening();
  }
});
