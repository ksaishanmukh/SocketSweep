import { useCallback, useRef, useState } from "react";
import type { SizeMetric } from "../api";
import { readPrefs, storePrefs } from "../lib/prefs";

/** Which tab the result screen shows. */
export type Mode = "overview" | "explorer";

/**
 * What the user chose to look at: the tab (`mode`) and the size (`metric`).
 * Both are mirrored in refs for async code that must not read a stale closure.
 */
export function useDisplayChoice() {
  const [mode, setModeState] = useState<Mode>("overview");
  const modeRef = useRef<Mode>("overview");

  // Disk usage is what deleting frees, so it is the default; apparent size is
  // the file-manager number. The last choice is remembered.
  const [initialMetric] = useState(() => readPrefs().metric);
  const [metric, setMetricState] = useState<SizeMetric>(initialMetric);
  const metricRef = useRef<SizeMetric>(initialMetric);

  const setMode = useCallback((next: Mode) => {
    modeRef.current = next;
    setModeState(next);
  }, []);

  const storeMetric = useCallback((next: SizeMetric) => {
    metricRef.current = next;
    setMetricState(next);
    storePrefs({ metric: next });
  }, []);

  return { mode, modeRef, setMode, metric, metricRef, storeMetric };
}
