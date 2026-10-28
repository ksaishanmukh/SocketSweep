import { useEffect, useRef } from "react";
import { events, type ConnectionLost, type ScanProgress } from "../api";

/**
 * Subscribes to the host's two lifecycle events. Handlers go through refs so
 * the subscriptions are made once and always call the latest closure.
 */
export function useHostEvents(
  onProgress: (p: ScanProgress) => void,
  onConnectionLost: (lost: ConnectionLost) => void,
) {
  const progressRef = useRef(onProgress);
  const lostRef = useRef(onConnectionLost);
  useEffect(() => {
    progressRef.current = onProgress;
    lostRef.current = onConnectionLost;
  });
  useEffect(() => {
    const progress = events.scanProgress((p) => progressRef.current(p));
    const lost = events.connectionLost((l) => lostRef.current(l));
    return () => {
      progress.then((off) => off());
      lost.then((off) => off());
    };
  }, []);
}
