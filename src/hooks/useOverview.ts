import { useCallback, useState } from "react";
import { commands, type Overview } from "../api";
import type { SessionDeps } from "./types";

/**
 * The whole scan at a glance, for the Overview tab.
 *
 * One cheap host query, so it is simply re-run whenever the picture changes
 * (scan progress, finish, delete, size toggle, app names arriving). Failure
 * is silent: the overview is secondary and must not disturb browsing.
 */
export function useOverview({
  sessionRef,
  metricRef,
}: Pick<SessionDeps, "sessionRef" | "metricRef">) {
  const [overview, setOverview] = useState<Overview | null>(null);

  const refresh = useCallback(async () => {
    const session = sessionRef.current;
    if (session === null) return;
    try {
      const next = await commands.overview(metricRef.current);
      if (session === sessionRef.current) setOverview(next);
    } catch {
      // Secondary.
    }
  }, [metricRef, sessionRef]);

  const clear = useCallback(() => setOverview(null), []);

  return { overview, refresh, clear };
}
