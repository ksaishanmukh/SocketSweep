import { useCallback, useEffect, useRef, useState } from "react";
import * as ipc from "../lib/ipc";
import {
  ROOT_ID,
  type Crumb,
  type Row,
  type ScanRecord,
  type AppUsage,
  type Stats,
  type TreemapNode,
  type TypeGroup,
  type View,
} from "../lib/types";

/** Which analysis the main canvas is showing. */
export type Mode = "treemap" | "largest" | "types" | "apps";

/** `failed` is a scan that did not finish: connected, but with nothing trustworthy to show. */
export type Phase = "setup" | "connecting" | "scanning" | "result" | "failed";

const MAX_LOG_LINES = 200;
const SEARCH_DEBOUNCE_MS = 150;
/** How many more rows "Load more" asks the host for each time. */
export const VIEW_PAGE_SIZE = 500;

/** Search results, tagged with the query they answered. */
interface Hits {
  query: string;
  rows: Row[];
}

/**
 * The connect → scan → navigate → delete state machine.
 *
 * Lifted out of the app component, where it was interleaved with JSX, toast
 * timers and icon definitions in a single 864-line file.
 *
 * Two counters keep late replies from landing on the wrong screen. The
 * *generation* changes whenever the tree is replaced or dropped (a scan starts,
 * the device is disconnected); any reply that began under an older one is
 * discarded, because the ids in it name nothing now. The *navigation* counter
 * orders navigations within one generation, so the slower of two quick clicks
 * cannot overwrite the later one.
 */
