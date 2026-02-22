import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { listen } from "@tauri-apps/api/event";
import { requestPermission } from "tauri-plugin-stt-api";
import AudioRecorder from "audio-recorder-polyfill";

const icon = document.getElementById("icon")!;
const agentWrap = document.getElementById("agent-wrap")!;
const modePicker = document.getElementById("mode-picker")!;
const agentMain = document.getElementById("agent-main")!;
const modeVoiceBtn = document.getElementById("mode-voice")!;
const modeGazeBtn = document.getElementById("mode-gaze")!;
const switchModeBtn = document.getElementById("switch-mode")!;

const STORAGE_KEY = "accesspilot_input_mode";
type InputMode = "voice" | "gaze";

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
    speak("Extension didn't respond. Make sure Chrome is open with a regular webpage, the AccessPilot extension is enabled, and try refreshing the tab.");
  }, EXTENSION_TIMEOUT_MS);
}

/** Shorten errors for TTS (max ~80 chars). Don't mask — surface the real issue. */
function shortenForTts(msg: string): string {
  const s = msg.trim();
  if (s.length <= 80) return s;
  return s.slice(0, 77) + "...";
}

type Step = { action: string; payload: string; target_type?: string };
type ParsedIntent = { steps: Step[]; chat_reply?: string | null };

