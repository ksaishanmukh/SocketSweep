import { useEffect, type MutableRefObject } from "react";
import type { Mode } from "./useDisplayChoice";

/** How often the treemap and analyses are re-read while sizes are still arriving. */
const LIVE_REFRESH_MS = 1500;

interface Options {
  browsing: boolean;
  running: boolean;
  /** The focused folder has just been fully measured. */
  settled: boolean;
  modeRef: MutableRefObject<Mode>;
  refreshTreemap: () => void;
  refreshAnalyses: () => Promise<void>;
}

/**
 * Keeps the snapshots fresh while a scan runs.
 *
 * Sizes keep climbing while the scan runs, and the treemap and analyses are
 * snapshots: re-read them on a slow tick. The open folder's rows need no
 * tick, because the host pushes them with every `scan-progress`.
 */
export function useLiveRefresh({
  browsing,
  running,
  settled,
  modeRef,
  refreshTreemap,
  refreshAnalyses,
}: Options) {
  useEffect(() => {
    if (!browsing || !running) return;
    const timer = setInterval(() => {
      refreshTreemap();
      if (modeRef.current !== "treemap") void refreshAnalyses();
    }, LIVE_REFRESH_MS);
    return () => clearInterval(timer);
  }, [browsing, running, modeRef, refreshAnalyses, refreshTreemap]);

  // A folder the phone has just finished measuring is worth redrawing at once.
  useEffect(() => {
    if (browsing && settled) refreshTreemap();
  }, [browsing, settled, refreshTreemap]);
}
