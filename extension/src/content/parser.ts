/**
 * Command parser - maps user input to command intents
 */

import type { ParsedCommand } from "./types";

export function parseCommand(raw: string): ParsedCommand {
  const trimmed = raw.trim().toLowerCase();
  if (!trimmed) return { type: "UNKNOWN", payload: "", raw };

  if (/^open\s+(.+)/.test(trimmed)) {
    const payload = trimmed.replace(/^open\s+/, "").trim();
    return { type: "OPEN", payload, raw };
  }
  if (/^search\s+(?:for\s+)?(.+)/.test(trimmed)) {
    const payload = trimmed.replace(/^search\s+(?:for\s+)?/, "").trim();
    return { type: "SEARCH", payload, raw };
  }
  if (/^find\s+(.+)/.test(trimmed)) {
    const payload = trimmed.replace(/^find\s+/, "").trim();
    return { type: "FIND", payload, raw };
  }
  if (/^click\s+(.+)/.test(trimmed)) {
    const payload = trimmed.replace(/^click\s+/, "").trim();
    return { type: "CLICK", payload, raw };
  }
  if (/(?:one[- ]?hand\s+mode|target\s+boost)\s+(on|off)/.test(trimmed)) {
    const match = trimmed.match(/(on|off)/);
    return { type: "ACCESS_MODE", payload: match ? match[1] : "toggle", raw };
  }

  // Bare commands: treat "continue" as click continue, "scholarships" as search
  if (/^(continue|next|submit|back)$/i.test(trimmed)) {
    return { type: "CLICK", payload: trimmed, raw };
  }
  const bare = trimmed.split(/\s+/);
  if (bare.length === 1 && bare[0].length > 2) {
    return { type: "FIND", payload: bare[0], raw };
  }

  return { type: "UNKNOWN", payload: trimmed, raw };
}
