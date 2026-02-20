/**
 * Agent - plan-act-verify loop
 * Orchestrates detection, execution, verification
 */

import {
  collectCandidates,
  rankSearchbox,
  rankClickTarget,
  findTextMatch,
} from "./detect";
import { click, focus, typeInto, pressEnter, scrollTo } from "./execute";
import { verifyAfterAction, resetVerifier } from "./verify";
import { parseCommand } from "./parser";
import type { ParsedCommand, StepLog } from "./types";

const MAX_ATTEMPTS = 3;

export type AgentCallback = (log: StepLog) => void;

let panicFlag = false;
let stepCallback: AgentCallback | null = null;

export function setPanic(): void {
  panicFlag = true;
}

export function clearPanic(): void {
  panicFlag = false;
}

export function onStep(cb: AgentCallback): void {
  stepCallback = cb;
}

function logStep(step: StepLog): void {
  stepCallback?.(step);
}

function checkPanic(): boolean {
  if (panicFlag) {
    logStep({
      step: 0,
      total: 0,
      action: "Stopped (panic)",
      success: false,
    });
    return true;
  }
  return false;
}

export async function runCommand(
  rawCommand: string
): Promise<{ success: boolean; message: string; steps: StepLog[] }> {
  panicFlag = false;
  resetVerifier();
  const steps: StepLog[] = [];
  stepCallback = (s) => steps.push(s);

  const parsed = parseCommand(rawCommand);

  if (parsed.type === "UNKNOWN") {
    logStep({
      step: 1,
      total: 1,
      action: "Unknown command",
      reason: `Try: open <url>, search for <query>, find <text>, click <label>`,
      success: false,
    });
    return { success: false, message: "Unknown command", steps };
  }

  if (parsed.type === "OPEN") {
    const url = resolveUrl(parsed.payload);
    logStep({
      step: 1,
      total: 1,
      action: "Navigate",
      target: url,
      reason: "Opening URL",
      success: true,
    });
    window.location.href = url;
    return { success: true, message: `Opening ${url}`, steps };
  }

  if (parsed.type === "FIND") {
    const match = findTextMatch(parsed.payload);
    if (!match) {
      logStep({
        step: 1,
        total: 1,
        action: "Find",
        target: parsed.payload,
        reason: "No match found",
        success: false,
      });
      return { success: false, message: `Could not find "${parsed.payload}"`, steps };
    }
    scrollTo(match.element);
    highlight(match.element);
    logStep({
      step: 1,
      total: 1,
      action: "Find",
      target: parsed.payload,
      reason: `Found and highlighted`,
      success: true,
    });
    return { success: true, message: `Found and highlighted "${parsed.payload}"`, steps };
  }

  if (parsed.type === "SEARCH") {
    const candidates = collectCandidates();
    const searchboxes = rankSearchbox(candidates);
    if (searchboxes.length === 0) {
      logStep({
        step: 1,
        total: 1,
        action: "Search",
        reason: "No search box found on page",
        success: false,
      });
      return { success: false, message: "No search box found", steps };
    }

    for (let i = 0; i < Math.min(MAX_ATTEMPTS, searchboxes.length); i++) {
      if (checkPanic()) return { success: false, message: "Stopped", steps };

      const candidate = searchboxes[i];
      logStep({
        step: i + 1,
        total: MAX_ATTEMPTS,
        action: "Search",
        target: candidate.labels[0] || "search input",
        reason: `Typing "${parsed.payload}"`,
        success: true,
      });

      focus(candidate.element);
      typeInto(candidate.element, parsed.payload);
      pressEnter(candidate.element);

      await delay(500);
      const verified = verifyAfterAction({
        textAppear: ["result", "search", "found", "no result", "0 result"],
      });
      if (verified.verified) {
        return { success: true, message: `Searched for "${parsed.payload}"`, steps };
      }
    }
    return {
      success: true,
      message: `Searched for "${parsed.payload}" (verification inconclusive)`,
      steps,
    };
  }

  if (parsed.type === "CLICK") {
    const candidates = collectCandidates();
    const ranked = rankClickTarget(candidates, parsed.payload);
    if (ranked.length === 0) {
      logStep({
        step: 1,
        total: 1,
        action: "Click",
        target: parsed.payload,
        reason: "No matching element found",
        success: false,
      });
      return { success: false, message: `Could not find "${parsed.payload}" to click`, steps };
    }

    for (let i = 0; i < Math.min(MAX_ATTEMPTS, ranked.length); i++) {
      if (checkPanic()) return { success: false, message: "Stopped", steps };

      const candidate = ranked[i];
      const label = candidate.labels[0] || candidate.tagName;
      logStep({
        step: i + 1,
        total: MAX_ATTEMPTS,
        action: "Click",
        target: label,
        reason: `Best match for "${parsed.payload}"`,
        success: true,
      });

      const result = click(candidate.element);
      if (!result.success) continue;

      await delay(300);
      const verified = verifyAfterAction();
      if (verified.verified || i === ranked.length - 1) {
        return { success: true, message: `Clicked "${label}"`, steps };
      }
    }
    return {
      success: true,
      message: `Attempted to click "${parsed.payload}"`,
      steps,
    };
  }

  if (parsed.type === "ACCESS_MODE") {
    const on = parsed.payload === "on";
    toggleAccessMode(on);
    logStep({
      step: 1,
      total: 1,
      action: "Access mode",
      target: on ? "on" : "off",
      success: true,
    });
    return {
      success: true,
      message: `Access mode ${on ? "enabled" : "disabled"}`,
      steps,
    };
  }

  return { success: false, message: "Unhandled command", steps };
}

