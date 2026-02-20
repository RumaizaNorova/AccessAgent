/**
 * Verifier - confirm success after each action
 */

import type { VerificationResult } from "./types";

let lastUrl = "";
let lastDomSnapshot = "";

function getDomSnapshot(region?: Element): string {
  const root = region || document.body;
  return root.innerHTML.slice(0, 2000);
}

export function verifyAfterAction(
  expected?: {
    urlChange?: boolean;
    textAppear?: string[];
    activeElement?: Element;
  }
): VerificationResult {
  const url = window.location.href;
  const snapshot = getDomSnapshot();

  if (expected?.urlChange && url !== lastUrl) {
    lastUrl = url;
    return { verified: true, reason: "URL changed as expected" };
  }

  if (expected?.textAppear?.length) {
    const bodyText = document.body.innerText.toLowerCase();
    const found = expected.textAppear.some((t) => bodyText.includes(t.toLowerCase()));
    if (found) {
      return { verified: true, reason: `Expected text appeared: ${expected.textAppear.find((t) => bodyText.includes(t.toLowerCase()))}` };
    }
  }

  if (expected?.activeElement && document.activeElement === expected.activeElement) {
    return { verified: true, reason: "Expected element is focused" };
  }

  if (snapshot !== lastDomSnapshot) {
    lastDomSnapshot = snapshot;
    return { verified: true, reason: "DOM updated" };
  }

  if (url !== lastUrl) {
    lastUrl = url;
    return { verified: true, reason: "URL changed" };
  }

  return { verified: false, reason: "No verifiable change detected" };
}

export function resetVerifier(): void {
  lastUrl = window.location.href;
  lastDomSnapshot = getDomSnapshot();
}
