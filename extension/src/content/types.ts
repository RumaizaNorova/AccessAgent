/**
 * Shared types for AccessPilot content scripts
 */

export type CommandType = "OPEN" | "SEARCH" | "FIND" | "CLICK" | "ACCESS_MODE" | "UNKNOWN";

export interface ParsedCommand {
  type: CommandType;
  payload: string;
  raw: string;
}

export interface InteractiveCandidate {
  element: HTMLElement;
  tagName: string;
  role: string;
  labels: string[];
  type: "searchbox" | "button" | "link" | "input" | "other";
  score: number;
  rect: DOMRect | null;
  isVisible: boolean;
}

export interface ActionResult {
  success: boolean;
  reason?: string;
  fallback?: HTMLElement;
}

export interface StepLog {
  step: number;
  total: number;
  action: string;
  target?: string;
  reason?: string;
  success: boolean;
}

export interface VerificationResult {
  verified: boolean;
  reason: string;
}
