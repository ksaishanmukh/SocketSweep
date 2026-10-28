import { useCallback, useRef, useState } from "react";
import type { Crumb, TreemapNode, View } from "../api";

/**
 * What the navigation hooks share: the open folder, its path and treemap, and
 * the counter that lets the slower of two quick clicks lose.
 */
export function useNavigationState() {
  const [view, setView] = useState<View | null>(null);
  const [crumbs, setCrumbs] = useState<Crumb[]>([]);
  const [treemap, setTreemap] = useState<TreemapNode | null>(null);
  const [loadingMore, setLoadingMore] = useState(false);

  /** Mirrors `view` for async code that must not read a stale closure. */
  const viewRef = useRef<View | null>(null);
  const navSeqRef = useRef(0);

  const put = useCallback((next: View | null) => {
    viewRef.current = next;
    setView(next);
  }, []);

  const clear = useCallback(() => {
    navSeqRef.current += 1;
    put(null);
    setCrumbs([]);
    setTreemap(null);
    setLoadingMore(false);
  }, [put]);

  /** The latest view, for callers that must not wait for a render. */
  const currentView = useCallback(() => viewRef.current, []);

  return {
    view,
    crumbs,
    treemap,
    loadingMore,
    setCrumbs,
    setTreemap,
    setLoadingMore,
    viewRef,
    navSeqRef,
    put,
    clear,
    currentView,
  };
}

export type NavigationState = ReturnType<typeof useNavigationState>;
