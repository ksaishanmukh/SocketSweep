/**
 * The map's height, set by dragging the grip below it.
 *
 * Pure so the snapping rules are testable without pointer events: drag
 * follows the pointer, a release near the top collapses the map to nothing
 * but the grip, and a click on a collapsed grip brings it back.
 */

export const MAP_DEFAULT_HEIGHT = 260;
export const MAP_MAX_HEIGHT = 560;
/** Released shorter than this, the map is too small to read, so it goes away. */
export const MAP_COLLAPSE_BELOW = 90;
/** Pointer travel under this is a click, not a drag. */
export const DRAG_THRESHOLD = 3;
/** How far one arrow key press moves the grip. */
export const KEY_STEP = 40;

const STORAGE_KEY = "socketsweep.mapHeight";

export interface Drag {
  startHeight: number;
  /** Where the pointer has taken the height so far. */
  height: number;
  moved: boolean;
}

function clamp(height: number): number {
  return Math.round(Math.min(MAP_MAX_HEIGHT, Math.max(0, height)));
}

export function startDrag(height: number): Drag {
  return { startHeight: height, height, moved: false };
}

/** The height while the pointer is `dy` pixels below where it went down. */
export function moveDrag(drag: Drag, dy: number): Drag {
  return {
    startHeight: drag.startHeight,
    height: clamp(drag.startHeight + dy),
    moved: drag.moved || Math.abs(dy) >= DRAG_THRESHOLD,
  };
}

/** Where the map settles when the pointer is let go. */
export function releaseDrag(drag: Drag, lastOpen: number): number {
  if (!drag.moved) return drag.startHeight === 0 ? restoredHeight(lastOpen) : drag.startHeight;
  return drag.height < MAP_COLLAPSE_BELOW ? 0 : drag.height;
}

/** The height a collapsed map comes back at: the last one it was open at, if that was usable. */
export function restoredHeight(lastOpen: number): number {
  return lastOpen >= MAP_COLLAPSE_BELOW ? clamp(lastOpen) : MAP_DEFAULT_HEIGHT;
}

/** The grip's keyboard equivalent of dragging, or null for a key it does not handle. */
export function keyHeight(key: string, height: number, lastOpen: number): number | null {
  switch (key) {
    case "ArrowUp": {
      const next = height - KEY_STEP;
      return next < MAP_COLLAPSE_BELOW ? 0 : next;
    }
    case "ArrowDown":
      return height === 0 ? restoredHeight(lastOpen) : clamp(height + KEY_STEP);
    case "Enter":
    case " ":
      return height === 0 ? restoredHeight(lastOpen) : 0;
    default:
      return null;
  }
}

/** The remembered height, or the default; storage can be missing or refuse access. */
export function readStoredHeight(): number {
  try {
    const raw = window.localStorage.getItem(STORAGE_KEY);
    const value = raw === null ? NaN : Number(raw);
    return Number.isFinite(value) ? clamp(value) : MAP_DEFAULT_HEIGHT;
  } catch {
    return MAP_DEFAULT_HEIGHT;
  }
}

export function storeHeight(height: number) {
  try {
    window.localStorage.setItem(STORAGE_KEY, String(clamp(height)));
  } catch {
    // Not remembering the height is no reason to disturb anyone.
  }
}
