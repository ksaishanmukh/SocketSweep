import { useCallback, useEffect, useRef, useState } from "react";
import { commands, type Row } from "../api";
import type { SessionDeps } from "./types";

const SEARCH_DEBOUNCE_MS = 150;

/** Search results, tagged with what they answered. */
interface Hits {
  query: string;
  session: number;
  rows: Row[];
}

/**
 * Name search over the host's tree.
 *
 * Hits are shown only for the query and the session they answered, so a late
 * or old answer can never be presented as the answer to what is in the box
 * now, nor after a rescan.
 */
export function useSearch({
  sessionRef,
  metricRef,
  sessionId,
}: Pick<SessionDeps, "sessionRef" | "metricRef"> & { sessionId: number | null }) {
  const [query, setQuery] = useState("");
  const [hits, setHits] = useState<Hits | null>(null);
  /** Bumped to re-run the current search, e.g. after a delete or when the scan finishes. */
  const [nonce, setNonce] = useState(0);

  const trimmed = query.trim();
  // Derived rather than stored: "no query" is a property of the query.
  const results =
    trimmed && hits?.query === trimmed && hits.session === sessionId ? hits.rows : null;

  const updateQuery = useCallback((next: string) => {
    setQuery(next);
    if (!next.trim()) setHits(null);
  }, []);

  const rerun = useCallback(() => setNonce((n) => n + 1), []);

  const clear = useCallback(() => {
    setQuery("");
    setHits(null);
  }, []);

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
      try {
        const rows = await commands.search(trimmed, undefined, metricRef.current);
        if (current()) setHits({ query: trimmed, session: sessionId, rows });
      } catch {
        if (current()) setHits({ query: trimmed, session: sessionId, rows: [] });
      }
    }, SEARCH_DEBOUNCE_MS);

    return () => clearTimeout(timer);
  }, [trimmed, nonce, sessionId, sessionRef, metricRef]);

  return { query, setQuery: updateQuery, results, rerun, clear, dropHits };
}
