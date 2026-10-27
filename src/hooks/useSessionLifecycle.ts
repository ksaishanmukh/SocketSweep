import { useCallback, useEffect, useRef, useState } from "react";
import {
  commands,
  events,
  type ConnectionLost,
  type NodeRef,
  type ScanOutcome,
  type ScanPhase,
  type ScanProgress,
  type ScanRecord,
  type Totals,
} from "../api";
import { formatNumber } from "../lib/format";
import type { Notify, SessionDeps } from "./types";
import type { useAnalyses } from "./useAnalyses";
import type { useNavigation } from "./useNavigation";
import type { useSearch } from "./useSearch";

/**
 * `connecting` covers connect and the scan's first moments, until the
 * skeleton is listed. From then on the user is `browsing` while sizes keep
 * filling in; the scan itself is described by {@link ScanInfo}. `failed` is a
 * scan that never produced anything to browse.
 */
export type Phase = "setup" | "connecting" | "browsing" | "failed";

/** The running (or ended) scan, as the last `scan-progress` described it. */
export interface ScanInfo {
  phase: ScanPhase;
  running: boolean;
  listed: number;
  known: number;
  elapsedMs: number;
  /** The focused folder is fully measured. */
  focusSettled: boolean;
  /** Set once the scan has ended. */
  outcome: ScanOutcome | null;
  /** Why, when it failed. */
  message: string | null;
}

interface Deps extends SessionDeps {
  notify: Notify;
  setSessionId: (id: number | null) => void;
  setTotals: (totals: Totals | null) => void;
  nav: ReturnType<typeof useNavigation>;
  analyses: ReturnType<typeof useAnalyses>;
  search: ReturnType<typeof useSearch>;
  /** Forgets everything that holds nodes of the tree being replaced or dropped. */
  reset: () => void;
}

const SKELETON_NOTE = "[SCAN] Top levels listed. Browsing while sizes fill in.";

/**
 * connect → scan → browse → disconnect.
 *
 * Owns the current session id. Three things end a tree, and each starts by
 * clearing the session so that every reply still in flight is dropped: a new
 * scan, a disconnect and a `connection-lost` event. The *epoch* does the same
 * for the stretch before a session id exists (connecting, `startScan` in
 * flight).
 */
