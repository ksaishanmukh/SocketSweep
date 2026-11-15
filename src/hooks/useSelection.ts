import { useCallback, useMemo, useState } from "react";
import type { NodeRef } from "../api";
import { inSession, type Target } from "../lib/selection";
import { nodeKey } from "../lib/size";

/**
 * What is picked for a batch action, across the whole screen: list rows,
 * biggest files and leftovers alike.
 *
 * Picks of another session are never shown or acted on, even before `clear`
 * runs, because their nodes name nothing in the current tree.
 */
export function useSelection(sessionId: number | null) {
  const [picked, setPicked] = useState<ReadonlyMap<string, Target>>(new Map());

  const items = useMemo(
    () => [...picked.values()].filter((t) => inSession(t, sessionId)),
    [picked, sessionId],
  );
  const keys = useMemo(() => new Set(items.map((t) => t.key)), [items]);

  const has = useCallback((key: string) => keys.has(key), [keys]);

  /** Picks or drops every one of `targets` together, as the select-all box does. */
  const setMany = useCallback((targets: Target[], on: boolean) => {
    setPicked((prev) => {
      const next = new Map(prev);
      for (const t of targets) {
        if (on) next.set(t.key, t);
        else next.delete(t.key);
      }
      return next;
    });
  }, []);

  const toggle = useCallback(
    (target: Target) => setMany([target], !keys.has(target.key)),
    [keys, setMany],
  );

  /** Drops the picks that held any of `nodes`, e.g. once they are deleted. */
  const forget = useCallback((nodes: NodeRef[]) => {
    const gone = new Set(nodes.map(nodeKey));
    setPicked((prev) => {
      const next = new Map(prev);
      for (const [key, t] of prev) {
        if (t.nodes.some((n) => gone.has(nodeKey(n)))) next.delete(key);
      }
      return next;
    });
  }, []);

  const clear = useCallback(() => setPicked(new Map()), []);

  return { items, has, toggle, setMany, forget, clear };
}

export type Selection = ReturnType<typeof useSelection>;
