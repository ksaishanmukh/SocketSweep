/**
 * App icons, fetched one package at a time on first sight and kept for the
 * session. Icons travel apart from rows so that rows stay small.
 *
 * A package the host had no icon for yet is remembered as a miss until
 * `retryMisses` (more icons may have arrived since). `clear` starts over for
 * a new tree, and drops the answers of fetches still in flight.
 */
export interface IconStore {
  /** A data URL, or null while there is none. */
  get: (pkg: string) => string | null;
  /** Fetches the icon unless it is known, in flight or a miss. */
  want: (pkg: string) => void;
  retryMisses: () => void;
  clear: () => void;
  subscribe: (listener: () => void) => () => void;
  /** Changes whenever anything above does, for `useSyncExternalStore`. */
  version: () => number;
}

export function createIconStore(fetchIcon: (pkg: string) => Promise<string | null>): IconStore {
  let icons = new Map<string, string>();
  const inFlight = new Set<string>();
  const misses = new Set<string>();
  const listeners = new Set<() => void>();
  let generation = 0;
  let version = 0;

  const changed = () => {
    version += 1;
    listeners.forEach((l) => l());
  };

  const settle = (pkg: string, gen: number, png: string | null) => {
    if (gen !== generation) return;
    inFlight.delete(pkg);
    if (png) icons.set(pkg, `data:image/png;base64,${png}`);
    else misses.add(pkg);
    changed();
  };

  return {
    get: (pkg) => icons.get(pkg) ?? null,
    want: (pkg) => {
      if (icons.has(pkg) || inFlight.has(pkg) || misses.has(pkg)) return;
      inFlight.add(pkg);
      const gen = generation;
      fetchIcon(pkg).then(
        (png) => settle(pkg, gen, png),
        () => settle(pkg, gen, null),
      );
    },
    retryMisses: () => {
      if (misses.size === 0) return;
      misses.clear();
      changed();
    },
    clear: () => {
      generation += 1;
      icons = new Map();
      inFlight.clear();
      misses.clear();
      changed();
    },
    subscribe: (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    version: () => version,
  };
}
