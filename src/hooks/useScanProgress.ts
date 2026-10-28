import { useCallback } from "react";
import type { NodeRef, ScanProgress } from "../api";
import type { ProgressDeps, ScanInfo } from "./sessionTypes";
import { useEnterBrowsing, useScanFinished } from "./useScanProgressSteps";

/**
 * Handles each `scan-progress` event of the current session: totals, the scan's
 * description, entering the browser once the skeleton is listed, live views
 * and the scan's end. Events for another session are dropped.
 */
export function useScanProgress(deps: ProgressDeps) {
  const { sessionRef, setTotals, setScanInfo, nav, reset, trackingRef, setPhase, setFailure } =
    deps;
  const enter = useEnterBrowsing(deps);
  const finish = useScanFinished(deps);

  /** Decides, from the first useful event, whether there is anything to browse. */
  const startBrowsing = useCallback(
    (p: ScanProgress, root: NodeRef) => {
      if (p.skeleton || p.finished?.outcome === "complete") {
        enter(root);
      } else if (p.finished) {
        // Ended before anything was listed: nothing to browse.
        const why = p.finished.message ?? "The scan was stopped before anything was listed.";
        reset();
        setFailure(why);
        setPhase("failed");
      }
    },
    [enter, reset, setFailure, setPhase],
  );

  return useCallback(
    (p: ScanProgress) => {
      const session = sessionRef.current;
      if (session === null) {
        // The first events can beat `startScan`'s own reply.
        if (trackingRef.current.starting) trackingRef.current.buffered = p;
        return;
      }
      if (p.session !== session) return;

      setTotals(p.totals);
      setScanInfo(scanInfoOf(p));

      const root = trackingRef.current.root;
      if (!trackingRef.current.entered && root) startBrowsing(p, root);
      if (trackingRef.current.entered && p.view) nav.applyLiveView(p.view);

      if (p.finished && !trackingRef.current.finishedSeen) finish(p.totals, p.finished);
    },
    [finish, nav, trackingRef, sessionRef, setScanInfo, setTotals, startBrowsing],
  );
}

function scanInfoOf(p: ScanProgress): ScanInfo {
  return {
    phase: p.phase,
    running: p.finished === null,
    listed: p.listed,
    known: p.known,
    elapsedMs: p.finished?.elapsedMs ?? p.elapsedMs,
    focusSettled: p.focus?.settled ?? false,
    outcome: p.finished?.outcome ?? null,
    message: p.finished?.message ?? null,
  };
}