async function parseCommand(text: string, frontmostApp: string | null): Promise<ParsedIntent> {
  const intent = await invoke<ParsedIntent>("parse_intent", {
    command: text,
    apiKeyOverride: null,
    frontmostApp: frontmostApp ?? null,
  });
  return {
    steps: intent.steps || [],
    chat_reply: intent.chat_reply ?? null,
  };
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

/** Map user phrases to macOS app names for open_app. */
function resolveNativeAppName(payload: string): string {
  const p = payload.trim().toLowerCase();
  const aliases: Record<string, string> = {
    email: "Mail",
    "my email": "Mail",
    mail: "Mail",
    messages: "Messages",
    imessage: "Messages",
    notes: "Notes",
    finder: "Finder",
    settings: "System Settings",
    "system settings": "System Settings",
    "system preferences": "System Settings",
    calendar: "Calendar",
    reminders: "Reminders",
    "vs code": "Visual Studio Code",
    vscode: "Visual Studio Code",
    "visual studio code": "Visual Studio Code",
    slack: "Slack",
    discord: "Discord",
    zoom: "Zoom",
    spotify: "Spotify",
    teams: "Microsoft Teams",
    outlook: "Outlook",
    notion: "Notion",
  };
  return aliases[p] || payload.trim();
}

const PAGE_LOAD_DELAY_MS = 3500;
const extensionActions = ["click", "find", "find_and_read", "find_next", "find_prev", "page_search", "scroll", "go_to_page", "access_mode", "close_popup", "go_to", "type"];

async function executeStep(step: Step, isChromeFocused: boolean): Promise<"opened_url" | "sent_to_ext" | "done"> {
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
      if (!isChromeFocused) {
        speak("I need Chrome focused for that. Switch to Chrome, then say it again.");
        return "sent_to_ext";
      }
      const sent = await invoke<boolean>("send_to_extension", { command: `click ${payload}` });
      if (!sent) speak("Chrome extension not connected. Open Chrome, load AccessPilot from chrome://extensions, open a webpage, then try again.");
      else {
        pendingGoToIntent = null;
        scheduleExtensionTimeout();
      }
      return "sent_to_ext";
    }
    const wantsNativeApp = target_type === "native_app";
    if (wantsNativeApp) {
      const appName = resolveNativeAppName(payload);
      try {
        await invoke("open_app", { name: appName });
      } catch {
        await invoke("open_url", { url: resolveUrl(payload) });
      }
    } else {
      await invoke("open_url", { url: resolveUrl(payload) });
    }
    speak(`Opening ${wantsNativeApp ? resolveNativeAppName(payload) : payload}`);
    return "opened_url";
  }
  if (type === "search") {
    await invoke("open_url", { url: `https://duckduckgo.com/?q=${encodeURIComponent(p)}` });
    speak(`Searching for ${p}`);
    return "opened_url";
  }
  if (type === "open_and_search") {
    let site = "";
    let query = "";
    if (p.includes("|")) {
      const parts = p.split("|").map((s) => s.trim());
      site = parts[0] || "";
      query = parts.slice(1).join("|").trim();
    } else {
      // Parse "imagenet in wikipedia", "wikipedia imagenet", "look for X in Y" when GPT misses the pipe
      const knownSites = ["wikipedia", "amazon", "google", "youtube", "reddit", "github", "ebay", "walmart", "gmail"];
      const lower = p.toLowerCase().trim();
      const inMatch = lower.match(/^(.+?)\s+in\s+(wikipedia|amazon|google|youtube|reddit|github|ebay|walmart|gmail)$/i);
      const siteFirst = lower.match(/^(wikipedia|amazon|google|youtube|reddit|github|ebay|walmart|gmail)\s+(.+)$/i);
      const siteLast = lower.match(/^(.+?)\s+(wikipedia|amazon|google|youtube|reddit|github|ebay|walmart|gmail)$/i);
      if (inMatch) {
        query = inMatch[1].replace(/^(look for|search for|find)\s+/i, "").trim();
        site = inMatch[2].toLowerCase();
      } else if (siteFirst) {
        site = siteFirst[1].toLowerCase();
        query = siteFirst[2].replace(/^(and |then |go to |look for |search for |find )/i, "").trim();
      } else if (siteLast) {
        site = siteLast[2].toLowerCase();
        query = siteLast[1].replace(/^(look for|search for|find)\s+/i, "").trim();
      } else {
        const found = knownSites.find((s) => lower.includes(s));
        if (found) {
          site = found;
          query = lower
          .replace(new RegExp(found, "gi"), "")
          .replace(/\b(open|and|in|go to|look for|search for|find|the)\b/gi, "")
          .replace(/\s+/g, " ")
          .trim();
        }
      }
    }
    if (!site || !query) {
      speak("I didn't get that. Try: open Wikipedia and search for imagenet.");
      return "done";
    }
    await invoke("open_url", { url: resolveUrl(site) });
    speak("Opening. Searching in a moment.");
    await new Promise((r) => setTimeout(r, PAGE_LOAD_DELAY_MS));
    // Strip site:.org etc — GPT sometimes adds web-search modifiers; in-page search doesn't use them
    const cleanQuery = query.replace(/\s*site:[^\s]*/gi, "").trim();
    const sent = await invoke<boolean>("send_to_extension", { command: `search for ${cleanQuery}` });
    if (!sent) speak("Chrome extension not connected. Open Chrome, load AccessPilot from chrome://extensions, open a webpage, then try again.");
    else scheduleExtensionTimeout();
    return "done";
  }
  if (type === "press_keys") {
    try {
      // Restore focus to the app we stole it from (e.g. Chrome) so keystroke goes there
      if (lastFocusedAppBundleId) {
        await invoke("activate_app", { bundleId: lastFocusedAppBundleId });
        await new Promise((r) => setTimeout(r, 150));
      }
      const combo = normalizeKeyCombo(p.trim());
      await invoke("press_keys", { payload: combo });
      speak(`Pressed ${combo}`);
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      speak(shortenForTts(msg));
    }
    return "done";
  }
  if (extensionActions.includes(type)) {
    if (!isChromeFocused) {
      speak("I need Chrome focused for that. Switch to Chrome, then say it again.");
      return "done";
    }
    const extCmd = formatExtensionCommand(type, p);
    const sent = await invoke<boolean>("send_to_extension", { command: extCmd });
    if (!sent) speak("Chrome extension not connected. Open Chrome, load AccessPilot from chrome://extensions, open a webpage, then try again.");
    else {
      if (type === "go_to") pendingGoToIntent = p;
      scheduleExtensionTimeout();
    }
    return "sent_to_ext";
  }
  return "done";
}

