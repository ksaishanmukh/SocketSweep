import { useCallback, useEffect, useRef, useState } from "react";
import {
  commands,
  events,
  type NodeRef,
  type StagedChanged,
  type StagedSummary,
  type StagedTotals,
} from "../api";
import { plural } from "../lib/format";
import { inSession, type Target } from "../lib/selection";
import type { Notify, SessionDeps } from "./types";
import type { Selection } from "./useSelection";

const EMPTY: StagedTotals = {
  items: 0,
  size: { apparent: 0, disk: 0 },
  files: 0,
  measure: "final",
};

interface Deps extends Pick<SessionDeps, "sessionRef" | "log" | "fail"> {
  notify: Notify;
  selection: Selection;
  /** Re-reads what is on screen, whose `staged` and `projected` values changed. */
  refresh: () => void;
}

/** The totals, tagged with the tree they describe, so another tree's are never shown. */
interface Tagged {
  session: number;
  totals: StagedTotals;
}

/**
 * The pending-delete queue, kept on the host.
 *
 * Nothing here deletes anything: staging only marks nodes, and every view
 * the host returns says what is marked and what sizes would be left. This
 * hook holds the queue's totals for the bar, runs the staging commands, and
 * re-reads the screen when the queue changes, whether by the user or by the
 * host (a scan removing a staged folder). The queue belongs to one session:
 * totals of any other are ignored, and the host drops the queue itself on a
 * rescan or disconnect.
 */
export function useStaged(deps: Deps, sessionId: number | null) {
  const { sessionRef, log, fail, notify, selection, refresh } = deps;
  const [tagged, setTagged] = useState<Tagged | null>(null);
  const totals = tagged && tagged.session === sessionId ? tagged.totals : EMPTY;
  const totalsRef = useRef(totals);
  useEffect(() => {
    totalsRef.current = totals;
  });

  const apply = useCallback((session: number, next: StagedTotals) => {
    totalsRef.current = next;
    setTagged({ session, totals: next });
  }, []);

  useStagedChanged(sessionRef, totalsRef, apply, refresh);

  /** Runs a staging command for the current session, then applies its answer. */
  const run = useCallback(
    async (call: () => Promise<StagedTotals>): Promise<boolean> => {
      const session = sessionRef.current;
      if (session === null) return false;
      try {
        const next = await call();
        if (session !== sessionRef.current) return false;
        apply(session, next);
        refresh();
        return true;
      } catch (err) {
        fail(err);
        refresh();
        return false;
      }
    },
    [apply, fail, refresh, sessionRef],
  );

  /** Queues the targets and lets go of their picks. `false` if the host refused. */
  const stage = useCallback(
    async (targets: Target[]): Promise<boolean> => {
      const nodes = targets.flatMap((t) => t.nodes);
      if (nodes.length === 0) return false;
      if (!targets.every((t) => inSession(t, sessionRef.current))) {
        notify("Results are out of date — rescan.", "error");
        return false;
      }
      log(`[STAGE] ${plural(targets.length, "item")} marked for deletion.`);
      const staged = await run(() => commands.stageDelete(nodes));
      if (staged) selection.forget(nodes);
      return staged;
    },
    [log, notify, run, selection, sessionRef],
  );

  const unstage = useCallback(
    (nodes: NodeRef[]) => run(() => commands.unstageDelete(nodes)),
    [run],
  );

  const clearQueue = useCallback(() => run(() => commands.clearStaged()), [run]);

  /** The queue as a list, or null if there is no tree or the host failed. */
  const summary = useCallback(async (): Promise<StagedSummary | null> => {
    const session = sessionRef.current;
    if (session === null) return null;
    try {
      const next = await commands.stagedSummary();
      return session === sessionRef.current ? next : null;
    } catch {
      return null;
    }
  }, [sessionRef]);

  /** Re-reads the totals alone: a staged folder's size grows while the scan measures it. */
  const refreshTotals = useCallback(async () => {
    const session = sessionRef.current;
    if (session === null || totalsRef.current.items === 0) return;
    const next = await summary();
    if (next && session === sessionRef.current) apply(session, next.totals);
  }, [apply, sessionRef, summary]);

  /** Forgets the queue with its tree. */
  const forget = useCallback(() => setTagged(null), []);

  return { totals, stage, unstage, clearQueue, summary, refreshTotals, forget };
}

/**
 * The host says the queue changed. The user's own changes were already
 * applied from the command's answer; this catches the host's (a scan or a
 * delete removed a staged node), so it re-reads the screen only when the
 * totals differ from what is held.
 */
function useStagedChanged(
  sessionRef: Deps["sessionRef"],
  totalsRef: { current: StagedTotals },
  apply: (session: number, totals: StagedTotals) => void,
  refresh: () => void,
) {
  const handler = useRef<(c: StagedChanged) => void>(() => {});
  useEffect(() => {
    handler.current = (changed) => {
      if (changed.session !== sessionRef.current) return;
      const known = JSON.stringify(totalsRef.current) === JSON.stringify(changed.totals);
      apply(changed.session, changed.totals);
      if (!known) refresh();
    };
  });
  useEffect(() => {
    const off = events.stagedChanged((c) => handler.current(c));
    return () => {
      void off.then((stop) => stop());
    };
  }, []);
}
