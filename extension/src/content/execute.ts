/**
 * Safe action executor
 * click, focus, type, scroll, back
 */

import type { ActionResult } from "./types";

export function click(el: HTMLElement): ActionResult {
  try {
    if (!el.isConnected) return { success: false, reason: "Element no longer in DOM" };
    el.scrollIntoView({ behavior: "instant", block: "center" });
    el.focus();
    el.click();
    return { success: true };
  } catch (e) {
    return { success: false, reason: String(e) };
  }
}

export function focus(el: HTMLElement): ActionResult {
  try {
    if (!el.isConnected) return { success: false, reason: "Element no longer in DOM" };
    el.scrollIntoView({ behavior: "instant", block: "center" });
    el.focus();
    return { success: true };
  } catch (e) {
    return { success: false, reason: String(e) };
  }
}

export function typeInto(el: HTMLElement, text: string): ActionResult {
  try {
    if (!el.isConnected) return { success: false, reason: "Element no longer in DOM" };
    if (!(el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement)) {
      return { success: false, reason: "Element is not an input" };
    }
    el.scrollIntoView({ behavior: "instant", block: "center" });
    el.focus();
    el.select();
    el.value = text;
    el.dispatchEvent(new Event("input", { bubbles: true }));
    el.dispatchEvent(new Event("change", { bubbles: true }));
    return { success: true };
  } catch (e) {
    return { success: false, reason: String(e) };
  }
}

export function pressEnter(el: HTMLElement): ActionResult {
  try {
    if (!el.isConnected) return { success: false, reason: "Element no longer in DOM" };
    el.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Enter", code: "Enter", bubbles: true })
    );
    el.dispatchEvent(
      new KeyboardEvent("keypress", { key: "Enter", code: "Enter", bubbles: true })
    );
    el.dispatchEvent(
      new KeyboardEvent("keyup", { key: "Enter", code: "Enter", bubbles: true })
    );
    const form = el.closest("form");
    if (form) form.requestSubmit();
    return { success: true };
  } catch (e) {
    return { success: false, reason: String(e) };
  }
}

export function scrollTo(el: Element): ActionResult {
  try {
    if (!el.isConnected) return { success: false, reason: "Element no longer in DOM" };
    el.scrollIntoView({ behavior: "smooth", block: "center" });
    return { success: true };
  } catch (e) {
    return { success: false, reason: String(e) };
  }
}

export function scrollBy(dy: number): ActionResult {
  try {
    window.scrollBy({ top: dy, behavior: "smooth" });
    return { success: true };
  } catch (e) {
    return { success: false, reason: String(e) };
  }
}

export function back(): ActionResult {
  try {
    window.history.back();
    return { success: true };
  } catch (e) {
    return { success: false, reason: String(e) };
  }
}