/** Normalize key combo for press_keys: "Command+S" (rdev uses Meta for Command on Mac). */
function normalizeKeyCombo(p: string): string {
  return p
    .replace(/\s+/g, "")
    .replace(/\bcmd\b/gi, "Command")
    .replace(/\bctrl\b/gi, "Control")
    .replace(/\balt\b/gi, "Option")
    .replace(/\bopt\b/gi, "Option");
}

async function runCommand(text: string) {
  const frontmostApp = await invoke<string | null>("get_frontmost_app");
  const isChromeFocused = frontmostApp?.toLowerCase().includes("chrome") ?? false;

  const { steps, chat_reply } = await parseCommand(text, frontmostApp);

  // Conversational intent: speak reply and return (no steps to execute)
  if (chat_reply?.trim()) {
    speak(chat_reply.trim());
    return;
  }

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
    const result = await executeStep(step, isChromeFocused);
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
    case "find_next":
      return "find next match";
    case "find_prev":
      return "find prev match";
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
    case "go_to_page":
      return `go_to_page:${p}`;
    case "type":
      return `type ${p}`;
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

const CHUNK_INTERVAL_MS = 100;
const SILENCE_MS = 2000; // Auto-stop after 2s silence (allow "close the messages app" etc. to finish)
const SILENCE_CHECK_MS = 100;

async function startListeningWhisper(onSilence?: () => void) {
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
  recorder.start(CHUNK_INTERVAL_MS);
  mediaRecorder = recorder as MediaRecorder;

  // Auto-stop on silence: one tap, talk, done (accessibility-friendly)
  if (onSilence && typeof AudioContext !== "undefined") {
    const ctx = new AudioContext();
    const src = ctx.createMediaStreamSource(stream);
    const analyser = ctx.createAnalyser();
    analyser.fftSize = 256;
    analyser.smoothingTimeConstant = 0.8;
    src.connect(analyser);
    const data = new Uint8Array(analyser.frequencyBinCount);
    let lastSpeechAt = 0;
    let hasHeardSpeech = false;
    const check = () => {
      if (!mediaRecorder || mediaRecorder.state === "inactive") return;
      analyser.getByteFrequencyData(data);
      const avg = data.reduce((a, b) => a + b, 0) / data.length;
      if (avg > 15) {
        hasHeardSpeech = true;
        lastSpeechAt = Date.now();
      }
      if (hasHeardSpeech && Date.now() - lastSpeechAt > SILENCE_MS) {
        onSilence();
        return;
      }
      setTimeout(check, SILENCE_CHECK_MS);
    };
    setTimeout(check, 500); // Start after a brief delay
  }
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

async function processAndRun() {
  isListening = false;
  icon.classList.remove("listening");
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
      speak("No speech detected. Try again.");
    }
  } catch (err) {
    const msg = err instanceof Error ? err.message : String(err);
    console.error("Transcription error:", msg);
    speak(shortenForTts(msg));
  }
}

