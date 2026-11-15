import { useCallback, useRef, useState } from "react";
import { commands, type AppsUpdated, type Totals } from "../api";
import { createIconStore } from "../lib/iconStore";
import type { Notify, SessionDeps } from "./types";
import { useAppsUpdated } from "./useAppsUpdated";
import { useCopy } from "./useCopy";
import { useDelete } from "./useDelete";
import { useDisplayChoice } from "./useDisplayChoice";
import { useLiveRefresh } from "./useLiveRefresh";
import { useNavigation } from "./useNavigation";
import { useOverview } from "./useOverview";
import { useSearch } from "./useSearch";
import { useSelection } from "./useSelection";
import { useSessionLifecycle } from "./useSessionLifecycle";
import { useSessionLog } from "./useSessionLog";
import { useBrowseActions, useViewControls } from "./useViewControls";
import { useViewOptions, type ViewOptionsState } from "./useViewOptions";

export { VIEW_PAGE_SIZE } from "./useNavigation";
export type { CopyState } from "./useCopy";
export type { Mode } from "./useDisplayChoice";
export type { Phase, ScanInfo } from "./useSessionLifecycle";

/**
 * The whole GUI-side session: connect, scan, browse, search, select, delete,
 * copy.
 *
 * A composition of smaller hooks, each owning one concern:
 *
 * - `useSessionLifecycle`: connect / scan / disconnect, the `scan-progress` and
 *   `connection-lost` events, and the session id.
 * - `useNavigation`: the open folder, its path, map and history, and `focus`.
 * - `useViewOptions`: the list's sort, filters and search box.
 * - `useSearch`, `useOverview`: queries over the whole tree.
 * - `useSelection`, `useDelete`, `useCopy`: batch actions on picked nodes.
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
  const options = useViewOptions();

  const deps: SessionDeps = {
    sessionRef,
    metricRef: display.metricRef,
    optionsRef: options.optionsRef,
    log,
    fail,
  };
  const parts = useTreeParts(deps, notify, sessionId, options);
  const { nav, overview, search, copier, selection } = parts;

  const lifecycle = useSessionLifecycle({ ...deps, ...parts, notify, setSessionId, setTotals });
  const remove = useDelete({ ...deps, notify, nav, overview, search, selection, setTotals });
  const controls = useViewControls({ options, parts, storeMetric: display.storeMetric, deps });
  const actions = useBrowseActions({ display, options, parts, rootNode: lifecycle.rootNode });
  useAppsUpdated(useOnAppsUpdated(sessionRef, parts));

  useLiveRefresh({
    browsing: lifecycle.phase === "browsing",
    running: lifecycle.scanInfo?.running ?? false,
    settled: lifecycle.scanInfo?.focusSettled ?? false,
    modeRef: display.modeRef,
    refreshTreemap: nav.refreshTreemap,
    refreshOverview: overview.refresh,
  });

  return {
    ...lifecycle,
    sessionId,
    totals,
    logs,
    metric: display.metric,
    mode: display.mode,
    nav,
    overview: overview.overview,
    searchResults: search.results,
    options,
    controls,
    actions,
    selection,
    icons: parts.icons,
    remove,
    copy: copier.copy,
    startCopy: copier.start,
    cancelCopy: copier.cancel,
  };
}

export type ScanSession = ReturnType<typeof useScanSession>;

export type TreeParts = ReturnType<typeof useTreeParts>;

/** The hooks that hold nodes of the current tree, and the one call that forgets them all. */
function useTreeParts(
  deps: SessionDeps,
  notify: Notify,
  sessionId: number | null,
  options: ViewOptionsState,
) {
  const nav = useNavigation(deps);
  const overview = useOverview(deps);
  const search = useSearch({
    ...deps,
    sessionId,
    query: options.everywhere ? options.text : "",
  });
  const copier = useCopy({ sessionRef: deps.sessionRef, log: deps.log, notify });
  const selection = useSelection(sessionId);
  const [icons] = useState(() => createIconStore((pkg) => commands.appIcon(pkg)));

  const { clear: clearNav } = nav;
  const { clear: clearOverview } = overview;
  const { clear: clearSearch } = search;
  const { reset: resetCopy } = copier;
  const { clear: clearSelection } = selection;
  const { clearText } = options;
  const reset = useCallback(() => {
    clearNav();
    clearOverview();
    clearSearch();
    clearText();
    resetCopy();
    clearSelection();
    icons.clear();
  }, [clearNav, clearOverview, clearSearch, clearSelection, clearText, icons, resetCopy]);

  return { nav, overview, search, copier, selection, icons, reset };
}

/**
 * Names and icons arrived: everything fetched before lacks them, so fetch it
 * again. Rows already on screen keep their place; only their labels change.
 */
function useOnAppsUpdated(sessionRef: SessionDeps["sessionRef"], parts: TreeParts) {
  const { nav, overview, search, icons } = parts;
  return useCallback(
    (update: AppsUpdated) => {
      if (update.session !== sessionRef.current) return;
      icons.retryMisses();
      void overview.refresh();
      void nav.reload();
      search.rerun();
    },
    [icons, nav, overview, search, sessionRef],
  );
}