export function useSessionLifecycle({
  sessionRef,
  log,
  fail,
  notify,
  setSessionId,
  setTotals,
  nav,
  analyses,
  search,
  reset,
}: Deps) {
  const [phase, setPhaseState] = useState<Phase>("setup");
  const [status, setStatus] = useState("");
  const [failure, setFailure] = useState<string | null>(null);
  const [scanInfo, setScanInfo] = useState<ScanInfo | null>(null);
  const [previous, setPrevious] = useState<ScanRecord | null>(null);
  const [model, setModel] = useState<string | null>(null);

  const phaseRef = useRef<Phase>("setup");
  const setPhase = useCallback((next: Phase) => {
    phaseRef.current = next;
    setPhaseState(next);
  }, []);

  const epochRef = useRef(0);
  const rootRef = useRef<NodeRef | null>(null);
  /** True from the call to `startScan` until it answers; events before then belong to no known session. */
  const starting = useRef(false);
  const buffered = useRef<ScanProgress | null>(null);
  const entered = useRef(false);
  const finishedSeen = useRef(false);

  const bind = useCallback(
    (id: number | null) => {
      sessionRef.current = id;
      setSessionId(id);
    },
    [sessionRef, setSessionId],
  );

  /** Ends the current tree, whatever replaces it: every pending reply becomes stale. */
  const invalidate = useCallback(() => {
    epochRef.current += 1;
    starting.current = false;
    buffered.current = null;
    entered.current = false;
    finishedSeen.current = false;
    rootRef.current = null;
    bind(null);
    reset();
    setTotals(null);
    setScanInfo(null);
  }, [bind, reset, setTotals]);

  /** The skeleton is listed: open the root and start browsing. */
  const enter = useCallback(
    (root: NodeRef) => {
      entered.current = true;
      const epoch = epochRef.current;
      void (async () => {
        const opened = await nav.open(root);
        if (epoch !== epochRef.current) return;
        if (!opened) {
          // `open` has already shown why.
          reset();
          setFailure("The scan started, but its results could not be opened.");
          setPhase("failed");
          return;
        }
        setPhase("browsing");
        log(SKELETON_NOTE);
        void analyses.refresh();
      })();
    },
    [analyses, log, nav, reset, setPhase],
  );

  const onProgress = useCallback(
    (p: ScanProgress) => {
      const session = sessionRef.current;
      if (session === null) {
        // The first events can beat `startScan`'s own reply.
        if (starting.current) buffered.current = p;
        return;
      }
      if (p.session !== session) return;

      setTotals(p.totals);
      setScanInfo({
        phase: p.phase,
        running: p.finished === null,
        listed: p.listed,
        known: p.known,
        elapsedMs: p.finished?.elapsedMs ?? p.elapsedMs,
        focusSettled: p.focus?.settled ?? false,
        outcome: p.finished?.outcome ?? null,
        message: p.finished?.message ?? null,
      });

      const root = rootRef.current;
      if (!entered.current && root) {
        if (p.skeleton || p.finished?.outcome === "complete") {
          enter(root);
        } else if (p.finished) {
          // Ended before anything was listed: nothing to browse.
          const why = p.finished.message ?? "The scan was stopped before anything was listed.";
          reset();
          setFailure(why);
          setPhase("failed");
        }
      }
      if (entered.current && p.view) nav.applyLiveView(p.view);

      if (p.finished && !finishedSeen.current) {
        finishedSeen.current = true;
        const { outcome, message } = p.finished;
        if (outcome === "complete") {
          log(
            `[SCAN] ${formatNumber(p.totals.files)} files, ${formatNumber(p.totals.dirs)} folders.`,
          );
        } else {
          log(
            `[SCAN] ${outcome === "failed" ? "Failed" : "Stopped"}${message ? `: ${message}` : "."}`,
          );
          if (outcome === "failed" && phaseRef.current === "browsing") {
            notify(message ?? "The scan failed.", "error");
          }
        }
        // Sizes are final now (or as final as they will get): redraw everything.
        void analyses.refresh();
        nav.refreshTreemap();
        search.rerun();
      }
    },
    [analyses, enter, log, nav, notify, reset, search, sessionRef, setPhase, setTotals],
  );

  /** (Re)starts the scan on the connected phone. */
  const scan = useCallback(async () => {
    invalidate();
    const epoch = epochRef.current;
    setFailure(null);
    setStatus("Starting scan…");
    setPhase("connecting");
    starting.current = true;
    log("[SCAN] Starting…");
    try {
      const started = await commands.startScan();
      if (epoch !== epochRef.current) return;
      starting.current = false;
      rootRef.current = started.root;
      bind(started.session);
      setScanInfo({
        phase: "skeleton",
        running: true,
        listed: 0,
        known: 1,
        elapsedMs: 0,
        focusSettled: false,
        outcome: null,
        message: null,
      });
      const early = buffered.current;
      buffered.current = null;
      if (early && early.session === started.session) onProgress(early);
    } catch (err) {
      if (epoch !== epochRef.current) return;
      starting.current = false;
      const message = fail(err);
      setFailure(message);
      setPhase("failed");
    }
  }, [bind, fail, invalidate, log, onProgress, setPhase]);

  /** `serial` picks a specific device; omitted, the host chooses the only usable one. */
  const connect = useCallback(
    async (serial?: string) => {
      const epoch = ++epochRef.current;
      setFailure(null);
      setStatus("Connecting…");
      setPhase("connecting");
      try {
        log("[ADB] Looking for a device…");
        const info = await commands.connect(serial);
        if (epoch !== epochRef.current) return;
        // Captured once per connection, so a rescan still compares against the
        // previous session rather than against the scan a moment ago.
        setPrevious(info.previous);
        setModel(info.model);
        log(`[ADB] ${info.model} (${info.serial}).`);
        await scan();
      } catch (err) {
        if (epoch !== epochRef.current) return;
        fail(err);
        setPhase("setup");
      }
    },
    [fail, log, scan, setPhase],
  );

  const cancelScan = useCallback(() => {
    commands.cancelScan().catch(fail);
  }, [fail]);

  const disconnect = useCallback(async () => {
    // First, so replies to anything still running are dropped, including the
    // scan that this call is about to cancel.
    invalidate();
    log("[SOCKET] Stopping the daemon…");
    try {
      await commands.disconnect();
      notify("Disconnected", "info");
    } catch {
      log("[SOCKET] Daemon was already stopped.");
    }
    setFailure(null);
    setPrevious(null);
    setModel(null);
    setPhase("setup");
  }, [invalidate, log, notify, setPhase]);

  // The host's two lifecycle events. Handlers go through refs so the
  // subscriptions are made once and always call the latest closure.
  const progressRef = useRef(onProgress);
  const lostRef = useRef<(lost: ConnectionLost) => void>(() => {});
  useEffect(() => {
    progressRef.current = onProgress;
    lostRef.current = (lost) => {
      // Already back on the setup screen (the user disconnected): nothing to undo.
      if (phaseRef.current === "setup") return;
      invalidate();
      setFailure(null);
      setPrevious(null);
      setModel(null);
      setPhase("setup");
      notify(`Lost the connection to the phone. ${lost.message}`.trim(), "error");
      log(`[SOCKET] Connection to ${lost.serial} lost: ${lost.message}`);
    };
  });
  useEffect(() => {
    const progress = events.scanProgress((p) => progressRef.current(p));
    const lost = events.connectionLost((l) => lostRef.current(l));
    return () => {
      progress.then((off) => off());
      lost.then((off) => off());
    };
  }, []);

  return {
    phase,
    status,
    failure,
    scanInfo,
    previous,
    model,
    connect,
    disconnect,
    scan,
    cancelScan,
  };
}
