import { useCallback, useEffect, useRef, useState } from "react";
import { errorMessage, type Row, type SizeMetric, type Totals } from "../api";
import type { Notify, SessionDeps } from "./types";
import { useAnalyses } from "./useAnalyses";
import { useCopy } from "./useCopy";
import { useDelete } from "./useDelete";
import { useNavigation } from "./useNavigation";
import { useSearch } from "./useSearch";
import { useSessionLifecycle } from "./useSessionLifecycle";

export { VIEW_PAGE_SIZE } from "./useNavigation";
export type { CopyState } from "./useCopy";
export type { Phase, ScanInfo } from "./useSessionLifecycle";

/** Which analysis the main canvas is showing. */
export type Mode = "treemap" | "largest" | "types" | "apps";

const MAX_LOG_LINES = 200;
/** How often the treemap and analyses are re-read while sizes are still arriving. */
const LIVE_REFRESH_MS = 1500;

/**
 * The whole GUI-side session: connect, scan, browse, search, delete, copy.
 *
 * A composition of smaller hooks, each owning one concern:
 *
 * - `useSessionLifecycle`: connect / scan / disconnect, the `scan-progress` and
 *   `connection-lost` events, and the session id.
 * - `useNavigation`: the open folder, its path and treemap, and `focus`.
 * - `useSearch`, `useAnalyses`: queries over the whole tree.
 * - `useDelete`, `useCopy`: actions on a node.
 *
 * What ties them together is the session id (`NodeRef.session`): anything that
 * began under an older session is dropped when it replies, because the node
 * ids in it name nothing in the current tree.
 */
export function useScanSession(notify: Notify) {
  const [sessionId, setSessionId] = useState<number | null>(null);
  const sessionRef = useRef<number | null>(null);
  const [totals, setTotals] = useState<Totals | null>(null);
  const [logs, setLogs] = useState<string[]>([]);
  const [mode, setModeState] = useState<Mode>("treemap");
  const modeRef = useRef<Mode>("treemap");

  // Disk usage is what deleting frees, so it is the default; apparent size is
  // the file-manager number.
  const [metric, setMetricState] = useState<SizeMetric>("disk");
  const metricRef = useRef<SizeMetric>("disk");

  const log = useCallback((msg: string) => {
    setLogs((prev) =>
      [...prev, `[${new Date().toLocaleTimeString()}] ${msg}`].slice(-MAX_LOG_LINES),
    );
  }, []);

  const fail = useCallback(
    (err: unknown) => {
      const message = errorMessage(err);
      notify(message, "error");
      log(`[ERROR] ${message}`);
      return message;
    },
    [log, notify],
  );

  const deps: SessionDeps = { sessionRef, metricRef, log, fail };
  const nav = useNavigation(deps);
  const analyses = useAnalyses(deps);
  const search = useSearch({ sessionRef, sessionId });
  const copier = useCopy({ sessionRef, log, notify });

  const { clear: clearNav } = nav;
  const { clear: clearAnalyses } = analyses;
  const { clear: clearSearch } = search;
  const { reset: resetCopy } = copier;
  const reset = useCallback(() => {
    clearNav();
    clearAnalyses();
    clearSearch();
    resetCopy();
  }, [clearAnalyses, clearNav, clearSearch, resetCopy]);

  const lifecycle = useSessionLifecycle({
    ...deps,
    notify,
    setSessionId,
    setTotals,
    nav,
    analyses,
    search,
    reset,
  });
  const remove = useDelete({ ...deps, notify, nav, analyses, search, setTotals });

  const setMode = useCallback((next: Mode) => {
    modeRef.current = next;
    setModeState(next);
  }, []);

  /** Changing the size re-sorts everything the host sorted for us, so ask again. */
  const { reload, refreshTreemap } = nav;
  const { refresh: refreshAnalyses } = analyses;
  const { rerun: rerunSearch } = search;
  const setMetric = useCallback(
    (next: SizeMetric) => {
      if (metricRef.current === next) return;
      metricRef.current = next;
      setMetricState(next);
      void reload();
      void refreshAnalyses();
      rerunSearch();
    },
    [refreshAnalyses, reload, rerunSearch],
  );

  // Sizes keep climbing while the scan runs, and the treemap and analyses are
  // snapshots: re-read them on a slow tick. The open folder's rows need no
  // tick, because the host pushes them with every `scan-progress`.
  const browsing = lifecycle.phase === "browsing";
  const running = lifecycle.scanInfo?.running ?? false;
  useEffect(() => {
    if (!browsing || !running) return;
    const timer = setInterval(() => {
      refreshTreemap();
      if (modeRef.current !== "treemap") void refreshAnalyses();
    }, LIVE_REFRESH_MS);
    return () => clearInterval(timer);
  }, [browsing, running, refreshAnalyses, refreshTreemap]);

  // A folder the phone has just finished measuring is worth redrawing at once.
  const settled = lifecycle.scanInfo?.focusSettled ?? false;
  useEffect(() => {
    if (browsing && settled) refreshTreemap();
  }, [browsing, settled, refreshTreemap]);

  const { open } = nav;
  /**
   * Jump to the folder containing a cross-tree result, and back to the treemap
   * so the destination has context. Finding a huge file is only half the job.
   */
  const reveal = useCallback(
    async (row: Row) => {
      if (!row.parentNode) return;
      setMode("treemap");
      clearSearch();
      await open(row.parentNode);
    },
    [clearSearch, open, setMode],
  );

  return {
    ...lifecycle,
    sessionId,
    totals,
    logs,
    metric,
    setMetric,
    mode,
    setMode,
    view: nav.view,
    crumbs: nav.crumbs,
    treemap: nav.treemap,
    loadingMore: nav.loadingMore,
    open: nav.open,
    loadMore: nav.loadMore,
    reveal,
    query: search.query,
    setQuery: search.setQuery,
    searchResults: search.results,
    largest: analyses.largest,
    types: analyses.types,
    apps: analyses.apps,
    remove,
    copy: copier.copy,
    startCopy: copier.start,
    cancelCopy: copier.cancel,
  };
}

export type ScanSession = ReturnType<typeof useScanSession>;
