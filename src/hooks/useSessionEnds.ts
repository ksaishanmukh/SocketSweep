import { useCallback, type MutableRefObject } from "react";
import { commands, type ConnectionLost } from "../api";
import type { ProgressDeps, SessionTracking, TreeDeps } from "./sessionTypes";
import type { Notify, SessionDeps } from "./types";

/** Points the shared session ref, and the state that mirrors it, at a session id. */
export function useBindSession({
  sessionRef,
  setSessionId,
}: Pick<SessionDeps, "sessionRef"> & { setSessionId: (id: number | null) => void }) {
  return useCallback(
    (id: number | null) => {
      sessionRef.current = id;
      setSessionId(id);
    },
    [sessionRef, setSessionId],
  );
}

/** Ends the current tree, whatever replaces it: every pending reply becomes stale. */
export function useInvalidate({
  bind,
  reset,
  setTotals,
  setScanInfo,
  trackingRef,
}: Pick<TreeDeps, "reset" | "setTotals"> &
  Pick<ProgressDeps, "setScanInfo" | "trackingRef"> & { bind: (id: number | null) => void }) {
  return useCallback(() => {
    trackingRef.current.epoch += 1;
    trackingRef.current.starting = false;
    trackingRef.current.buffered = null;
    trackingRef.current.entered = false;
    trackingRef.current.finishedSeen = false;
    trackingRef.current.root = null;
    bind(null);
    reset();
    setTotals(null);
    setScanInfo(null);
  }, [bind, trackingRef, reset, setScanInfo, setTotals]);
}

export function useDisconnect({
  invalidate,
  log,
  notify,
  returnToSetup,
}: {
  invalidate: () => void;
  log: (msg: string) => void;
  notify: Notify;
  returnToSetup: () => void;
}) {
  return useCallback(async () => {
    // First, so replies to anything still running are dropped, including the
    // scan that this call is about to cancel.
    invalidate();
    log("[SOCKET] Stopping the daemon…");
    try {
      await commands.disconnect();
      notify("Disconnected", "info");
    } catch {
      log("[SOCKET] Daemon was already stopped.");
    }
    returnToSetup();
  }, [invalidate, log, notify, returnToSetup]);
}

/** What happens when the host reports that the phone is gone. */
export function useConnectionLost({
  trackingRef,
  invalidate,
  returnToSetup,
  notify,
  log,
}: {
  trackingRef: MutableRefObject<SessionTracking>;
  invalidate: () => void;
  returnToSetup: () => void;
  notify: Notify;
  log: (msg: string) => void;
}) {
  return useCallback(
    (lost: ConnectionLost) => {
      // Already back on the setup screen (the user disconnected): nothing to undo.
      if (trackingRef.current.phase === "setup") return;
      invalidate();
      returnToSetup();
      notify(`Lost the connection to the phone. ${lost.message}`.trim(), "error");
      log(`[SOCKET] Connection to ${lost.serial} lost: ${lost.message}`);
    },
    [invalidate, log, notify, returnToSetup, trackingRef],
  );
}