export function useScanSession(notify: (msg: string, type?: "error" | "success" | "info") => void) {
  const [phase, setPhase] = useState<Phase>("setup");
  const [failure, setFailure] = useState<string | null>(null);
  const [stats, setStats] = useState<Stats | null>(null);
  const [view, setView] = useState<View | null>(null);
  const [loadingMore, setLoadingMore] = useState(false);
  const [crumbs, setCrumbs] = useState<Crumb[]>([]);
  const [treemap, setTreemap] = useState<TreemapNode | null>(null);
  const [elapsedMs, setElapsedMs] = useState<number | null>(null);
  const [scanningPath, setScanningPath] = useState("");
  const [logs, setLogs] = useState<string[]>([]);

  const [mode, setMode] = useState<Mode>("treemap");
  const [largest, setLargest] = useState<Row[]>([]);
  const [types, setTypes] = useState<TypeGroup[]>([]);
  const [apps, setApps] = useState<AppUsage[]>([]);
  /** Baseline captured at connect, so a rescan still compares to last session. */
  const [previous, setPrevious] = useState<ScanRecord | null>(null);

  const [query, setQuery] = useState("");
  const [searchHits, setSearchHits] = useState<Hits | null>(null);
  /** Bumped to re-run the current search, e.g. after a delete. */
  const [searchNonce, setSearchNonce] = useState(0);

  // Exposed so the owner of any id-bearing state (the delete dialog) can tell
  // that its ids went stale. The ref is what async code compares against.
  const [generation, setGeneration] = useState(0);
  const generationRef = useRef(0);
  const navSeq = useRef(0);
  /** Progress events that arrive outside a scan are leftovers and must not repaint. */
  const scanning = useRef(false);

  // Derived rather than stored: "no query" is a property of the query, so
  // writing it into state from an effect would only add a render pass. Hits are
  // shown only for the query they answered, so a late or old answer can never
  // be presented as the answer to what is in the box now.
  const trimmedQuery = query.trim();
  const searchResults = trimmedQuery && searchHits?.query === trimmedQuery ? searchHits.rows : null;

  const log = useCallback((msg: string) => {
    setLogs((prev) =>
      [...prev, `[${new Date().toLocaleTimeString()}] ${msg}`].slice(-MAX_LOG_LINES),
    );
  }, []);

  const fail = useCallback(
    (err: unknown) => {
      const message = ipc.errorMessage(err);
      notify(message, "error");
      log(`[ERROR] ${message}`);
      return message;
    },
    [log, notify],
  );

  /** Starts a new generation: every reply still in flight is now stale. */
  const nextGeneration = useCallback(() => {
    generationRef.current += 1;
    navSeq.current += 1;
    setGeneration(generationRef.current);
    return generationRef.current;
  }, []);

  /** Forgets everything that refers to node ids of the tree being replaced. */
  const clearTree = useCallback(() => {
    setView(null);
    setLoadingMore(false);
    setCrumbs([]);
    setTreemap(null);
    setLargest([]);
    setTypes([]);
    setApps([]);
    setQuery("");
    setSearchHits(null);
  }, []);

  // Live scan updates. The host pushes only the view we said we were watching,
  // so this payload does not grow with the size of the device.
  useEffect(() => {
    const pending = ipc.onScanProgress((p) => {
      if (!scanning.current) return;
      setStats(p.stats);
      if (p.view) {
        setView(p.view);
        setScanningPath(p.view.path);
      }
    });
    return () => {
      pending.then((off) => off());
    };
  }, []);

  /** The cross-tree analyses. Cheap Rust queries, so just re-run both. */
  const refreshAnalyses = useCallback(async () => {
    const gen = generationRef.current;
    try {
      const [top, groups, owners] = await Promise.all([
        ipc.largestFiles(100),
        ipc.typeBreakdown(),
        ipc.appBreakdown(50),
      ]);
      if (gen !== generationRef.current) return;
      setLargest(top);
      setTypes(groups);
      setApps(owners);
    } catch {
      // These are secondary views; a failure here should not disturb the scan.
    }
  }, []);

  const loadTreemap = useCallback(async (id: number, seq: number, gen: number) => {
    const current = () => seq === navSeq.current && gen === generationRef.current;
    try {
      const next = await ipc.getTreemap(id, 2);
      if (current()) setTreemap(next);
    } catch {
      // A stale treemap is not worth interrupting the user over.
      if (current()) setTreemap(null);
    }
  }, []);

  /**
   * Opens a folder. Resolves false only when the host could not show it; a
   * navigation overtaken by a newer one counts as fine, since the newer one
   * reports for itself.
   */
  const open = useCallback(
    async (id: number): Promise<boolean> => {
      const seq = ++navSeq.current;
      const gen = generationRef.current;
      try {
        const [nextView, nextCrumbs] = await Promise.all([ipc.getView(id), ipc.getBreadcrumbs(id)]);
        if (gen !== generationRef.current) return false;
        if (seq !== navSeq.current) return true;
        setView(nextView);
        setCrumbs(nextCrumbs);
        void loadTreemap(id, seq, gen);
        return true;
      } catch (err) {
        if (gen === generationRef.current && seq === navSeq.current) fail(err);
        return false;
      }
    },
    [fail, loadTreemap],
  );

  /** Shows a view the host already returned, then fills in what hangs off it. */
  const showView = useCallback(
    (next: View) => {
      const seq = ++navSeq.current;
      const gen = generationRef.current;
      setView(next);
      ipc
        .getBreadcrumbs(next.id)
        .then((c) => {
          if (seq === navSeq.current && gen === generationRef.current) setCrumbs(c);
        })
        .catch((err) => log(`[ERROR] ${ipc.errorMessage(err)}`));
      void loadTreemap(next.id, seq, gen);
    },
    [loadTreemap, log],
  );

  /** Fetches the next page of a folder that has more rows than were loaded. */
  const loadMore = useCallback(async () => {
    if (!view || view.hidden <= 0) return;
    const { id } = view;
    const seq = navSeq.current;
    const gen = generationRef.current;
    setLoadingMore(true);
    try {
      const next = await ipc.getView(id, view.rows.length + VIEW_PAGE_SIZE);
      // If the user went elsewhere meanwhile, these rows belong to a folder
      // that is no longer open.
      if (seq === navSeq.current && gen === generationRef.current) setView(next);
    } catch (err) {
      if (gen === generationRef.current) fail(err);
    } finally {
      if (gen === generationRef.current) setLoadingMore(false);
    }
  }, [fail, view]);

  /**
   * Jump to the folder containing a cross-tree result, and back to the treemap
   * so the destination has context. Finding a huge file is only half the job.
   */
  const reveal = useCallback(
    async (row: Row) => {
      if (row.parentId === undefined) return;
      setMode("treemap");
      setQuery("");
      await open(row.parentId);
    },
    [open],
  );

  const scan = useCallback(async () => {
    const gen = nextGeneration();
    // The old tree is gone the moment a scan starts. Keeping its view around
    // would let a failed rescan show half of one scan under the totals of
    // another.
    clearTree();
    scanning.current = true;
    setFailure(null);
    setPhase("scanning");
    setElapsedMs(null);
    setScanningPath("");
    const started = performance.now();
    log("[SCAN] Walking the device…");

    try {
      const final = await ipc.scan();
      // Disconnect cancels a running scan; it has already reset everything.
      if (gen !== generationRef.current) return;
      scanning.current = false;
      setStats(final);
      setElapsedMs(performance.now() - started);

      if (!(await open(ROOT_ID))) {
        if (gen !== generationRef.current) return;
        // `open` has already shown why.
        setFailure("The scan finished, but its results could not be opened.");
        clearTree();
        setPhase("failed");
        return;
      }
      if (gen !== generationRef.current) return;
      void refreshAnalyses();
      setPhase("result");
      log(`[SCAN] ${final.files.toLocaleString()} files, ${final.dirs.toLocaleString()} folders.`);
    } catch (err) {
      if (gen !== generationRef.current) return;
      scanning.current = false;
      const message = fail(err);
      // The host keeps the partial tree but never marks it finished, so none of
      // it is offered as a result.
      clearTree();
      setStats(null);
      setFailure(message);
      setPhase("failed");
    }
  }, [clearTree, fail, log, nextGeneration, open, refreshAnalyses]);

  /** `serial` picks a specific device; omitted, the host chooses the only usable one. */
  const connect = useCallback(
    async (serial?: string) => {
      setFailure(null);
      setPhase("connecting");
      try {
        log("[ADB] Looking for a device…");
        const info = await ipc.connect(serial);
        // Captured once per connection, so a rescan still compares against the
        // previous session rather than against the scan a moment ago.
        setPrevious(info.previous);
        log(`[ADB] ${info.model} (${info.serial}).`);
        log("[SOCKET] Daemon listening on an abstract socket.");
        await scan();
      } catch (err) {
        fail(err);
        setPhase("setup");
      }
    },
    [fail, log, scan],
  );

  const disconnect = useCallback(async () => {
    // First, so replies to anything still running are dropped, including the
    // scan that this call is about to cancel.
    nextGeneration();
    scanning.current = false;
    log("[SOCKET] Stopping the daemon…");
    try {
      await ipc.disconnect();
      notify("Disconnected", "info");
    } catch {
      log("[SOCKET] Daemon was already stopped.");
    }
    clearTree();
    setStats(null);
    setFailure(null);
    setElapsedMs(null);
    setScanningPath("");
    setMode("treemap");
    setPrevious(null);
    setPhase("setup");
  }, [clearTree, log, nextGeneration, notify]);

  const remove = useCallback(
    async (row: Row) => {
      const gen = generationRef.current;
      log(`[DELETE] ${row.name}…`);
      try {
        const result = await ipc.deleteNode(row.id, row.gen);
        if (gen !== generationRef.current) return;
        setStats(result.stats);
        // Never null: if the open folder went away, this is its nearest
        // surviving ancestor, so the screen never points at a deleted node.
        showView(result.view);
        // A deleted node may still be sitting in the results list, along with
        // anything that lived inside it.
        setSearchHits((hits) =>
          hits ? { ...hits, rows: hits.rows.filter((h) => h.id !== row.id) } : hits,
        );
        setSearchNonce((n) => n + 1);
        void refreshAnalyses();
        notify(`Deleted ${row.name}`, "success");
        log(`[DELETE] Removed ${result.items.toLocaleString()} items.`);
      } catch (err) {
        // Includes "Results are out of date": the host refuses and nothing
        // changes, so all there is to do is say so.
        fail(err);
      }
    },
    [fail, log, notify, refreshAnalyses, showView],
  );

  const updateQuery = useCallback((next: string) => {
    setQuery(next);
    if (!next.trim()) setSearchHits(null);
  }, []);

  // Debounced so typing does not fire a query per keystroke.
  const searchSeq = useRef(0);
  useEffect(() => {
    const seq = ++searchSeq.current;
    if (!trimmedQuery) return;

    const gen = generationRef.current;
    const timer = setTimeout(async () => {
      // Drop a slow response that a newer query, a rescan or a disconnect has
      // already superseded.
      const current = () => seq === searchSeq.current && gen === generationRef.current;
      try {
        const rows = await ipc.search(trimmedQuery);
        if (current()) setSearchHits({ query: trimmedQuery, rows });
      } catch {
        if (current()) setSearchHits({ query: trimmedQuery, rows: [] });
      }
    }, SEARCH_DEBOUNCE_MS);

    return () => clearTimeout(timer);
  }, [trimmedQuery, searchNonce]);

  return {
    phase,
    failure,
    generation,
    stats,
    view,
    loadingMore,
    crumbs,
    treemap,
    elapsedMs,
    scanningPath,
    logs,
    query,
    searchResults,
    mode,
    largest,
    types,
    apps,
    previous,
    setQuery: updateQuery,
    setMode,
    reveal,
    connect,
    disconnect,
    scan,
    open,
    loadMore,
    remove,
  };
}
