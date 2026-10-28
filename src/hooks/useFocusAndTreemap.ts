import { useCallback } from "react";
import { commands, errorMessage, type NodeRef } from "../api";
import type { SessionDeps } from "./types";
import type { NavigationState } from "./useNavigationState";

/**
 * Tells the host which folder the user is looking at, and how: the live
 * views it pushes while the scan runs use the size and row count given here
 * (`getView` does not change them), so ask again whenever either changes.
 * Failure only costs speed.
 */
export function useFocus({ metricRef, log }: Pick<SessionDeps, "metricRef" | "log">) {
  return useCallback(
    (node: NodeRef, limit?: number) => {
      commands
        .focus(node, limit, metricRef.current)
        .catch((err) => log(`[FOCUS] ${errorMessage(err)}`));
    },
    [log, metricRef],
  );
}

/** Loads the treemap of a folder, unless the user has moved on by the time it arrives. */
export function useLoadTreemap(
  { sessionRef, metricRef }: Pick<SessionDeps, "sessionRef" | "metricRef">,
  { navSeqRef, setTreemap }: Pick<NavigationState, "navSeqRef" | "setTreemap">,
) {
  return useCallback(
    async (node: NodeRef, seq: number) => {
      const session = sessionRef.current;
      const current = () => seq === navSeqRef.current && session === sessionRef.current;
      try {
        const next = await commands.getTreemap(node, 2, metricRef.current);
        if (current() && next.node.session === session) setTreemap(next);
      } catch {
        // A missing treemap is not worth interrupting the user over.
        if (current()) setTreemap(null);
      }
    },
    [metricRef, navSeqRef, sessionRef, setTreemap],
  );
}
