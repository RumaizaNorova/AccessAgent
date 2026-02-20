/**
 * App entry - wire UI, voice, agent
 */

const input = document.getElementById("command-input");
const micBtn = document.getElementById("mic-btn");
const statusEl = document.getElementById("status");
const logEl = document.getElementById("log");

function setStatus(msg) {
  statusEl.textContent = msg;
}

function appendLog(msg, isAgent = false) {
  const p = document.createElement("p");
  p.textContent = msg;
  p.style.color = isAgent ? "#0f172a" : "#64748b";
  logEl.appendChild(p);
  logEl.scrollTop = logEl.scrollHeight;
}

function clearLog() {
  logEl.innerHTML = "";
}

async function runCommand(cmd) {
  if (!cmd.trim()) return;
  clearLog();
  appendLog(`Heard: "${cmd}"`);
  setStatus("Working on it...");

  try {
    const result = await runAgent(cmd);
    setStatus(result.success ? "Done" : "Sorry");
    appendLog(`AccessPilot: ${result.message}`, true);

    if (result.stop) {
      window.AccessPilotVoice?.stopSpeaking();
      return;
    }

    if (result.success && result.message) {
      window.AccessPilotVoice?.speak(result.message);
    }
  } catch (e) {
    setStatus("Error");
    appendLog(`Error: ${e.message}`, true);
    window.AccessPilotVoice?.speak("Sorry, something went wrong.");
  }
}

input.addEventListener("keydown", (e) => {
  if (e.key === "Enter") {
    runCommand(input.value);
    input.value = "";
  }
});

let recognizer = null;
let isListening = false;

async function startListening() {
  const voice = window.AccessPilotVoice;
  if (!voice?.hasSpeech || !voice.createRecognizer) {
    setStatus("Voice not supported. Use Chrome or Edge, or type your command.");
    appendLog("Voice not supported in this browser. Try Chrome.", false);
    return;
  }

  // Request mic permission first so user sees the prompt
  try {
    const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
    stream.getTracks().forEach((t) => t.stop());
  } catch (e) {
    setStatus("Microphone blocked. Allow mic in browser settings, or type your command.");
    appendLog("Mic access denied. Check your browser permissions.", false);
    return;
  }

  recognizer = voice.createRecognizer(
    (text) => {
      if (text) {
        input.value = text;
        runCommand(text);
      }
    },
    () => {
      isListening = false;
      micBtn.classList.remove("listening");
      setStatus("");
    },
    (err) => {
      isListening = false;
      micBtn.classList.remove("listening");
      if (err === "not-allowed") {
        setStatus("Microphone blocked. Allow mic, or type your command.");
      } else if (err === "no-speech") {
        setStatus("No speech heard. Click mic again and speak clearly.");
      } else {
        setStatus(`Voice error: ${err}. Try typing instead.`);
      }
    },
    (interim) => {
      input.value = interim;
      setStatus(`Listening…`);
    }
  );
  if (recognizer) {
    recognizer.start();
    isListening = true;
    micBtn.classList.add("listening");
    setStatus("Listening...");
  }
}

function stopListening() {
  if (recognizer && isListening) {
    recognizer.stop();
  }
}

micBtn.addEventListener("click", (e) => {
  e.preventDefault();
  if (isListening) {
    stopListening();
  } else {
    startListening();
  }
});

if (window.electronAPI?.onSummon) {
  window.electronAPI.onSummon(() => {
    input.focus();
    input.select();
  });
}

// Server mode: use fetch to open URLs (no Electron)
if (!window.electronAPI) {
  window.AccessPilotOpenUrl = (url) =>
    fetch(`/open?url=${encodeURIComponent(url)}`).catch(() => window.open(url, "_blank"));
}
