import type { SessionDeps } from "./types";
import { useFocus, useLoadTreemap } from "./useFocusAndTreemap";
import { useNavigationState } from "./useNavigationState";
import { useOpenFolder } from "./useOpenFolder";
import { useRefreshFolder } from "./useRefreshFolder";

export { VIEW_PAGE_SIZE } from "./useRefreshFolder";

/**
 * Which folder is open, and everything that hangs off it: its rows, the path
 * to it and its treemap.
 *
 * Every way of opening a folder (list row, breadcrumb, treemap tile, search
 * reveal, app breakdown) comes through `open`, which tells the host with
 * `focus` so it measures that folder first. Replies are guarded twice: by the
 * session they began under (a rescan or reconnect replaces the tree) and by a
 * navigation counter (the slower of two quick clicks must not win).
 */
export function useNavigation(deps: SessionDeps) {
  const state = useNavigationState();
  const focus = useFocus(deps);
  const loadTreemap = useLoadTreemap(deps, state);
  const opening = useOpenFolder({ ...deps, state, focus, loadTreemap });
  const refreshing = useRefreshFolder({ ...deps, state, focus, loadTreemap });

  return {
    view: state.view,
    crumbs: state.crumbs,
    treemap: state.treemap,
    loadingMore: state.loadingMore,
    ...opening,
    ...refreshing,
    currentView: state.currentView,
    clear: state.clear,
  };
}
