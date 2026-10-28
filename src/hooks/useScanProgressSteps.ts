import { useCallback } from "react";
import type { NodeRef, ScanFinished, Totals } from "../api";
import { formatNumber } from "../lib/format";
import type { ProgressDeps } from "./sessionTypes";

const SKELETON_NOTE = "[SCAN] Top levels listed. Browsing while sizes fill in.";

/** The skeleton is listed: open the root and start browsing. */
export function useEnterBrowsing({
  log,
  nav,
  analyses,
  reset,
  trackingRef,
  setPhase,
  setFailure,
}: ProgressDeps) {
  return useCallback(
    (root: NodeRef) => {
      trackingRef.current.entered = true;
      const epoch = trackingRef.current.epoch;
      void (async () => {
        const opened = await nav.open(root);
        if (epoch !== trackingRef.current.epoch) return;
        if (!opened) {
          // `open` has already shown why.
          reset();
          setFailure("The scan started, but its results could not be opened.");
          setPhase("failed");
          return;
        }
        setPhase("browsing");
        log(SKELETON_NOTE);
        void analyses.refresh();
      })();
    },
    [analyses, log, nav, trackingRef, reset, setFailure, setPhase],
  );
}

/** The scan has ended: say how, and redraw everything with the final sizes. */
export function useScanFinished({ log, notify, nav, analyses, search, trackingRef }: ProgressDeps) {
  return useCallback(
    (totals: Totals, finished: ScanFinished) => {
      trackingRef.current.finishedSeen = true;
      const { outcome, message } = finished;
      if (outcome === "complete") {
        log(`[SCAN] ${formatNumber(totals.files)} files, ${formatNumber(totals.dirs)} folders.`);
      } else {
        log(
          `[SCAN] ${outcome === "failed" ? "Failed" : "Stopped"}${message ? `: ${message}` : "."}`,
        );
        if (outcome === "failed" && trackingRef.current.phase === "browsing") {
          notify(message ?? "The scan failed.", "error");
        }
      }
      // Sizes are final now (or as final as they will get): redraw everything.
      void analyses.refresh();
      nav.refreshTreemap();
      search.rerun();
    },
    [analyses, log, nav, notify, trackingRef, search],
  );
}
