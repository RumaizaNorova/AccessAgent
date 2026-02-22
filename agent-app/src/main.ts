import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
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
