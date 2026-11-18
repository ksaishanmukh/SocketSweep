import { useCallback, useEffect, useRef, useState } from "react";
import { commands, type Row } from "../api";
import { searchFilter } from "../lib/filterWire";
import type { SessionDeps } from "./types";

const SEARCH_DEBOUNCE_MS = 150;

/** Search results, tagged with what they answered. */
interface Hits {
  query: string;
  session: number;
  rows: Row[];
}

interface Deps extends Pick<SessionDeps, "sessionRef" | "metricRef" | "optionsRef"> {
  sessionId: number | null;
  /** What to search the whole phone for; empty when not searching everywhere. */
  query: string;
}

/**
 * Name search over the whole tree, narrowed by the list's filters and in its
 * order.
 *
 * Hits are shown only for the query and the session they answered, so a late
 * or old answer can never be presented as the answer to what is in the box
 * now, nor after a rescan.
 */
export function useSearch({ sessionRef, metricRef, optionsRef, sessionId, query }: Deps) {
  const [hits, setHits] = useState<Hits | null>(null);
  /** Bumped to re-run the current search: after a delete, a new sort or filter, or the scan finishing. */
  const [nonce, setNonce] = useState(0);

  const trimmed = query.trim();
  // Derived rather than stored: "no query" is a property of the query.
  const results =
    trimmed && hits?.query === trimmed && hits.session === sessionId ? hits.rows : null;

  const rerun = useCallback(() => setNonce((n) => n + 1), []);
  const clear = useCallback(() => setHits(null), []);

  /** Drops hits the caller knows are gone, without waiting for a fresh answer. */
  const dropHits = useCallback((gone: (row: Row) => boolean) => {
    setHits((h) => (h ? { ...h, rows: h.rows.filter((r) => !gone(r)) } : h));
  }, []);

  // Debounced so typing does not fire a query per keystroke.
  const seqRef = useRef(0);
  useEffect(() => {
    const seq = ++seqRef.current;
    if (!trimmed || sessionId === null) return;

    const timer = setTimeout(async () => {
      // Drop a slow response that a newer query, a rescan or a disconnect has
      // already superseded.
      const current = () => seq === seqRef.current && sessionId === sessionRef.current;
      const { sort, filter } = optionsRef.current;
      try {
        const rows = await commands.search({
          query: trimmed,
          by: metricRef.current,
          filter: searchFilter(filter),
          sort,
          within: null,
        });
        if (current()) setHits({ query: trimmed, session: sessionId, rows });
      } catch {
        if (current()) setHits({ query: trimmed, session: sessionId, rows: [] });
      }
    }, SEARCH_DEBOUNCE_MS);

    return () => clearTimeout(timer);
  }, [trimmed, nonce, sessionId, sessionRef, metricRef, optionsRef]);

  return { results, rerun, clear, dropHits };
}
