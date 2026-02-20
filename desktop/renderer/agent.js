/**
 * Desktop agent - parse intent, execute actions, return message to speak
 * Runs in renderer; uses electronAPI.openExternal for URLs
 */

function parseCommand(raw) {
  const trimmed = raw.trim().toLowerCase().replace(/\s+/g, " ");
  if (!trimmed) return { type: "UNKNOWN", payload: "" };

  // open something
  if (/open\s+(.+)/.test(trimmed)) {
    return { type: "OPEN", payload: trimmed.replace(/.*?open\s+/, "").trim() };
  }
  // search for something
  if (/search(?:\s+for)?\s+(.+)/.test(trimmed)) {
    return { type: "SEARCH", payload: trimmed.replace(/.*?search(?:\s+for)?\s+/, "").trim() };
  }
  // time
  if (/\btime\b|what'?s?\s+the\s+time/.test(trimmed)) {
    return { type: "TIME", payload: "" };
  }
  // date
  if (/\bdate\b|what'?s?\s+the\s+date/.test(trimmed)) {
    return { type: "DATE", payload: "" };
  }
  // stop
  if (/^(stop|cancel|never\s*mind)$/.test(trimmed)) {
    return { type: "STOP", payload: "" };
  }

  // anything else → treat as search (so "scholarships" or "wikipedia" just works)
  return { type: "SEARCH", payload: trimmed };
}

function resolveUrl(payload) {
  const t = payload.trim();
  if (/^https?:\/\//i.test(t)) return t;
  if (/^[a-z0-9-]+\.[a-z]{2,}$/i.test(t)) return `https://${t}`;
  return `https://www.google.com/search?q=${encodeURIComponent(t)}`;
}

async function runAgent(rawCommand) {
  const parsed = parseCommand(rawCommand);

  if (parsed.type === "UNKNOWN") {
    return {
      success: false,
      message: "Say something like: open wikipedia, search scholarships, or what time is it.",
    };
  }

  if (parsed.type === "STOP") {
    return { success: true, message: "Stopped.", stop: true };
  }

  if (parsed.type === "TIME") {
    const now = new Date();
    const time = now.toLocaleTimeString([], { hour: "numeric", minute: "2-digit" });
    return { success: true, message: `The time is ${time}.` };
  }

  if (parsed.type === "DATE") {
    const now = new Date();
    const date = now.toLocaleDateString([], { weekday: "long", year: "numeric", month: "long", day: "numeric" });
    return { success: true, message: `Today is ${date}.` };
  }

  if (parsed.type === "OPEN") {
    const url = resolveUrl(parsed.payload);
    if (window.electronAPI?.openExternal) {
      await window.electronAPI.openExternal(url);
    } else if (window.AccessPilotOpenUrl) {
      await window.AccessPilotOpenUrl(url);
    } else {
      window.open(url, "_blank");
    }
    const site = parsed.payload.includes(".") ? parsed.payload : `search for ${parsed.payload}`;
    return { success: true, message: `Opening ${site}.` };
  }

  if (parsed.type === "SEARCH") {
    const url = `https://www.google.com/search?q=${encodeURIComponent(parsed.payload)}`;
    if (window.electronAPI?.openExternal) {
      await window.electronAPI.openExternal(url);
    } else if (window.AccessPilotOpenUrl) {
      await window.AccessPilotOpenUrl(url);
    } else {
      window.open(url, "_blank");
    }
    return { success: true, message: `Searching for ${parsed.payload}. Results are opening in your browser.` };
  }

  return { success: false, message: "I couldn't do that." };
}
