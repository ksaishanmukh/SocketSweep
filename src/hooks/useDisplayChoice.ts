import { useCallback, useRef, useState } from "react";
import type { SizeMetric } from "../api";

/** Which analysis the main canvas is showing. */
export type Mode = "treemap" | "largest" | "types" | "apps";

/**
 * What the user chose to look at: the analysis (`mode`) and the size
 * (`metric`). Both are mirrored in refs for async code that must not read a
 * stale closure.
 */
export function useDisplayChoice() {
  const [mode, setModeState] = useState<Mode>("treemap");
  const modeRef = useRef<Mode>("treemap");

  // Disk usage is what deleting frees, so it is the default; apparent size is
  // the file-manager number.
  const [metric, setMetricState] = useState<SizeMetric>("disk");
  const metricRef = useRef<SizeMetric>("disk");

  const setMode = useCallback((next: Mode) => {
    modeRef.current = next;
    setModeState(next);
  }, []);

  const storeMetric = useCallback((next: SizeMetric) => {
    metricRef.current = next;
    setMetricState(next);
  }, []);

  return { mode, modeRef, setMode, metric, metricRef, storeMetric };
}
