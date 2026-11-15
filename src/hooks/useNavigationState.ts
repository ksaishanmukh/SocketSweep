import { useCallback, useRef, useState } from "react";
import type { Crumb, TreemapNode, View } from "../api";
import { EMPTY_HISTORY, type History } from "../lib/history";

/**
 * What the navigation hooks share: the open folder, its path, map and
 * history, and the counter that lets the slower of two quick clicks lose.
 */
export function useNavigationState() {
  const [view, setView] = useState<View | null>(null);
  const [crumbs, setCrumbs] = useState<Crumb[]>([]);
  const [treemap, setTreemap] = useState<TreemapNode | null>(null);
  const [loadingMore, setLoadingMore] = useState(false);
  const [history, setHistoryState] = useState<History>(EMPTY_HISTORY);

  /** Mirrors `view` for async code that must not read a stale closure. */
  const viewRef = useRef<View | null>(null);
  const historyRef = useRef<History>(EMPTY_HISTORY);
  const navSeqRef = useRef(0);

  const put = useCallback((next: View | null) => {
    viewRef.current = next;
    setView(next);
  }, []);

  const setHistory = useCallback((next: History) => {
    historyRef.current = next;
    setHistoryState(next);
  }, []);

  const clear = useCallback(() => {
    navSeqRef.current += 1;
    put(null);
    setCrumbs([]);
    setTreemap(null);
    setLoadingMore(false);
    setHistory(EMPTY_HISTORY);
  }, [put, setHistory]);

  /** The latest view, for callers that must not wait for a render. */
  const currentView = useCallback(() => viewRef.current, []);

  return {
    view,
    crumbs,
    treemap,
    loadingMore,
    history,
    setCrumbs,
    setTreemap,
    setLoadingMore,
    setHistory,
    viewRef,
    historyRef,
    navSeqRef,
    put,
    clear,
    currentView,
  };
}

export type NavigationState = ReturnType<typeof useNavigationState>;
