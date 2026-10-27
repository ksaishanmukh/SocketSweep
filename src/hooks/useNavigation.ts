import { useCallback, useRef, useState } from "react";
import {
  commands,
  errorMessage,
  type Crumb,
  type NodeRef,
  type TreemapNode,
  type View,
} from "../api";
import { sameNode } from "../lib/size";
import type { SessionDeps } from "./types";

/** How many more rows "Load more" asks the host for each time. */
export const VIEW_PAGE_SIZE = 500;

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
export function useNavigation({ sessionRef, metricRef, log, fail }: SessionDeps) {
  const [view, setView] = useState<View | null>(null);
  const [crumbs, setCrumbs] = useState<Crumb[]>([]);
  const [treemap, setTreemap] = useState<TreemapNode | null>(null);
  const [loadingMore, setLoadingMore] = useState(false);

  /** Mirrors `view` for async code that must not read a stale closure. */
  const viewRef = useRef<View | null>(null);
  const navSeq = useRef(0);

  const put = useCallback((next: View | null) => {
    viewRef.current = next;
    setView(next);
  }, []);

  const clear = useCallback(() => {
    navSeq.current += 1;
    put(null);
    setCrumbs([]);
    setTreemap(null);
    setLoadingMore(false);
  }, [put]);

  /** Tells the host which folder the user is looking at. Failure only costs speed. */
  const focus = useCallback(
    (node: NodeRef) => {
      commands.focus(node).catch((err) => log(`[FOCUS] ${errorMessage(err)}`));
    },
    [log],
  );

  const loadTreemap = useCallback(
    async (node: NodeRef, seq: number) => {
      const session = sessionRef.current;
      const current = () => seq === navSeq.current && session === sessionRef.current;
      try {
        const next = await commands.getTreemap(node, 2, metricRef.current);
        if (current() && next.node.session === session) setTreemap(next);
      } catch {
        // A missing treemap is not worth interrupting the user over.
        if (current()) setTreemap(null);
      }
    },
    [metricRef, sessionRef],
  );

  /**
   * Opens a folder. Resolves false only when the host could not show it; a
   * navigation overtaken by a newer one counts as fine, since the newer one
   * reports for itself.
   */
  const open = useCallback(
    async (node: NodeRef): Promise<boolean> => {
      const session = sessionRef.current;
      // A node from another session names nothing in this tree.
      if (session === null || node.session !== session) return false;
      const seq = ++navSeq.current;
      focus(node);
      try {
        const [nextView, nextCrumbs] = await Promise.all([
          commands.getView(node, undefined, metricRef.current),
          commands.getBreadcrumbs(node),
        ]);
        if (session !== sessionRef.current || nextView.node.session !== session) return false;
        if (seq !== navSeq.current) return true;
        put(nextView);
        setCrumbs(nextCrumbs);
        void loadTreemap(node, seq);
        return true;
      } catch (err) {
        if (session === sessionRef.current && seq === navSeq.current) fail(err);
        return false;
      }
    },
    [fail, focus, loadTreemap, metricRef, put, sessionRef],
  );

  /** Shows a view the host already returned (after a delete), then fills in what hangs off it. */
  const showView = useCallback(
    (next: View) => {
      const session = sessionRef.current;
      if (session === null || next.node.session !== session) return;
      const seq = ++navSeq.current;
      put(next);
      commands
        .getBreadcrumbs(next.node)
        .then((c) => {
          if (seq === navSeq.current && session === sessionRef.current) setCrumbs(c);
        })
        .catch((err) => log(`[ERROR] ${errorMessage(err)}`));
      void loadTreemap(next.node, seq);
    },
    [loadTreemap, log, put, sessionRef],
  );

  /**
   * A refreshed copy of the folder on screen, pushed by the host while the
   * scan runs. Only the open folder is taken; one for a folder the user has
   * since left, or from another session, is dropped.
   */
  const applyLiveView = useCallback(
    (next: View) => {
      if (next.node.session !== sessionRef.current) return;
      if (!sameNode(next.node, viewRef.current?.node)) return;
      put(next);
    },
    [put, sessionRef],
  );

  /** Re-reads the open folder in place, e.g. after the size toggle changes the sort. */
  const reload = useCallback(async () => {
    const current = viewRef.current;
    const session = sessionRef.current;
    if (!current || session === null) return;
    const seq = navSeq.current;
    try {
      const next = await commands.getView(
        current.node,
        current.rows.length || undefined,
        metricRef.current,
      );
      if (session !== sessionRef.current || seq !== navSeq.current) return;
      if (next.node.session !== session) return;
      put(next);
    } catch (err) {
      if (session === sessionRef.current) log(`[ERROR] ${errorMessage(err)}`);
    }
    void loadTreemap(current.node, seq);
  }, [loadTreemap, log, metricRef, put, sessionRef]);

  /** Re-reads only the treemap, whose tiles grow as the scan measures. */
  const refreshTreemap = useCallback(() => {
    const current = viewRef.current;
    if (current) void loadTreemap(current.node, navSeq.current);
  }, [loadTreemap]);

  /** Fetches the next page of a folder that has more rows than were loaded. */
  const loadMore = useCallback(async () => {
    const current = viewRef.current;
    const session = sessionRef.current;
    if (!current || session === null || current.hidden <= 0) return;
    const seq = navSeq.current;
    setLoadingMore(true);
    try {
      const next = await commands.getView(
        current.node,
        current.rows.length + VIEW_PAGE_SIZE,
        metricRef.current,
      );
      // If the user went elsewhere meanwhile, these rows belong to a folder
      // that is no longer open.
      if (seq === navSeq.current && session === sessionRef.current) put(next);
    } catch (err) {
      if (session === sessionRef.current) fail(err);
    } finally {
      if (session === sessionRef.current) setLoadingMore(false);
    }
  }, [fail, metricRef, put, sessionRef]);

  /** The latest view, for callers that must not wait for a render. */
  const currentView = useCallback(() => viewRef.current, []);

  return {
    view,
    crumbs,
    treemap,
    loadingMore,
    open,
    showView,
    applyLiveView,
    reload,
    refreshTreemap,
    loadMore,
    currentView,
    clear,
  };
}
