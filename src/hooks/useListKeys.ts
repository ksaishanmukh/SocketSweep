import { useLayoutEffect, useRef, useState, type KeyboardEvent, type RefObject } from "react";
import type { Row } from "../api";
import {
  clampIndex,
  indicesIn,
  leftBehind,
  moveTarget,
  spanBetween,
  type Span,
} from "../lib/listKeys";

/** What the list's keys do beyond moving: the screen's own actions. */
export interface ListKeyActions {
  back: () => void;
  pick: (rows: Row[], on: boolean) => void;
  clearPicks: () => void;
  /** Stages what is picked, or these rows when nothing is. */
  deleteRows: (rows: Row[]) => void;
}

interface Deps {
  rows: Row[];
  /** Changes when the list shows another folder; the active row starts over and takes focus. */
  resetKey: string;
  listRef: RefObject<HTMLElement | null>;
  scrollTo: (index: number) => void;
  /** Rows that fit on screen, for PageUp and PageDown. */
  pageSize: () => number;
  isPicked: (row: Row) => boolean;
  open: (row: Row) => void;
  keys: ListKeyActions;
}

const rowsAt = (rows: Row[], indices: number[]) => indices.map((i) => rows[i]).filter(Boolean);

/**
 * Roving focus for the list: one row is the tab stop, arrow keys move it,
 * and the rows are virtual, so a row is scrolled into view before it can be
 * focused. Keys act only on the row itself; a button inside a row keeps its
 * own Enter and Space.
 */
export function useListKeys(deps: Deps) {
  const { rows, resetKey, listRef, keys } = deps;
  const [pos, setPos] = useState({ key: resetKey, index: 0 });
  const active = pos.key === resetKey ? clampIndex(pos.index, rows.length) : 0;
  const wantFocus = useRef(false);
  const seenKey = useRef(resetKey);
  const anchor = useRef<number | null>(null);
  const span = useRef<Span | null>(null);

  // Runs every render: a row far off screen is mounted only after the scroll.
  useLayoutEffect(() => {
    if (seenKey.current !== resetKey) {
      seenKey.current = resetKey;
      wantFocus.current = true;
      anchor.current = null;
      span.current = null;
      deps.scrollTo(0);
    }
    if (!wantFocus.current) return;
    const el = listRef.current?.querySelector<HTMLElement>(`[data-row-index="${active}"]`);
    if (!el) return;
    wantFocus.current = false;
    el.focus();
  });

  const activate = (index: number) => setPos({ key: resetKey, index });

  const goTo = (index: number) => {
    activate(index);
    wantFocus.current = true;
    deps.scrollTo(index);
  };

  const extend = (to: number) => {
    const from = anchor.current ?? active;
    anchor.current = from;
    const next = spanBetween(from, to);
    keys.pick(rowsAt(rows, leftBehind(span.current, next)), false);
    keys.pick(rowsAt(rows, indicesIn(next)), true);
    span.current = next;
    goTo(to);
  };

  const move = (e: KeyboardEvent, to: number) => {
    e.preventDefault();
    if (e.shiftKey) return extend(to);
    anchor.current = null;
    span.current = null;
    goTo(to);
  };

  const onOtherKey = (e: KeyboardEvent<HTMLElement>, row: Row, index: number) => {
    switch (e.key) {
      case "Enter":
        e.preventDefault();
        if (row.kind === "dir") deps.open(row);
        break;
      case " ":
        e.preventDefault();
        anchor.current = index;
        span.current = null;
        keys.pick([row], !deps.isPicked(row));
        break;
      case "Backspace":
        e.preventDefault();
        keys.back();
        break;
      case "Escape":
        // An open popover closes first; its own handler takes this key.
        if (e.currentTarget.closest("section")?.querySelector('[role="dialog"]')) break;
        keys.clearPicks();
        break;
    }
  };

  const onKeyDown = (e: KeyboardEvent<HTMLElement>) => {
    const el = (e.target as HTMLElement).closest<HTMLElement>("[data-row-index]");
    const index = Number(el?.dataset.rowIndex);
    const row = rows[index];
    if (!el || !row) return;
    if (e.key === "Delete") {
      // From a button inside the row too, as before.
      e.preventDefault();
      if (!row.staged || rows.some(deps.isPicked)) keys.deleteRows([row]);
      return;
    }
    if (e.target !== el || e.altKey) return;
    if (e.ctrlKey || e.metaKey) {
      if (e.key.toLowerCase() !== "a") return;
      e.preventDefault();
      keys.pick(rows, true);
      return;
    }
    const to = moveTarget(e.key, index, rows.length, deps.pageSize());
    if (to !== null) move(e, to);
    else onOtherKey(e, row, index);
  };

  return { active, activate, onKeyDown };
}
