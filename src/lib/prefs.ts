import type { SizeMetric, SortKey, ViewSort } from "../api";
import { CATEGORIES, CHANGED_BUCKETS, KINDS } from "./categories";
import { DEFAULT_SORT, NO_FILTERS, type Filters } from "./viewOptions";

/**
 * What the person chose last time: size, sort and filters, kept across launches.
 * They are global, not per folder. The copy destination is chosen by the host,
 * not the GUI, so there is nothing to remember for it.
 */
export interface Prefs {
  metric: SizeMetric;
  sort: ViewSort;
  filters: Filters;
}

/** Bump on a shape change; old data then falls back to the defaults. */
const STORAGE_KEY = "socketsweep.prefs.v1";

const SORT_KEYS: SortKey[] = ["name", "size", "count", "changed"];

export const DEFAULT_PREFS: Prefs = { metric: "disk", sort: DEFAULT_SORT, filters: NO_FILTERS };

function pick<T extends string>(raw: unknown, allowed: readonly T[], fallback: T): T {
  return allowed.includes(raw as T) ? (raw as T) : fallback;
}

/** The members of `allowed` that `raw` lists; anything else is dropped. */
function pickAll<T extends string>(raw: unknown, allowed: readonly T[]): T[] {
  if (!Array.isArray(raw)) return [];
  return allowed.filter((value) => raw.includes(value));
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

/** Reads stored JSON field by field, so one bad field costs only itself. */
export function parsePrefs(text: string | null): Prefs {
  let data: unknown;
  try {
    data = text === null ? null : JSON.parse(text);
  } catch {
    return DEFAULT_PREFS;
  }
  if (!isRecord(data)) return DEFAULT_PREFS;
  const sort = isRecord(data.sort) ? data.sort : {};
  const filters = isRecord(data.filters) ? data.filters : {};
  return {
    metric: pick(data.metric, ["disk", "apparent"], DEFAULT_PREFS.metric),
    sort: {
      key: pick(sort.key, SORT_KEYS, DEFAULT_SORT.key),
      descending: typeof sort.descending === "boolean" ? sort.descending : DEFAULT_SORT.descending,
    },
    filters: {
      categories: pickAll(filters.categories, CATEGORIES),
      kinds: pickAll(filters.kinds, KINDS),
      changed: pickAll(filters.changed, CHANGED_BUCKETS),
    },
  };
}

export function serializePrefs(prefs: Prefs): string {
  return JSON.stringify(prefs);
}

/** The remembered preferences, or the defaults; storage can be missing or refuse access. */
export function readPrefs(): Prefs {
  try {
    return parsePrefs(window.localStorage.getItem(STORAGE_KEY));
  } catch {
    return DEFAULT_PREFS;
  }
}

/** Changes some of the remembered preferences, leaving the rest as stored. */
export function storePrefs(change: Partial<Prefs>) {
  try {
    window.localStorage.setItem(STORAGE_KEY, serializePrefs({ ...readPrefs(), ...change }));
  } catch {
    // Not remembering a choice is no reason to disturb anyone.
  }
}
