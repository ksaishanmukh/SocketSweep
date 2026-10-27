import { useCallback, useState } from "react";
import { commands, type AppUsage, type Row, type TypeGroup } from "../api";
import type { SessionDeps } from "./types";

/**
 * The cross-tree analyses: largest files, storage by type, storage by app.
 *
 * Cheap host queries, so they are simply re-run whenever the picture changes
 * (scan progress, finish, delete, size toggle). Failure is silent: these are
 * secondary views and must not disturb browsing.
 */
export function useAnalyses({
  sessionRef,
  metricRef,
}: Pick<SessionDeps, "sessionRef" | "metricRef">) {
  const [largest, setLargest] = useState<Row[]>([]);
  const [types, setTypes] = useState<TypeGroup[]>([]);
  const [apps, setApps] = useState<AppUsage[]>([]);

  const refresh = useCallback(async () => {
    const session = sessionRef.current;
    if (session === null) return;
    const by = metricRef.current;
    try {
      const [top, groups, owners] = await Promise.all([
        commands.largestFiles(100, by),
        commands.typeBreakdown(by),
        commands.appBreakdown(50, by),
      ]);
      if (session !== sessionRef.current) return;
      setLargest(top);
      setTypes(groups);
      setApps(owners);
    } catch {
      // Secondary views.
    }
  }, [metricRef, sessionRef]);

  const clear = useCallback(() => {
    setLargest([]);
    setTypes([]);
    setApps([]);
  }, []);

  return { largest, types, apps, refresh, clear };
}
