import { useCallback } from "react";
import { canGoBack, canGoForward, currentEntry, step } from "../lib/history";
import type { NodeRef } from "../api";
import type { SessionDeps } from "./types";
import { useFocus, useLoadTreemap } from "./useFocusAndTreemap";
import { useNavigationState, type NavigationState } from "./useNavigationState";
import { useOpenFolder } from "./useOpenFolder";
import { useRefreshFolder } from "./useRefreshFolder";

export { VIEW_PAGE_SIZE } from "./useRefreshFolder";

/**
 * Which folder is open, and everything that hangs off it: its rows, the path
 * to it, its map and the back/forward history.
 *
 * Every way of opening a folder (list row, breadcrumb, map tile, search
 * reveal, an app on the overview) comes through `open`, which tells the host
 * with `focus` so it measures that folder first. Replies are guarded twice:
 * by the session they began under (a rescan or reconnect replaces the tree)
 * and by a navigation counter (the slower of two quick clicks must not win).
 */
export function useNavigation(deps: SessionDeps) {
  const state = useNavigationState();
  const focus = useFocus(deps);
  const loadTreemap = useLoadTreemap(deps, state);
  const opening = useOpenFolder({ ...deps, state, focus, loadTreemap });
  const refreshing = useRefreshFolder({ ...deps, state, focus, loadTreemap });
  const stepping = useHistorySteps(state, opening.open);

  return {
    view: state.view,
    crumbs: state.crumbs,
    treemap: state.treemap,
    loadingMore: state.loadingMore,
    canBack: canGoBack(state.history),
    canForward: canGoForward(state.history),
    ...opening,
    ...refreshing,
    ...stepping,
    currentView: state.currentView,
    clear: state.clear,
  };
}

/** Back and forward: the history moves only once the folder it points at has opened. */
function useHistorySteps(
  { historyRef, setHistory }: NavigationState,
  open: (node: NodeRef, record?: boolean) => Promise<boolean>,
) {
  const go = useCallback(
    async (delta: -1 | 1) => {
      const before = historyRef.current;
      const next = step(before, delta);
      const target = currentEntry(next);
      if (next === before || !target) return;
      if ((await open(target, false)) && historyRef.current === before) setHistory(next);
    },
    [historyRef, open, setHistory],
  );
  const back = useCallback(() => go(-1), [go]);
  const forward = useCallback(() => go(1), [go]);
  return { back, forward };
}
