/**
 * Detection + ranking engine
 * Collects interactive candidates and ranks them by intent
 */

import type { InteractiveCandidate } from "./types";

const SEARCH_INDICATORS = [
  "search",
  "q",
  "query",
  "s",
  "find",
  "look",
  "go",
  "searchbox",
  "search-input",
];
const PRIMARY_ACTION_PATTERNS = [
  /submit|continue|next|add to cart|checkout|save|confirm|apply|send|go|submit|login|sign in|register|buy now/i,
  /^(continue|next|submit|save|go|ok|yes)$/i,
];

function getLabels(el: Element): string[] {
  const labels: string[] = [];

  const ariaLabel = el.getAttribute("aria-label");
  if (ariaLabel) labels.push(ariaLabel.trim().toLowerCase());

  const ariaLabelledBy = el.getAttribute("aria-labelledby");
  if (ariaLabelledBy) {
    const ids = ariaLabelledBy.split(/\s+/);
    for (const id of ids) {
      const ref = document.getElementById(id);
      if (ref) labels.push(ref.textContent?.trim().toLowerCase() || "");
    }
  }

  const labelFor = document.querySelector(`label[for="${el.id}"]`);
  if (labelFor && el.id) labels.push(labelFor.textContent?.trim().toLowerCase() || "");

  const placeholder = el.getAttribute("placeholder");
  if (placeholder) labels.push(placeholder.trim().toLowerCase());

  const title = el.getAttribute("title");
  if (title) labels.push(title.trim().toLowerCase());

  const name = (el as HTMLInputElement).name;
  if (name) labels.push(name.trim().toLowerCase());

  const alt = (el as HTMLImageElement).alt;
  if (alt) labels.push(alt.trim().toLowerCase());

  const text = el.textContent?.trim();
  if (text && text.length < 100) labels.push(text.toLowerCase());

  return [...new Set(labels.filter(Boolean))];
}

function isVisible(el: Element): boolean {
  if (el instanceof HTMLElement) {
    const style = window.getComputedStyle(el);
    if (style.display === "none" || style.visibility === "hidden" || style.opacity === "0")
      return false;
    const rect = el.getBoundingClientRect();
    if (rect.width === 0 || rect.height === 0) return false;
    return true;
  }
  return false;
}

function isEnabled(el: Element): boolean {
  if (el instanceof HTMLButtonElement || el instanceof HTMLInputElement) {
    return !el.disabled;
  }
  if (el.getAttribute("aria-disabled") === "true") return false;
  return true;
}

export function collectCandidates(): InteractiveCandidate[] {
  const candidates: InteractiveCandidate[] = [];
  const interactive = document.querySelectorAll(
    "button, [role='button'], a[href], input:not([type='hidden']), select, textarea, [tabindex]:not([tabindex='-1']), [role='link'], [role='searchbox'], [role='combobox'], [role='option']"
  );

  for (const el of interactive) {
    if (!(el instanceof HTMLElement)) continue;
    if (!isVisible(el) || !isEnabled(el)) continue;

    const rect = el.getBoundingClientRect();
    if (rect.width === 0 || rect.height === 0) continue;

    const labels = getLabels(el);
    const tagName = el.tagName.toLowerCase();
    const role = el.getAttribute("role") || "";

    let type: InteractiveCandidate["type"] = "other";
    if (
      (el instanceof HTMLInputElement && (el.type === "search" || el.type === "text")) ||
      role === "searchbox" ||
      SEARCH_INDICATORS.some((s) => labels.some((l) => l.includes(s)))
    ) {
      type = "searchbox";
    } else if (
      el instanceof HTMLButtonElement ||
      role === "button" ||
      tagName === "button"
    ) {
      type = "button";
    } else if (el instanceof HTMLAnchorElement || role === "link" || tagName === "a") {
      type = "link";
    } else if (
      el instanceof HTMLInputElement ||
      el instanceof HTMLTextAreaElement ||
      el instanceof HTMLSelectElement
    ) {
      type = "input";
    }

    candidates.push({
      element: el,
      tagName,
      role,
      labels,
      type,
      score: 0,
      rect,
      isVisible: true,
    });
  }

  return candidates;
}

export function rankSearchbox(candidates: InteractiveCandidate[]): InteractiveCandidate[] {
  return candidates
    .filter((c) => c.type === "searchbox")
    .map((c) => ({
      ...c,
      score:
        (c.element instanceof HTMLInputElement && c.element.type === "search" ? 20 : 0) +
        (c.element.getAttribute("role") === "searchbox" ? 15 : 0) +
        (c.labels.some((l) => l.includes("search")) ? 25 : 0) +
        (c.rect ? Math.min(c.rect.width, 400) / 100 : 0),
    }))
    .sort((a, b) => b.score - a.score);
}

export function rankPrimaryAction(candidates: InteractiveCandidate[]): InteractiveCandidate[] {
  return candidates
    .filter((c) => c.type === "button" || c.type === "link")
    .map((c) => {
      const labelText = c.labels.join(" ");
      const match = PRIMARY_ACTION_PATTERNS.some((p) => p.test(labelText));
      let score = match ? 30 : 0;
      if (c.element instanceof HTMLButtonElement && c.element.type === "submit") score += 20;
      if (c.rect) score += Math.min(c.rect.width * c.rect.height, 10000) / 1000;
      return { ...c, score };
    })
    .sort((a, b) => b.score - a.score);
}

export function rankClickTarget(
  candidates: InteractiveCandidate[],
  query: string
): InteractiveCandidate[] {
  const q = query.toLowerCase().trim();
  const words = q.split(/\s+/);

  return candidates
    .filter((c) => c.type === "button" || c.type === "link" || c.type === "input" || c.type === "other")
    .map((c) => {
      const labelText = c.labels.join(" ");
      let score = 0;

      if (labelText.includes(q)) score += 50;
      const wordMatches = words.filter((w) => labelText.includes(w));
      score += wordMatches.length * 15;

      if (c.rect) {
        score += Math.min(c.rect.width * c.rect.height, 5000) / 500;
        if (c.rect.top >= 0 && c.rect.top < window.innerHeight) score += 10;
      }

      return { ...c, score };
    })
    .filter((c) => c.score > 0)
    .sort((a, b) => b.score - a.score);
}

export function findTextMatch(query: string): { element: Element; text: string } | null {
  const q = query.toLowerCase();
  const selectors = ["h1", "h2", "h3", "h4", "h5", "h6", "p", "li", "td", "section", "article", "div"];
  for (const sel of selectors) {
    const elements = document.querySelectorAll(sel);
    for (const el of elements) {
      const text = el.textContent?.trim() || "";
      if (text.length > 5 && text.toLowerCase().includes(q)) {
        return { element: el, text };
      }
    }
  }
  return null;
}
