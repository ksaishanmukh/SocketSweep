import type { NodeRef } from "../api";
import { sameNode } from "./size";

/** The folders visited, browser style: `index` is the one on screen. */
export interface History {
  entries: NodeRef[];
  index: number;
}

export const EMPTY_HISTORY: History = { entries: [], index: -1 };

/** Going somewhere new drops whatever was ahead, as a browser does. Revisiting the current folder is not a step. */
export function visit(history: History, node: NodeRef): History {
  if (sameNode(history.entries[history.index], node)) return history;
  const entries = [...history.entries.slice(0, history.index + 1), node];
  return { entries, index: entries.length - 1 };
}

/** Moves back (-1) or forward (+1), staying put at either end. */
export function step(history: History, delta: -1 | 1): History {
  const index = history.index + delta;
  if (index < 0 || index >= history.entries.length) return history;
  return { ...history, index };
}

export function canGoBack(history: History): boolean {
  return history.index > 0;
}

export function canGoForward(history: History): boolean {
  return history.index < history.entries.length - 1;
}

export function currentEntry(history: History): NodeRef | null {
  return history.entries[history.index] ?? null;
}