async function startListening() {
  if (isStarting) {
    speak("Please wait.");
    return;
  }
  if (isListening) {
    await processAndRun();
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
    await startListeningWhisper(() => {
      if (isListening) processAndRun();
    });
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

// Store app that had focus before we stole it (so press_keys goes there, not to us)
let lastFocusedAppBundleId: string | null = null;

// Global hotkey: Option+Space — summon agent and start listening from anywhere (voice mode only)
import("@tauri-apps/plugin-global-shortcut")
  .then(({ register }) =>
    register("Alt+Space", async (event) => {
      if (event.state !== "Pressed") return;
      if (getStoredMode() === "gaze") return;
      try {
        const bundleId = await invoke<string | null>("get_frontmost_app");
        if (bundleId && !bundleId.toLowerCase().includes("accesspilot")) {
          lastFocusedAppBundleId = bundleId;
        }
      } catch {
        /* ignore */
      }
      startListening();
    })
  )
  .catch((e) => console.warn("[AccessPilot] Hotkey unavailable:", e));

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

function getStoredMode(): InputMode | null {
  try {
    const m = localStorage.getItem(STORAGE_KEY);
    if (m === "voice" || m === "gaze") return m;
  } catch {
    // ignore
  }
  return null;
}

function setStoredMode(mode: InputMode) {
  localStorage.setItem(STORAGE_KEY, mode);
}

async function notifyExtensionMode(mode: InputMode) {
  try {
    await invoke<boolean>("send_to_extension", { command: `INPUT_MODE:${mode}` });
  } catch {
    // Extension may not be connected
  }
}

function applyMode(mode: InputMode) {
  setStoredMode(mode);
  notifyExtensionMode(mode);
  agentWrap.classList.remove("voice-mode", "gaze-mode");
  agentWrap.classList.add(`${mode}-mode`);

  if (mode === "voice") {
    agentMain.querySelector("#icon")!.innerHTML = `
      <svg viewBox="0 0 24 24" fill="currentColor" width="40" height="40">
        <path d="M12 14c1.66 0 3-1.34 3-3V5c0-1.66-1.34-3-3-3S9 3.34 9 5v6c0 1.66 1.34 3 3 3zm5.91-3c-.49 0-.9.36-.98.85C16.52 14.2 14.47 16 12 16s-4.52-1.8-4.93-4.15c-.08-.49-.49-.85-.98-.85-.61 0-1.09.54-1 1.14.49 3 2.89 5.35 5.91 5.78V20c0 .55.45 1 1 1s1-.45 1-1v-2.08c3.02-.43 5.42-2.78 5.91-5.78.1-.6-.39-1.14-1-1.14z"/>
      </svg>`;
    speak("Voice mode. Tap or press Option+Space to talk.");
  } else {
    agentMain.querySelector("#icon")!.innerHTML = `
      <svg viewBox="0 0 24 24" fill="currentColor" width="40" height="40">
        <path d="M12 4.5C7 4.5 2.73 7.61 1 12c1.73 4.39 6 7.5 11 7.5s9.27-3.11 11-7.5c-1.73-4.39-6-7.5-11-7.5zM12 17c-2.76 0-5-2.24-5-5s2.24-5 5-5 5 2.24 5 5-2.24 5-5 5zm0-8c-1.66 0-3 1.34-3 3s1.34 3 3 3 3-1.34 3-3-1.34-3-3-3z"/>
      </svg>`;
    speak("Gaze mode. Open Chrome, go to a webpage, then click Start Gaze in the bottom right. Allow camera. Then look at buttons to click.");
  }
}

function showModePicker() {
  modePicker.classList.remove("hidden");
  agentMain.classList.add("hidden");
  agentMain.style.display = "none";
}

function showAgentMain(mode: InputMode) {
  modePicker.classList.add("hidden");
  agentMain.classList.remove("hidden");
  agentMain.style.display = "flex";
  applyMode(mode);
}

function initMode() {
  const saved = getStoredMode();
  if (saved) {
    showAgentMain(saved);
  } else {
    showModePicker();
  }
}

modeVoiceBtn.addEventListener("click", () => {
  showAgentMain("voice");
});

modeGazeBtn.addEventListener("click", () => {
  showAgentMain("gaze");
});

switchModeBtn.addEventListener("click", (e) => {
  e.stopPropagation();
  showModePicker();
});

async function handleMicAction() {
  if (isStarting) return;
  const mode = getStoredMode();
  if (mode === "gaze") {
    speak("Gaze mode is on. Open a webpage in Chrome and look at items to click.");
    return;
  }
  if (isListening) {
    await processAndRun();
    return;
  }
  await startListening();
}

// Whole window tap — voice mode only does mic
agentWrap.addEventListener("pointerdown", (e) => {
  e.preventDefault();
  e.stopPropagation();
  if (modePicker.classList.contains("hidden")) {
    handleMicAction();
  }
});

icon.addEventListener("pointerdown", (e) => {
  e.preventDefault();
  e.stopPropagation();
  handleMicAction();
});

initMode();
