import type { SortKey, ViewSort } from "../api";

export const DEFAULT_SORT: ViewSort = { key: "size", descending: true };

/**
 * The direction a column starts in when first clicked: names read A to Z,
 * everything else biggest or newest first, since that is what one looks for.
 */
const STARTS_DESCENDING: Record<SortKey, boolean> = {
  name: false,
  size: true,
  count: true,
  changed: true,
};

/** Clicking the active column flips it; clicking another starts it in its natural direction. */
export function nextSort(current: ViewSort, key: SortKey): ViewSort {
  if (current.key === key) return { key, descending: !current.descending };
  return { key, descending: STARTS_DESCENDING[key] };
}
