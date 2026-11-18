import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type Dispatch,
  type MutableRefObject,
  type SetStateAction,
} from "react";
import {
  commands,
  errorCode,
  errorMessage,
  events,
  type CopyProgress,
  type CopyResult,
  type NodeRef,
} from "../api";
import { formatBytes, formatNumber } from "../lib/format";
import { inSession, type Target } from "../lib/selection";
import { nodeKey } from "../lib/size";
import type { Notify, SessionDeps } from "./types";

/** A copy to this computer in flight. */
export interface CopyState {
  nodes: NodeRef[];
  /** What is being copied, for the bar: one name, or "3 items". */
  name: string;
  /** The latest `copy-progress`; null until the first arrives. */
  progress: CopyProgress | null;
  /** Cancel was asked for and the host has not stopped yet. */
  cancelling: boolean;
}

/**
 * "Copy to computer": one copy at a time, of one thing or several, by
 * `NodeRef`.
 *
 * The host chooses the destination (the desktop) and reports it in
 * `copy-progress` and in the result. `copy` holds the live progress for the
 * bar; the outcome is reported with a toast.
 */
export function useCopy({
  sessionRef,
  log,
  notify,
}: Pick<SessionDeps, "sessionRef" | "log"> & { notify: Notify }) {
  const [copy, setCopy] = useState<CopyState | null>(null);
  /** The copy's nodes, as one key, for matching `copy-progress` without a stale closure. */
  const copying = useRef<string | null>(null);
  useCopyProgress(copying, setCopy);

  /** Forgets a copy whose session is gone (the host stops it with the connection). */
  const reset = useCallback(() => {
    copying.current = null;
    setCopy(null);
  }, []);

  const start = useCallback(
    async (targets: Target[]) => {
      const nodes = targets.flatMap((t) => t.nodes);
      if (nodes.length === 0) return;
      if (!targets.every((t) => inSession(t, sessionRef.current))) {
        notify("Results are out of date — rescan.", "error");
        return;
      }
      if (copying.current) {
        notify("A copy is already running.", "info");
        return;
      }
      const key = keyOf(nodes);
      const name = targets.length === 1 ? targets[0].name : `${formatNumber(targets.length)} items`;
      copying.current = key;
      setCopy({ nodes, name, progress: null, cancelling: false });
      log(`[COPY] ${name}…`);
      try {
        const done = await commands.copyMany(nodes, null);
        notify(copiedMessage(name, done), "success");
        log(`[COPY] ${formatNumber(done.files)} files to ${done.destination}.`);
      } catch (err) {
        reportFailure(err, notify, log);
      } finally {
        if (copying.current === key) reset();
      }
    },
    [log, notify, reset, sessionRef],
  );

  const cancel = useCallback(() => {
    setCopy((c) => (c ? { ...c, cancelling: true } : c));
    commands.cancelCopy().catch((err) => log(`[ERROR] ${errorMessage(err)}`));
  }, [log]);

  return { copy, start, cancel, reset };
}

function keyOf(nodes: NodeRef[]): string {
  return nodes.map(nodeKey).join(",");
}

/** Feeds the host's `copy-progress` events into the copy in flight. */
function useCopyProgress(
  copying: MutableRefObject<string | null>,
  setCopy: Dispatch<SetStateAction<CopyState | null>>,
) {
  useEffect(() => {
    const pending = events.copyProgress((p) => {
      if (copying.current === null || keyOf(p.nodes) !== copying.current) return;
      setCopy((c) => (c ? { ...c, progress: p } : c));
    });
    return () => {
      pending.then((off) => off());
    };
  }, [copying, setCopy]);
}

function reportFailure(err: unknown, notify: Notify, log: (msg: string) => void) {
  if (errorCode(err) === "cancelled") {
    notify("Copy cancelled. Files already copied stay on this computer.", "info");
    log("[COPY] Cancelled.");
  } else {
    notify(`Copy failed: ${errorMessage(err)}`, "error");
    log(`[ERROR] ${errorMessage(err)}`);
  }
}

function copiedMessage(name: string, done: CopyResult): string {
  const notes: string[] = [];
  if (done.skipped > 0) notes.push(`${formatNumber(done.skipped)} links not copied`);
  if (done.renamed > 0) notes.push(`${formatNumber(done.renamed)} renamed`);
  return `Copied ${name} to ${done.destination}: ${formatNumber(done.files)} ${
    done.files === 1 ? "file" : "files"
  }, ${formatBytes(done.bytes)}${notes.length ? ` (${notes.join(", ")})` : ""}`;
}
