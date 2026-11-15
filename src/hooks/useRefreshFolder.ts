import { useCallback } from "react";
import { commands, errorMessage, type NodeRef } from "../api";
import type { SessionDeps } from "./types";
import type { NavigationState } from "./useNavigationState";

/** How many more rows "Load more" asks the host for each time. */
export const VIEW_PAGE_SIZE = 500;

interface Deps extends SessionDeps {
  state: NavigationState;
  focus: (node: NodeRef, limit?: number) => void;
  loadTreemap: (node: NodeRef, seq: number) => Promise<void>;
}

/** Re-reads the folder that is already open: in place, its map alone, or its next page. */
export function useRefreshFolder(deps: Deps) {
  const { state, loadTreemap } = deps;
  const { navSeqRef, viewRef } = state;
  const reload = useReload(deps);
  const loadMore = useLoadMore(deps);

  /** Re-reads only the map, whose tiles grow as the scan measures. */
  const refreshTreemap = useCallback(() => {
    const current = viewRef.current;
    if (current) void loadTreemap(current.node, navSeqRef.current);
  }, [loadTreemap, navSeqRef, viewRef]);

  return { reload, refreshTreemap, loadMore };
}

/**
 * Re-reads the open folder in place: after the size toggle, a new sort or
 * filter, or app names arriving. A changed filter can match fewer rows than
 * were loaded, so the row count asked for is kept rather than the result's.
 */
function useReload({ sessionRef, metricRef, optionsRef, log, state, focus, loadTreemap }: Deps) {
  const { navSeqRef, viewRef, put } = state;
  return useCallback(async () => {
    const current = viewRef.current;
    const session = sessionRef.current;
    if (!current || session === null) return;
    const seq = navSeqRef.current;
    const limit = current.rows.length > VIEW_PAGE_SIZE ? current.rows.length : undefined;
    focus(current.node, limit);
    const { sort, filter } = optionsRef.current;
    try {
      const next = await commands.getView(current.node, limit, metricRef.current, sort, filter);
      if (session !== sessionRef.current || seq !== navSeqRef.current) return;
      if (next.node.session !== session) return;
      put(next);
    } catch (err) {
      if (session === sessionRef.current) log(`[ERROR] ${errorMessage(err)}`);
    }
    void loadTreemap(current.node, seq);
  }, [focus, loadTreemap, log, metricRef, navSeqRef, optionsRef, put, sessionRef, viewRef]);
}

/** Fetches the next page of a folder that has more rows than were loaded. */
function useLoadMore({ sessionRef, metricRef, optionsRef, fail, state, focus }: Deps) {
  const { navSeqRef, viewRef, put, setLoadingMore } = state;
  return useCallback(async () => {
    const current = viewRef.current;
    const session = sessionRef.current;
    if (!current || session === null || current.hidden <= 0) return;
    const seq = navSeqRef.current;
    const limit = current.rows.length + VIEW_PAGE_SIZE;
    setLoadingMore(true);
    focus(current.node, limit);
    const { sort, filter } = optionsRef.current;
    try {
      const next = await commands.getView(current.node, limit, metricRef.current, sort, filter);
      // If the user went elsewhere meanwhile, these rows belong to a folder
      // that is no longer open.
      if (seq === navSeqRef.current && session === sessionRef.current) put(next);
    } catch (err) {
      if (session === sessionRef.current) fail(err);
    } finally {
      if (session === sessionRef.current) setLoadingMore(false);
    }
  }, [fail, focus, metricRef, navSeqRef, optionsRef, put, sessionRef, setLoadingMore, viewRef]);
}
