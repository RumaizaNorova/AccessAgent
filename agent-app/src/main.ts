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

type Step = { action: string; payload: string; target_type?: string };
async function parseCommand(text: string): Promise<{ steps: Step[] }> {
  const intent = await invoke<{ steps: Array<{ action: string; payload: string; target_type?: string }> }>("parse_intent", {
    command: text,
    apiKeyOverride: null,
  });
  return { steps: intent.steps || [] };
}

function resolveUrl(payload: string): string {
  const raw = payload.trim();
  if (/^https?:\/\//i.test(raw)) return raw;
  if (/^[a-z0-9-]+\.[a-z]{2,}$/i.test(raw)) return `https://${raw}`;
  // Normalize: "wikipedia web app", "the youtube", "open amazon" → extract site name
  const p = raw
    .toLowerCase()
    .replace(/\b(the|web|site|app|page|website)\b/gi, "")
    .replace(/\s+/g, " ")
    .trim();
  const known: Record<string, string> = {
    amazon: "https://www.amazon.com",
    ebay: "https://www.ebay.com",
    walmart: "https://www.walmart.com",
    wikipedia: "https://www.wikipedia.org",
    google: "https://www.google.com",
    youtube: "https://www.youtube.com",
    netflix: "https://www.netflix.com",
    reddit: "https://www.reddit.com",
    facebook: "https://www.facebook.com",
    twitter: "https://twitter.com",
    instagram: "https://www.instagram.com",
    linkedin: "https://www.linkedin.com",
    gmail: "https://mail.google.com",
    github: "https://github.com",
  };
  const key = p.split(/\s+/)[0] || p;
  if (known[key]) return known[key];
  return `https://www.${key}.com`;
}

const PAGE_LOAD_DELAY_MS = 3500;
const extensionActions = ["click", "find", "find_and_read", "page_search", "scroll", "access_mode", "close_popup", "go_to"];

async function executeStep(step: Step): Promise<"opened_url" | "sent_to_ext" | "done"> {
  const { action: type, payload: p, target_type } = step;

  if (type === "time") {
    const now = new Date();
    speak(`The time is ${now.toLocaleTimeString([], { hour: "numeric", minute: "2-digit" })}`);
    return "done";
  }
  if (type === "date") {
    const now = new Date();
    speak(`Today is ${now.toLocaleDateString([], { weekday: "long", month: "long", day: "numeric" })}`);
    return "done";
  }
  if (type === "stop") {
    speak("Stopped");
    return "done";
  }
  if (type === "open") {
    const payload = p.trim();
    const looksLikeInPage = /\b(first|second|third|this|that)\s*(one|video|result|item)?\b/i.test(payload) || /\bvideo\b/i.test(payload);
    if (looksLikeInPage) {
      const sent = await invoke<boolean>("send_to_extension", { command: `click ${payload}` });
      if (!sent) speak("Install the AccessPilot Chrome extension and refresh your tab.");
      else {
        pendingGoToIntent = null;
        scheduleExtensionTimeout();
      }
      return "sent_to_ext";
    }
    const wantsNativeApp = target_type === "native_app";
    if (wantsNativeApp) {
      try {
        await invoke("open_app", { name: payload });
      } catch {
        await invoke("open_url", { url: resolveUrl(payload) });
      }
    } else {
      await invoke("open_url", { url: resolveUrl(payload) });
    }
    speak(`Opening ${payload}`);
    return "opened_url";
  }
  if (type === "search") {
    await invoke("open_url", { url: `https://duckduckgo.com/?q=${encodeURIComponent(p)}` });
    speak(`Searching for ${p}`);
    return "opened_url";
  }
  if (type === "open_and_search") {
    const [site, ...rest] = p.split("|").map((s) => s.trim());
    const query = rest.join("|").trim();
    if (!site || !query) {
      speak("Try: open Amazon and search for candles.");
      return "done";
    }
    await invoke("open_url", { url: resolveUrl(site) });
    speak("Opening. Searching in a moment.");
    await new Promise((r) => setTimeout(r, PAGE_LOAD_DELAY_MS));
    const sent = await invoke<boolean>("send_to_extension", { command: `search for ${query}` });
    if (!sent) speak("Install the AccessPilot extension for in-page search.");
    else scheduleExtensionTimeout();
    return "done";
  }
  if (extensionActions.includes(type)) {
    const extCmd = formatExtensionCommand(type, p);
    const sent = await invoke<boolean>("send_to_extension", { command: extCmd });
    if (!sent) speak("Install the AccessPilot Chrome extension and refresh your tab, then try again.");
    else {
      if (type === "go_to") pendingGoToIntent = p;
      scheduleExtensionTimeout();
    }
    return "sent_to_ext";
  }
  return "done";
}

async function runCommand(text: string) {
  const { steps } = await parseCommand(text);
  if (!steps.length) {
    speak("I didn't understand. Try: open Wikipedia and search for something.");
    return;
  }

  let justOpenedUrl = false;
  for (const step of steps) {
    if (justOpenedUrl && extensionActions.includes(step.action)) {
      speak("Loading. Give it a moment.");
      await new Promise((r) => setTimeout(r, PAGE_LOAD_DELAY_MS));
      justOpenedUrl = false;
    }
    const result = await executeStep(step);
    justOpenedUrl = result === "opened_url";
  }
}


function formatExtensionCommand(action: string, payload: string): string {
  const p = payload.trim();
  switch (action) {
    case "click":
      return `click ${p}`;
    case "find":
      return `find ${p}`;
    case "find_and_read":
      return `find_and_read ${p}`;
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
