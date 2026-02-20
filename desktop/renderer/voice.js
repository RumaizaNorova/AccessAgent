/**
 * Voice input (Speech-to-Text) and output (TTS)
 * Accessibility-first for users who cannot move freely
 */

const SpeechRecognition = window.SpeechRecognition || window.webkitSpeechRecognition;

function speak(text, opts = {}) {
  if (window.speechSynthesis) {
    window.speechSynthesis.cancel();
    const u = new SpeechSynthesisUtterance(text);
    u.rate = opts.rate ?? 1;
    u.pitch = opts.pitch ?? 1;
    u.volume = 1;
    window.speechSynthesis.speak(u);
  }
}

function stopSpeaking() {
  if (window.speechSynthesis) {
    window.speechSynthesis.cancel();
  }
}

function createRecognizer(onResult, onEnd, onError, onInterim) {
  if (!SpeechRecognition) return null;
  const rec = new SpeechRecognition();
  rec.continuous = false;  // one phrase, then stop (easier for short commands)
  rec.interimResults = true;
  rec.lang = navigator.language || "en-US";

  rec.onresult = (e) => {
    for (let i = e.resultIndex; i < e.results.length; i++) {
      const r = e.results[i];
      const t = (r[0] && r[0].transcript) || "";
      if (r.isFinal && t.trim()) {
        onResult(t.trim());
      } else if (onInterim && t) {
        onInterim(t.trim());
      }
    }
  };

  rec.onend = () => onEnd();

  rec.onerror = (e) => {
    if (e.error !== "aborted") onError(e.error);
  };

  return rec;
}

window.AccessPilotVoice = { speak, stopSpeaking, createRecognizer, hasSpeech: !!SpeechRecognition };
