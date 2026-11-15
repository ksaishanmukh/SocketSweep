import { useCallback } from "react";
import { commands, errorMessage, type NodeRef, type View } from "../api";
import { visit } from "../lib/history";
import { sameNode } from "../lib/size";
import type { SessionDeps } from "./types";
import type { NavigationState } from "./useNavigationState";

interface Deps extends SessionDeps {
  state: NavigationState;
  focus: (node: NodeRef, limit?: number) => void;
  loadTreemap: (node: NodeRef, seq: number) => Promise<void>;
}

/**
 * The ways a folder comes on screen: opened by the user, shown from a reply
 * the host already gave (after a delete), or pushed live while the scan runs.
 */
export function useOpenFolder(deps: Deps) {
  const { sessionRef, log, state, loadTreemap } = deps;
  const { navSeqRef, viewRef, historyRef, put, setCrumbs, setHistory } = state;
  const open = useOpen(deps);

  /** Shows a view the host already returned (after a delete), then fills in what hangs off it. */
  const showView = useCallback(
    (next: View) => {
      const session = sessionRef.current;
      if (session === null || next.node.session !== session) return;
      const seq = ++navSeqRef.current;
      put(next);
      setHistory(visit(historyRef.current, next.node));
      commands
        .getBreadcrumbs(next.node)
        .then((c) => {
          if (seq === navSeqRef.current && session === sessionRef.current) setCrumbs(c);
        })
        .catch((err) => log(`[ERROR] ${errorMessage(err)}`));
      void loadTreemap(next.node, seq);
    },
    [historyRef, loadTreemap, log, navSeqRef, put, sessionRef, setCrumbs, setHistory],
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
    [put, sessionRef, viewRef],
  );

  return { open, showView, applyLiveView };
}

/**
 * Opens a folder. Resolves false only when the host could not show it; a
 * navigation overtaken by a newer one counts as fine, since the newer one
 * reports for itself. `record` false is a step through the history, which
 * must not add to it.
 */
function useOpen({ sessionRef, metricRef, optionsRef, fail, state, focus, loadTreemap }: Deps) {
  const { navSeqRef, historyRef, put, setCrumbs, setHistory } = state;
  return useCallback(
    async (node: NodeRef, record = true): Promise<boolean> => {
      const session = sessionRef.current;
      // A node from another session names nothing in this tree.
      if (session === null || node.session !== session) return false;
      const seq = ++navSeqRef.current;
      focus(node);
      const { sort, filter } = optionsRef.current;
      try {
        const [nextView, nextCrumbs] = await Promise.all([
          commands.getView(node, undefined, metricRef.current, sort, filter),
          commands.getBreadcrumbs(node),
        ]);
        if (session !== sessionRef.current || nextView.node.session !== session) return false;
        if (seq !== navSeqRef.current) return true;
        put(nextView);
        setCrumbs(nextCrumbs);
        if (record) setHistory(visit(historyRef.current, node));
        void loadTreemap(node, seq);
        return true;
      } catch (err) {
        if (session === sessionRef.current && seq === navSeqRef.current) fail(err);
        return false;
      }
    },
    [
      fail,
      focus,
      historyRef,
      loadTreemap,
      metricRef,
      navSeqRef,
      optionsRef,
      put,
      sessionRef,
      setCrumbs,
      setHistory,
    ],
  );
}
