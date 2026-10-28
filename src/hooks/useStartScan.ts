import { useCallback } from "react";
import { commands, type ScanProgress } from "../api";
import type { ProgressDeps } from "./sessionTypes";

interface Deps extends Pick<ProgressDeps, "log" | "fail" | "trackingRef" | "setScanInfo"> {
  setPhase: ProgressDeps["setPhase"];
  setFailure: ProgressDeps["setFailure"];
  setStatus: (status: string) => void;
  bind: (id: number | null) => void;
  invalidate: () => void;
  onProgress: (p: ScanProgress) => void;
}

/** (Re)starts the scan on the connected phone. */
export function useStartScan({
  log,
  fail,
  trackingRef,
  setScanInfo,
  setPhase,
  setFailure,
  setStatus,
  bind,
  invalidate,
  onProgress,
}: Deps) {
  return useCallback(async () => {
    invalidate();
    const epoch = trackingRef.current.epoch;
    setFailure(null);
    setStatus("Starting scan…");
    setPhase("connecting");
    trackingRef.current.starting = true;
    log("[SCAN] Starting…");
    try {
      const started = await commands.startScan();
      if (epoch !== trackingRef.current.epoch) return;
      trackingRef.current.starting = false;
      trackingRef.current.root = started.root;
      bind(started.session);
      setScanInfo({
        phase: "skeleton",
        running: true,
        listed: 0,
        known: 1,
        elapsedMs: 0,
        focusSettled: false,
        outcome: null,
        message: null,
      });
      const early = trackingRef.current.buffered;
      trackingRef.current.buffered = null;
      if (early && early.session === started.session) onProgress(early);
    } catch (err) {
      if (epoch !== trackingRef.current.epoch) return;
      trackingRef.current.starting = false;
      const message = fail(err);
      setFailure(message);
      setPhase("failed");
    }
  }, [
    bind,
    fail,
    invalidate,
    log,
    onProgress,
    trackingRef,
    setFailure,
    setPhase,
    setScanInfo,
    setStatus,
  ]);
}
