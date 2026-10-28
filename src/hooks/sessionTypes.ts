import type { MutableRefObject } from "react";
import type { NodeRef, ScanOutcome, ScanPhase, ScanProgress, Totals } from "../api";
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

/** What the session's other hooks provide to the lifecycle. */
export interface TreeDeps extends SessionDeps {
  notify: Notify;
  setTotals: (totals: Totals | null) => void;
  nav: ReturnType<typeof useNavigation>;
  analyses: ReturnType<typeof useAnalyses>;
  search: ReturnType<typeof useSearch>;
  /** Forgets everything that holds nodes of the tree being replaced or dropped. */
  reset: () => void;
}

/** The lifecycle's mutable bookkeeping, shared by the hooks that split it up. */
export interface SessionTracking {
  phase: Phase;
  /** Bumped whenever a pending reply should be dropped; covers the stretch before a session id exists. */
  epoch: number;
  root: NodeRef | null;
  /** True from the call to `startScan` until it answers; events before then belong to no known session. */
  starting: boolean;
  buffered: ScanProgress | null;
  entered: boolean;
  finishedSeen: boolean;
}

/** What the progress handlers need on top of {@link TreeDeps}. */
export interface ProgressDeps extends TreeDeps {
  trackingRef: MutableRefObject<SessionTracking>;
  setScanInfo: (info: ScanInfo | null) => void;
  setPhase: (phase: Phase) => void;
  setFailure: (message: string | null) => void;
}
