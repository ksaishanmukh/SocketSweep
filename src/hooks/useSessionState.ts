import { useCallback, useRef, useState } from "react";
import type { ScanRecord } from "../api";
import type { Phase, ScanInfo, SessionTracking } from "./sessionTypes";

/** The lifecycle's state: what the screens show, and the tracking ref async code compares against. */
export function useSessionState() {
  const [phase, setPhaseState] = useState<Phase>("setup");
  const [status, setStatus] = useState("");
  const [failure, setFailure] = useState<string | null>(null);
  const [scanInfo, setScanInfo] = useState<ScanInfo | null>(null);
  const [previous, setPrevious] = useState<ScanRecord | null>(null);
  const [model, setModel] = useState<string | null>(null);
  const [copyDestination, setCopyDestination] = useState<string | null>(null);

  const trackingRef = useRef<SessionTracking>(initialTracking());

  const setPhase = useCallback(
    (next: Phase) => {
      trackingRef.current.phase = next;
      setPhaseState(next);
    },
    [trackingRef],
  );

  /** Back to the setup screen with nothing remembered about the device. */
  const returnToSetup = useCallback(() => {
    setFailure(null);
    setPrevious(null);
    setModel(null);
    setCopyDestination(null);
    setPhase("setup");
  }, [setPhase]);

  return {
    phase,
    status,
    failure,
    scanInfo,
    previous,
    model,
    copyDestination,
    trackingRef,
    setPhase,
    setStatus,
    setFailure,
    setScanInfo,
    setPrevious,
    setModel,
    setCopyDestination,
    returnToSetup,
  };
}

function initialTracking(): SessionTracking {
  return {
    phase: "setup",
    epoch: 0,
    root: null,
    starting: false,
    buffered: null,
    entered: false,
    finishedSeen: false,
  };
}
