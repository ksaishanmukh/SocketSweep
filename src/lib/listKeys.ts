/** An inclusive span of row indices, low to high. */
export type Span = readonly [number, number];

/** Where a movement key takes the active row, or null if the key is not one. */
export function moveTarget(
  key: string,
  active: number,
  count: number,
  page: number,
): number | null {
  const step = Math.max(1, page);
  const to = new Map([
    ["ArrowUp", active - 1],
    ["ArrowDown", active + 1],
    ["Home", 0],
    ["End", count - 1],
    ["PageUp", active - step],
    ["PageDown", active + step],
  ]).get(key);
  return to === undefined ? null : clampIndex(to, count);
}

/** The nearest row that exists; 0 when there are none. */
export function clampIndex(index: number, count: number): number {
  return Math.max(0, Math.min(index, count - 1));
}

/** The rows between two ends, whichever order they come in. */
export function spanBetween(a: number, b: number): Span {
  return a <= b ? [a, b] : [b, a];
}

export function indicesIn([lo, hi]: Span): number[] {
  return Array.from({ length: hi - lo + 1 }, (_, i) => lo + i);
}

/** What a shrinking shift-selection lets go of: rows in the old span, not the new one. */
export function leftBehind(prev: Span | null, next: Span): number[] {
  if (!prev) return [];
  return indicesIn(prev).filter((i) => i < next[0] || i > next[1]);
}
