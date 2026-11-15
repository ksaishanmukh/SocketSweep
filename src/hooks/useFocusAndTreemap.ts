import { useCallback } from "react";
import { commands, errorMessage, type NodeRef } from "../api";
import type { SessionDeps } from "./types";
import type { NavigationState } from "./useNavigationState";

/**
 * Tells the host which folder the user is looking at, and how: the live
 * views it pushes while the scan runs use the size, row count, sort and
 * filter given here (`getView` does not change them), so ask again whenever
 * any of them changes. Failure only costs speed.
 */
export function useFocus({
  metricRef,
  optionsRef,
  log,
}: Pick<SessionDeps, "metricRef" | "optionsRef" | "log">) {
  return useCallback(
    (node: NodeRef, limit?: number) => {
      const { sort, filter } = optionsRef.current;
      commands
        .focus(node, limit, metricRef.current, sort, filter)
        .catch((err) => log(`[FOCUS] ${errorMessage(err)}`));
    },
    [log, metricRef, optionsRef],
  );
}

/**
 * Loads the map of a folder, unless the user has moved on by the time it
 * arrives. One level: the map shows the open folder's children, the same
 * things the list does, so the two can point at each other.
 */
export function useLoadTreemap(
  { sessionRef, metricRef }: Pick<SessionDeps, "sessionRef" | "metricRef">,
  { navSeqRef, setTreemap }: Pick<NavigationState, "navSeqRef" | "setTreemap">,
) {
  return useCallback(
    async (node: NodeRef, seq: number) => {
      const session = sessionRef.current;
      const current = () => seq === navSeqRef.current && session === sessionRef.current;
      try {
        const next = await commands.getTreemap(node, 1, metricRef.current);
        if (current() && next.node.session === session) setTreemap(next);
      } catch {
        // A missing map is not worth interrupting the user over.
        if (current()) setTreemap(null);
      }
    },
    [metricRef, navSeqRef, sessionRef, setTreemap],
  );
}
