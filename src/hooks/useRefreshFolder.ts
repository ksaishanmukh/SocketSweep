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

/** Re-reads the folder that is already open: in place, its treemap alone, or its next page. */
export function useRefreshFolder({
  sessionRef,
  metricRef,
  log,
  fail,
  state,
  focus,
  loadTreemap,
}: Deps) {
  const { navSeqRef, viewRef, put, setLoadingMore } = state;

  /** Re-reads the open folder in place, e.g. after the size toggle changes the sort. */
  const reload = useCallback(async () => {
    const current = viewRef.current;
    const session = sessionRef.current;
    if (!current || session === null) return;
    const seq = navSeqRef.current;
    const limit = current.rows.length || undefined;
    focus(current.node, limit);
    try {
      const next = await commands.getView(current.node, limit, metricRef.current);
      if (session !== sessionRef.current || seq !== navSeqRef.current) return;
      if (next.node.session !== session) return;
      put(next);
    } catch (err) {
      if (session === sessionRef.current) log(`[ERROR] ${errorMessage(err)}`);
    }
    void loadTreemap(current.node, seq);
  }, [focus, loadTreemap, log, metricRef, navSeqRef, put, sessionRef, viewRef]);

  /** Re-reads only the treemap, whose tiles grow as the scan measures. */
  const refreshTreemap = useCallback(() => {
    const current = viewRef.current;
    if (current) void loadTreemap(current.node, navSeqRef.current);
  }, [loadTreemap, navSeqRef, viewRef]);

  /** Fetches the next page of a folder that has more rows than were loaded. */
  const loadMore = useCallback(async () => {
    const current = viewRef.current;
    const session = sessionRef.current;
    if (!current || session === null || current.hidden <= 0) return;
    const seq = navSeqRef.current;
    const limit = current.rows.length + VIEW_PAGE_SIZE;
    setLoadingMore(true);
    focus(current.node, limit);
    try {
      const next = await commands.getView(current.node, limit, metricRef.current);
      // If the user went elsewhere meanwhile, these rows belong to a folder
      // that is no longer open.
      if (seq === navSeqRef.current && session === sessionRef.current) put(next);
    } catch (err) {
      if (session === sessionRef.current) fail(err);
    } finally {
      if (session === sessionRef.current) setLoadingMore(false);
    }
  }, [fail, focus, metricRef, navSeqRef, put, sessionRef, setLoadingMore, viewRef]);

  return { reload, refreshTreemap, loadMore };
}