function resolveUrl(payload: string): string {
  const trimmed = payload.trim();
  if (/^https?:\/\//i.test(trimmed)) return trimmed;
  if (/^[a-z0-9-]+\.[a-z]{2,}$/i.test(trimmed)) return `https://${trimmed}`;
  return `https://www.google.com/search?q=${encodeURIComponent(trimmed)}`;
}

function delay(ms: number): Promise<void> {
  return new Promise((r) => setTimeout(r, ms));
}

const HIGHLIGHT_STYLE_ID = "accesspilot-highlight-style";
function highlight(el: Element): void {
  removeHighlight();
  if (!(el instanceof HTMLElement)) return;
  el.dataset.accesspilotHighlight = "1";
  el.style.outline = "3px solid #2563eb";
  el.style.outlineOffset = "2px";

  let style = document.getElementById(HIGHLIGHT_STYLE_ID) as HTMLStyleElement;
  if (!style) {
    style = document.createElement("style");
    style.id = HIGHLIGHT_STYLE_ID;
    style.textContent = `
      @keyframes accesspilot-pulse {
        0%, 100% { outline-color: #2563eb; }
        50% { outline-color: #60a5fa; }
      }
    `;
    document.head.appendChild(style);
  }
  el.style.animation = "accesspilot-pulse 1s ease-in-out 3";

  setTimeout(removeHighlight, 3000);
}

export function removeHighlight(): void {
  document.querySelectorAll("[data-accesspilot-highlight]").forEach((el) => {
    if (el instanceof HTMLElement) {
      el.style.outline = "";
      el.style.outlineOffset = "";
      el.style.animation = "";
      delete el.dataset.accesspilotHighlight;
    }
  });
}

let accessModeOverlay: HTMLElement | null = null;
function toggleAccessMode(on: boolean): void {
  if (accessModeOverlay) {
    accessModeOverlay.remove();
    accessModeOverlay = null;
    if (!on) return;
  }
  if (on) {
    accessModeOverlay = createAccessModeOverlay();
    document.body.appendChild(accessModeOverlay);
  }
}

function createAccessModeOverlay(): HTMLElement {
  const root = document.createElement("div");
  root.id = "accesspilot-access-mode";
  root.style.cssText = `
    position: fixed;
    bottom: 0;
    left: 0;
    right: 0;
    height: 72px;
    background: rgba(15, 23, 42, 0.95);
    border-top: 1px solid rgba(255,255,255,0.1);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 2147483646;
    font-family: system-ui, sans-serif;
  `;
  root.innerHTML = `
    <span style="color: #94a3b8; font-size: 14px;">One-hand mode • Target boost active • Press Esc to exit</span>
  `;
  document.body.querySelectorAll("button, a, input, [role=button]").forEach((el) => {
    if (el instanceof HTMLElement) {
      el.style.minHeight = "44px";
      el.style.minWidth = "44px";
    }
  });
  return root;
}
