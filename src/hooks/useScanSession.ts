import { useCallback, useRef, useState } from "react";
import type { Row, SizeMetric, Totals } from "../api";
import type { Notify, SessionDeps } from "./types";
import { useAnalyses } from "./useAnalyses";
import { useCopy } from "./useCopy";
import { useDelete } from "./useDelete";
import { useDisplayChoice, type Mode } from "./useDisplayChoice";
import { useLiveRefresh } from "./useLiveRefresh";
import { useNavigation } from "./useNavigation";
import { useSearch } from "./useSearch";
import { useSessionLifecycle } from "./useSessionLifecycle";
import { useSessionLog } from "./useSessionLog";

export { VIEW_PAGE_SIZE } from "./useNavigation";
export type { CopyState } from "./useCopy";
export type { Mode } from "./useDisplayChoice";
export type { Phase, ScanInfo } from "./useSessionLifecycle";

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
  const { logs, log, fail } = useSessionLog(notify);
  const display = useDisplayChoice();

  const deps: SessionDeps = { sessionRef, metricRef: display.metricRef, log, fail };
  const parts = useTreeParts(deps, notify, sessionId);
  const { nav, analyses, search, copier } = parts;

  const lifecycle = useSessionLifecycle({ ...deps, ...parts, notify, setSessionId, setTotals });
  const remove = useDelete({ ...deps, notify, nav, analyses, search, setTotals });
  const setMetric = useSetMetric(display.storeMetric, display.metricRef, parts);
  const reveal = useReveal(display.setMode, parts);

  useLiveRefresh({
    browsing: lifecycle.phase === "browsing",
    running: lifecycle.scanInfo?.running ?? false,
    settled: lifecycle.scanInfo?.focusSettled ?? false,
    modeRef: display.modeRef,
    refreshTreemap: nav.refreshTreemap,
    refreshAnalyses: analyses.refresh,
  });

  return {
    ...lifecycle,
    sessionId,
    totals,
    logs,
    metric: display.metric,
    setMetric,
    mode: display.mode,
    setMode: display.setMode,
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

type TreeParts = ReturnType<typeof useTreeParts>;

/** The hooks that hold nodes of the current tree, and the one call that forgets them all. */
function useTreeParts(deps: SessionDeps, notify: Notify, sessionId: number | null) {
  const nav = useNavigation(deps);
  const analyses = useAnalyses(deps);
  const search = useSearch({
    sessionRef: deps.sessionRef,
    metricRef: deps.metricRef,
    sessionId,
  });
  const copier = useCopy({ sessionRef: deps.sessionRef, log: deps.log, notify });

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

  return { nav, analyses, search, copier, reset };
}

/** Changing the size re-sorts everything the host sorted for us, so ask again. */
function useSetMetric(
  storeMetric: (next: SizeMetric) => void,
  metricRef: SessionDeps["metricRef"],
  { nav, analyses, search }: TreeParts,
) {
  const { reload } = nav;
  const { refresh: refreshAnalyses } = analyses;
  const { rerun: rerunSearch } = search;
  return useCallback(
    (next: SizeMetric) => {
      if (metricRef.current === next) return;
      storeMetric(next);
      void reload();
      void refreshAnalyses();
      rerunSearch();
    },
    [metricRef, refreshAnalyses, reload, rerunSearch, storeMetric],
  );
}

/**
 * Jump to the folder containing a cross-tree result, and back to the treemap
 * so the destination has context. Finding a huge file is only half the job.
 */
function useReveal(setMode: (mode: Mode) => void, { nav, search }: TreeParts) {
  const { open } = nav;
  const { clear: clearSearch } = search;
  return useCallback(
    async (row: Row) => {
      if (!row.parentNode) return;
      setMode("treemap");
      clearSearch();
      await open(row.parentNode);
    },
    [clearSearch, open, setMode],
  );
}
