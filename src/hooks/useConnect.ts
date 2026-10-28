import { useCallback, type MutableRefObject } from "react";
import { commands, type ScanRecord } from "../api";
import type { Phase, SessionTracking } from "./sessionTypes";
import type { SessionDeps } from "./types";

interface Deps extends Pick<SessionDeps, "log" | "fail"> {
  trackingRef: MutableRefObject<SessionTracking>;
  scan: () => Promise<void>;
  setPhase: (phase: Phase) => void;
  setFailure: (message: string | null) => void;
  setStatus: (status: string) => void;
  setPrevious: (previous: ScanRecord | null) => void;
  setModel: (model: string | null) => void;
  setCopyDestination: (destination: string | null) => void;
}

/** `serial` picks a specific device; omitted, the host chooses the only usable one. */
export function useConnect({
  log,
  fail,
  trackingRef,
  scan,
  setPhase,
  setFailure,
  setStatus,
  setPrevious,
  setModel,
  setCopyDestination,
}: Deps) {
  return useCallback(
    async (serial?: string) => {
      const epoch = ++trackingRef.current.epoch;
      setFailure(null);
      setStatus("Connecting…");
      setPhase("connecting");
      try {
        log("[ADB] Looking for a device…");
        const info = await commands.connect(serial);
        if (epoch !== trackingRef.current.epoch) return;
        // Captured once per connection, so a rescan still compares against the
        // previous session rather than against the scan a moment ago.
        setPrevious(info.previous);
        setModel(info.model);
        setCopyDestination(info.copyDestination);
        log(`[ADB] ${info.model} (${info.serial}).`);
        await scan();
      } catch (err) {
        if (epoch !== trackingRef.current.epoch) return;
        fail(err);
        setPhase("setup");
      }
    },
    [
      fail,
      log,
      scan,
      setCopyDestination,
      setFailure,
      setModel,
      setPhase,
      setPrevious,
      setStatus,
      trackingRef,
    ],
  );
}
