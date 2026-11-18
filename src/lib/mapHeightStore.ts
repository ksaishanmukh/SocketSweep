import { MAP_DEFAULT_HEIGHT, clampHeight } from "./mapGrip";

const STORAGE_KEY = "socketsweep.mapHeight";

/** The remembered height, or the default; storage can be missing or refuse access. */
export function readStoredHeight(): number {
  try {
    const raw = window.localStorage.getItem(STORAGE_KEY);
    const value = raw === null ? NaN : Number(raw);
    return Number.isFinite(value) ? clampHeight(value) : MAP_DEFAULT_HEIGHT;
  } catch {
    return MAP_DEFAULT_HEIGHT;
  }
}

export function storeHeight(height: number) {
  try {
    window.localStorage.setItem(STORAGE_KEY, String(clampHeight(height)));
  } catch {
    // Not remembering the height is no reason to disturb anyone.
  }
}
