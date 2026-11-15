import { useEffect, useRef } from "react";
import { events, type AppsUpdated } from "../api";

/**
 * Subscribes to `apps-updated`, which says the phone has reported app names
 * or icons. The handler goes through a ref so the subscription is made once
 * and always calls the latest closure.
 */
export function useAppsUpdated(onUpdated: (update: AppsUpdated) => void) {
  const handlerRef = useRef(onUpdated);
  useEffect(() => {
    handlerRef.current = onUpdated;
  });
  useEffect(() => {
    const pending = events.appsUpdated((u) => handlerRef.current(u));
    return () => {
      pending.then((off) => off());
    };
  }, []);
}
