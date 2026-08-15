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

type Phase = "setup" | "connecting" | "scanning" | "result";

const MAX_LOG_LINES = 200;
const SEARCH_DEBOUNCE_MS = 150;

/**
 * The connect → scan → navigate → delete state machine.
 *
 * Lifted out of the app component, where it was interleaved with JSX, toast
 * timers and icon definitions in a single 864-line file.
 */
export function useScanSession(notify: (msg: string, type?: "error" | "success" | "info") => void) {
  const [phase, setPhase] = useState<Phase>("setup");
  const [stats, setStats] = useState<Stats | null>(null);
  const [view, setView] = useState<View | null>(null);
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
  const [searchHits, setSearchHits] = useState<Row[]>([]);

  // Derived rather than stored: "no query" is a property of the query, so
  // writing it into state from an effect would only add a render pass.
  const trimmedQuery = query.trim();
  const searchResults = trimmedQuery ? searchHits : null;

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

  // Live scan updates. The host pushes only the view we said we were watching,
  // so this payload does not grow with the size of the device.
  useEffect(() => {
    const pending = ipc.onScanProgress((p) => {
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
    try {
      const [top, groups, owners] = await Promise.all([
        ipc.largestFiles(100),
        ipc.typeBreakdown(),
        ipc.appBreakdown(50),
      ]);
      setLargest(top);
      setTypes(groups);
      setApps(owners);
    } catch {
      // These are secondary views; a failure here should not disturb the scan.
    }
  }, []);

  const refreshTreemap = useCallback(async (id: number) => {
    try {
      setTreemap(await ipc.getTreemap(id, 2));
    } catch {
      // A stale treemap is not worth interrupting the user over.
      setTreemap(null);
    }
  }, []);

  const open = useCallback(
    async (id: number) => {
      try {
        const [nextView, nextCrumbs] = await Promise.all([ipc.getView(id), ipc.getBreadcrumbs(id)]);
        setView(nextView);
        setCrumbs(nextCrumbs);
        void refreshTreemap(id);
      } catch (err) {
        fail(err);
      }
    },
    [fail, refreshTreemap],
  );

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
    setPhase("scanning");
    setElapsedMs(null);
    setQuery("");
    const started = performance.now();
    log("[SCAN] Walking the device…");

    try {
      const final = await ipc.scan();
      setStats(final);
      setElapsedMs(performance.now() - started);
      await open(ROOT_ID);
      void refreshAnalyses();
      setPhase("result");
      log(`[SCAN] ${final.files.toLocaleString()} files, ${final.dirs.toLocaleString()} folders.`);
    } catch (err) {
      fail(err);
      setPhase((p) => (p === "scanning" && view ? "result" : "setup"));
    }
  }, [fail, log, open, refreshAnalyses, view]);

  /** `serial` picks a specific device; omitted, the host chooses the only usable one. */
  const connect = useCallback(
    async (serial?: string) => {
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
    log("[SOCKET] Stopping the daemon…");
    try {
      await ipc.disconnect();
      notify("Disconnected", "info");
    } catch {
      log("[SOCKET] Daemon was already stopped.");
    }
    setView(null);
    setStats(null);
    setCrumbs([]);
    setTreemap(null);
    setQuery("");
    setLargest([]);
    setTypes([]);
    setApps([]);
    setPrevious(null);
    setPhase("setup");
  }, [log, notify]);

  const remove = useCallback(
    async (row: Row) => {
      log(`[DELETE] ${row.name}…`);
      try {
        const result = await ipc.deleteNode(row.id);
        setStats(result.stats);
        if (result.view) setView(result.view);
        if (view) void refreshTreemap(view.id);
        // A deleted node may still be sitting in the results list.
        setSearchHits((hits) => hits.filter((h) => h.id !== row.id));
        void refreshAnalyses();
        notify(`Deleted ${row.name}`, "success");
        log(`[DELETE] Removed ${result.items.toLocaleString()} items.`);
      } catch (err) {
        fail(err);
      }
    },
    [fail, log, notify, refreshAnalyses, refreshTreemap, view],
  );

  // Debounced so typing does not fire a query per keystroke.
  const searchSeq = useRef(0);
  useEffect(() => {
    if (!trimmedQuery) return;

    const seq = ++searchSeq.current;
    const timer = setTimeout(async () => {
      try {
        const hits = await ipc.search(trimmedQuery);
        // Drop a slow response that a newer query has already superseded.
        if (seq === searchSeq.current) setSearchHits(hits);
      } catch {
        if (seq === searchSeq.current) setSearchHits([]);
      }
    }, SEARCH_DEBOUNCE_MS);

    return () => clearTimeout(timer);
  }, [trimmedQuery]);

  return {
    phase,
    stats,
    view,
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
    setQuery,
    setMode,
    reveal,
    connect,
    disconnect,
    scan,
    open,
    remove,
  };
}
