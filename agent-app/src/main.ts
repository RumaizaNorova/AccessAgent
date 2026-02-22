import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { listen } from "@tauri-apps/api/event";
import { requestPermission } from "tauri-plugin-stt-api";
import AudioRecorder from "audio-recorder-polyfill";

const icon = document.getElementById("icon")!;

function speak(text: string) {
  if (!text) return;
  if (window.speechSynthesis) {
    window.speechSynthesis.cancel();
    const u = new SpeechSynthesisUtterance(text);
    window.speechSynthesis.speak(u);
  }
}

function scheduleExtensionTimeout() {
  if (extensionResultTimeoutId) clearTimeout(extensionResultTimeoutId);
  extensionResultTimeoutId = setTimeout(() => {
    extensionResultTimeoutId = null;
    speak("Extension didn't respond. Click the extension icon to wake it, refresh the tab, then try again.");
  }, EXTENSION_TIMEOUT_MS);
}

/** Shorten errors for TTS (max ~80 chars). Don't mask — surface the real issue. */
function shortenForTts(msg: string): string {
  const s = msg.trim();
  if (s.length <= 80) return s;
  return s.slice(0, 77) + "...";
}

async function parseCommand(text: string): Promise<{ type: string; payload: string }> {
  const intent = await invoke<{ action: string; payload: string }>("parse_intent", {
    command: text,
    apiKeyOverride: null,
  });
  return { type: intent.action, payload: intent.payload || "" };
}

