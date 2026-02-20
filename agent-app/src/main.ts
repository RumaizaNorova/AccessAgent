import { invoke } from "@tauri-apps/api/core";

const SpeechRecognition = (window as any).SpeechRecognition || (window as any).webkitSpeechRecognition;
const icon = document.getElementById("icon")!;

function speak(text: string) {
  if (window.speechSynthesis) {
    window.speechSynthesis.cancel();
    const u = new SpeechSynthesisUtterance(text);
    window.speechSynthesis.speak(u);
  }
}

function parseCommand(raw: string): { type: string; payload: string } {
  const t = raw.trim().toLowerCase().replace(/\s+/g, " ");
  if (!t) return { type: "unknown", payload: "" };

  if (/open\s+(.+)/.test(t)) return { type: "open", payload: t.replace(/.*?open\s+/, "").trim() };
  if (/search(?:\s+for)?\s+(.+)/.test(t)) return { type: "search", payload: t.replace(/.*?search(?:\s+for)?\s+/, "").trim() };
  if (/\b(time|what'?s?\s+the\s+time)\b/.test(t)) return { type: "time", payload: "" };
  if (/\b(date|what'?s?\s+the\s+date)\b/.test(t)) return { type: "date", payload: "" };
  if (/^(stop|cancel)$/.test(t)) return { type: "stop", payload: "" };

  return { type: "search", payload: t };
}

function resolveUrl(payload: string): string {
  const p = payload.trim();
  if (/^https?:\/\//i.test(p)) return p;
  if (/^[a-z0-9-]+\.[a-z]{2,}$/i.test(p)) return `https://${p}`;
  return `https://www.google.com/search?q=${encodeURIComponent(p)}`;
}

async function runCommand(text: string) {
  const parsed = parseCommand(text);

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
    // App names: no dot, or known apps
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
    const url = `https://www.google.com/search?q=${encodeURIComponent(parsed.payload)}`;
    await invoke("open_url", { url });
    speak(`Searching for ${parsed.payload}`);
    return;
  }

  speak("I didn't understand. Try: open wikipedia, or search for something.");
}

let recognizer: any = null;
let isListening = false;

function startListening() {
  if (!SpeechRecognition) {
    speak("Voice not supported. Please use Chrome.");
    return;
  }

  try {
    recognizer = new SpeechRecognition();
    recognizer.continuous = false;
    recognizer.interimResults = true;
    recognizer.lang = navigator.language || "en-US";

    recognizer.onresult = (e: any) => {
      for (let i = e.resultIndex; i < e.results.length; i++) {
        const r = e.results[i];
        const t = (r[0]?.transcript || "").trim();
        if (r.isFinal && t) runCommand(t);
      }
    };

    recognizer.onend = () => {
      isListening = false;
      icon.classList.remove("listening");
    };

    recognizer.onerror = (e: any) => {
      isListening = false;
      icon.classList.remove("listening");
      if (e.error !== "aborted") speak("Couldn't hear you. Try again.");
    };

    navigator.mediaDevices.getUserMedia({ audio: true }).then((stream) => {
      stream.getTracks().forEach((t) => t.stop());
      recognizer.start();
      isListening = true;
      icon.classList.add("listening");
    }).catch(() => {
      speak("Allow microphone to use voice.");
    });
  } catch (e) {
    speak("Voice error. Try again.");
  }
}

function stopListening() {
  if (recognizer && isListening) {
    recognizer.stop();
  }
}

icon.addEventListener("click", (e) => {
  e.preventDefault();
  if (isListening) {
    stopListening();
  } else {
    startListening();
  }
});

