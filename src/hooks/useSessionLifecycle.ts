import { useCallback } from "react";
import { commands } from "../api";
import type { TreeDeps } from "./sessionTypes";
import { useConnect } from "./useConnect";
import { useHostEvents } from "./useHostEvents";
import { useScanProgress } from "./useScanProgress";
import { useBindSession, useConnectionLost, useDisconnect, useInvalidate } from "./useSessionEnds";
import { useSessionState } from "./useSessionState";
import { useStartScan } from "./useStartScan";

export type { Phase, ScanInfo } from "./sessionTypes";

interface Deps extends TreeDeps {
  setSessionId: (id: number | null) => void;
}

/**
 * connect → scan → browse → disconnect.
 *
 * Owns the current session id. Three things end a tree, and each starts by
 * clearing the session so that every reply still in flight is dropped: a new
 * scan, a disconnect and a `connection-lost` event. The *epoch* does the same
 * for the stretch before a session id exists (connecting, `startScan` in
 * flight).
 */
export function useSessionLifecycle(deps: Deps) {
  const { fail, log, notify, setTotals, reset } = deps;
  const state = useSessionState();
  const { trackingRef, setPhase, setFailure, setStatus, setScanInfo, returnToSetup } = state;

  const bind = useBindSession(deps);
  const invalidate = useInvalidate({ bind, reset, setTotals, setScanInfo, trackingRef });
  const progressDeps = { ...deps, trackingRef, setScanInfo, setPhase, setFailure };
  const onProgress = useScanProgress(progressDeps);
  const scan = useStartScan({ ...progressDeps, setStatus, bind, invalidate, onProgress });
  const connect = useConnect({ ...deps, ...state, scan });
  const disconnect = useDisconnect({ invalidate, log, notify, returnToSetup });
  const onConnectionLost = useConnectionLost({
    trackingRef,
    invalidate,
    returnToSetup,
    notify,
    log,
  });
  useHostEvents(onProgress, onConnectionLost);

  const cancelScan = useCallback(() => {
    commands.cancelScan().catch(fail);
  }, [fail]);

  return {
    phase: state.phase,
    status: state.status,
    failure: state.failure,
    scanInfo: state.scanInfo,
    previous: state.previous,
    model: state.model,
    copyDestination: state.copyDestination,
    connect,
    disconnect,
    scan,
    cancelScan,
  };
}
