import { useCallback, useEffect, useRef, useState } from "react";
import {
  commands,
  errorCode,
  errorMessage,
  events,
  type CopyProgress,
  type NodeRef,
  type Row,
} from "../api";
import { formatBytes, formatNumber } from "../lib/format";
import { sameNode } from "../lib/size";
import type { Notify, SessionDeps } from "./types";

/** A copy to this computer in flight. */
export interface CopyState {
  node: NodeRef;
  name: string;
  /** The latest `copy-progress`; null until the first arrives. */
  progress: CopyProgress | null;
  /** Cancel was asked for and the host has not stopped yet. */
  cancelling: boolean;
}

/**
 * "Copy to computer": one copy at a time, by `NodeRef`.
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
  /** The node being copied, for matching `copy-progress` without a stale closure. */
  const copying = useRef<NodeRef | null>(null);

  useEffect(() => {
    const pending = events.copyProgress((p) => {
      if (!sameNode(p.node, copying.current)) return;
      setCopy((c) => (c ? { ...c, progress: p } : c));
    });
    return () => {
      pending.then((off) => off());
    };
  }, []);

  /** Forgets a copy whose session is gone (the host stops it with the connection). */
  const reset = useCallback(() => {
    copying.current = null;
    setCopy(null);
  }, []);

  const start = useCallback(
    async (row: Row) => {
      const session = sessionRef.current;
      if (session === null || row.node.session !== session) {
        notify("Results are out of date — rescan.", "error");
        return;
      }
      if (copying.current) {
        notify("A copy is already running.", "info");
        return;
      }
      copying.current = row.node;
      setCopy({ node: row.node, name: row.name, progress: null, cancelling: false });
      log(`[COPY] ${row.name}…`);
      try {
        const done = await commands.copyToDesktop(row.node);
        const notes: string[] = [];
        if (done.skipped > 0) notes.push(`${formatNumber(done.skipped)} links not copied`);
        if (done.renamed > 0) notes.push(`${formatNumber(done.renamed)} renamed`);
        notify(
          `Copied ${row.name} to ${done.destination}: ${formatNumber(done.files)} ${
            done.files === 1 ? "file" : "files"
          }, ${formatBytes(done.bytes)}${notes.length ? ` (${notes.join(", ")})` : ""}`,
          "success",
        );
        log(`[COPY] ${formatNumber(done.files)} files to ${done.destination}.`);
      } catch (err) {
        if (errorCode(err) === "cancelled") {
          notify("Copy cancelled. Files already copied stay on this computer.", "info");
          log("[COPY] Cancelled.");
        } else {
          notify(`Copy failed: ${errorMessage(err)}`, "error");
          log(`[ERROR] ${errorMessage(err)}`);
        }
      } finally {
        if (sameNode(copying.current, row.node)) reset();
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