function resolveUrl(payload: string): string {
  const p = payload.trim().toLowerCase();
  if (/^https?:\/\//i.test(p)) return payload.trim();
  if (/^[a-z0-9-]+\.[a-z]{2,}$/i.test(p)) return `https://${p}`;
  const known: Record<string, string> = {
    amazon: "https://www.amazon.com",
    ebay: "https://www.ebay.com",
    walmart: "https://www.walmart.com",
    wikipedia: "https://www.wikipedia.org",
    google: "https://www.google.com",
    youtube: "https://www.youtube.com",
  };
  if (known[p]) return known[p];
  return `https://www.${p}.com`;
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

  // open_and_search: open site, wait for load, then search on it (one phrase does both)
  if (parsed.type === "open_and_search") {
    const parts = parsed.payload.split("|").map((s) => s.trim());
    const site = parts[0] || "";
    const query = parts.slice(1).join("|").trim();
    if (!site || !query) {
      speak("Try: open Amazon and search for candles.");
      return;
    }
    const url = resolveUrl(site);
    await invoke("open_url", { url });
    speak("Opening. Searching in a moment.");
    await new Promise((r) => setTimeout(r, 3500));
    const extCmd = `search for ${query}`;
    const sent = await invoke<boolean>("send_to_extension", { command: extCmd });
    if (!sent) speak("Install the AccessPilot extension for in-page search.");
    else scheduleExtensionTimeout();
    return;
  }

  // In-page actions: click, find, page_search, scroll, access_mode, close_popup, go_to — send to extension
  const extensionActions = ["click", "find", "page_search", "scroll", "access_mode", "close_popup", "go_to"];
  if (extensionActions.includes(parsed.type)) {
    const extCmd = formatExtensionCommand(parsed.type, parsed.payload);
    const sent = await invoke<boolean>("send_to_extension", { command: extCmd });
    if (!sent) {
      speak("Install the AccessPilot Chrome extension and refresh your tab, then try again.");
    } else {
      if (parsed.type === "go_to") pendingGoToIntent = parsed.payload;
      scheduleExtensionTimeout();
    }
    return;
  }

  speak("I didn't understand. Try: open wikipedia, click the buy button, or search for something.");
}

function formatExtensionCommand(action: string, payload: string): string {
  const p = payload.trim();
  switch (action) {
    case "click":
      return `click ${p}`;
    case "find":
      return `find ${p}`;
    case "page_search":
      return `search for ${p}`;
    case "scroll":
      return `scroll ${p.toLowerCase()}`;
    case "access_mode":
      return p.toLowerCase() === "on" ? "one-hand mode on" : "one-hand mode off";
    case "close_popup":
      return "close popup";
    case "go_to":
      return `get_headings:${p}`;
    default:
      return `${action} ${p}`;
  }
}

let isListening = false;
let isStarting = false;
let lastProcessed = "";

// Whisper path: MediaRecorder for accurate transcription
let mediaRecorder: MediaRecorder | null = null;
let audioChunks: Blob[] = [];
let whisperMimeType = "audio/webm";

/** Use polyfill when native would use mp4 (Safari/WebKit) — mp4 has encoding issues in WebKit */
function getMediaRecorderConstructor(): typeof MediaRecorder {
  const prefersMp4 = MediaRecorder.isTypeSupported("audio/mp4") && !MediaRecorder.isTypeSupported("audio/webm");
  return prefersMp4 ? (AudioRecorder as unknown as typeof MediaRecorder) : MediaRecorder;
}

const CHUNK_INTERVAL_MS = 100; // Request chunks during recording so we capture short utterances and don't rely on final flush

async function startListeningWhisper() {
  const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
  const Recorder = getMediaRecorderConstructor();
  const mimeType = Recorder === AudioRecorder
    ? "audio/wav"
    : MediaRecorder.isTypeSupported("audio/webm;codecs=opus")
      ? "audio/webm;codecs=opus"
      : "audio/webm";
  const recorder = new Recorder(stream, Recorder === MediaRecorder ? { mimeType } : undefined);
  whisperMimeType = Recorder === AudioRecorder ? "audio/wav" : mimeType;
  audioChunks = [];
  const onData = (e: BlobEvent) => {
    if (e.data.size > 0) audioChunks.push(e.data);
  };
  recorder.addEventListener("dataavailable", onData);
  recorder.start(CHUNK_INTERVAL_MS); // Timeslice = get chunks during recording, not just at stop
  mediaRecorder = recorder as MediaRecorder;
}

type StopResult = { base64: string; mimeType: string } | { error: string };

function bytesToBase64(bytes: Uint8Array): string {
  let binary = "";
  const chunk = 8192;
  for (let i = 0; i < bytes.length; i += chunk) {
    binary += String.fromCharCode(...bytes.subarray(i, i + chunk));
  }
  return btoa(binary);
}

function stopListeningWhisper(): Promise<StopResult | null> {
  return new Promise((resolve) => {
    const recorder = mediaRecorder;
    mediaRecorder = null;
    if (!recorder || recorder.state === "inactive") {
      resolve(null);
      return;
    }
    const handleStop = async () => {
      recorder.stream?.getTracks?.()?.forEach((t) => t.stop());
      await new Promise((r) => setTimeout(r, 150));
      const chunks = audioChunks.filter((c) => c.size > 0);
      if (chunks.length === 0) {
        resolve({ error: "Microphone didn't capture any audio." });
        return;
      }
      const blob = new Blob(chunks, { type: whisperMimeType });
      try {
        const bytes = new Uint8Array(await blob.arrayBuffer());
        const base64 = bytesToBase64(bytes);
        resolve({ base64, mimeType: whisperMimeType });
      } catch (e) {
        const msg = e instanceof Error ? e.message : String(e);
        console.error("[whisper] encode error:", msg);
        resolve({ error: `Could not encode audio: ${msg}` });
      }
    };
    recorder.addEventListener("stop", handleStop, { once: true });
    recorder.requestData?.();
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
  isStarting = true;
  try {
    const whisperAvail = await invoke<boolean>("whisper_available");
    if (!whisperAvail) {
      speak("Set OPENAI_API_KEY in agent-app .env file.");
      return;
    }
    const perm = await requestPermission();
    if (perm.microphone !== "granted") {
      speak("Allow microphone in System Settings.");
      return;
    }
    speak("Listening.");
    await startListeningWhisper();
    isListening = true;
    icon.classList.add("listening");
  } catch (e) {
    const msg = e instanceof Error ? e.message : String(e);
    speak(shortenForTts(msg));
  } finally {
    isStarting = false;
  }
}

getCurrentWindow()
  .setFocus()
  .catch(() => {});

// Timeout when extension doesn't respond (e.g. not connected or wrong tab)
let extensionResultTimeoutId: ReturnType<typeof setTimeout> | null = null;
const EXTENSION_TIMEOUT_MS = 5000;
// Two-phase go_to: we sent get_headings, waiting for headings to resolve via GPT
let pendingGoToIntent: string | null = null;

// Speak results from extension (click, find, search, etc.)
listen<string>("extension-result", async (e) => {
  if (extensionResultTimeoutId) {
    clearTimeout(extensionResultTimeoutId);
    extensionResultTimeoutId = null;
  }
  const payload = e.payload || "";
  if (pendingGoToIntent && payload.startsWith("HEADINGS:")) {
    const json = payload.slice(9).trim();
    const intent = pendingGoToIntent;
    pendingGoToIntent = null;
    try {
      const headings: string[] = JSON.parse(json);
      const section = await invoke<string>("resolve_section", { intent, headings });
      const sent = await invoke<boolean>("send_to_extension", {
        command: `scroll_to_section:${section}`,
      });
      if (!sent) speak("Couldn't scroll to that section.");
      else scheduleExtensionTimeout();
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      speak(shortenForTts(msg));
    }
    return;
  }
  if (payload) speak(payload);
}).catch(() => {});

icon.addEventListener("pointerdown", async (e) => {
  e.preventDefault();
  e.stopPropagation();
  if (isStarting) return;
  if (isListening) {
    isListening = false;
    icon.classList.remove("listening");
    speak("Transcribing.");
    try {
      const result = await stopListeningWhisper();
      if (!result) return;
      if ("error" in result) {
        speak(result.error);
        return;
      }
      const txt = await invoke<string>("transcribe_audio", {
        base64Audio: result.base64,
        mimeType: result.mimeType,
      });
      if (txt && txt !== lastProcessed) {
        lastProcessed = txt;
        try {
          await runCommand(txt);
        } catch (err) {
          const msg = err instanceof Error ? err.message : String(err);
          console.error("Command error:", msg);
          speak(shortenForTts(msg));
        }
        setTimeout(() => { lastProcessed = ""; }, 2000);
      } else if (!txt || !txt.trim()) {
        speak("No speech detected. Try speaking again.");
      }
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      console.error("Transcription error:", msg);
      speak(shortenForTts(msg));
    }
  } else {
    await startListening();
  }
});
