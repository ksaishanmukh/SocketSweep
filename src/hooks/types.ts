import type { MutableRefObject } from "react";
import type { SizeMetric } from "../api";

export type Notify = (msg: string, type?: "error" | "success" | "info") => void;

/**
 * What the session's smaller hooks share.
 *
 * The refs, not state, are what async code compares against: a reply that
 * began under an older session must be dropped, and a closure captured before
 * the await cannot know that the session has changed since.
 */
export interface SessionDeps {
  /** The live tree's session id, or null while there is none (setup, rescan in flight). */
  sessionRef: MutableRefObject<number | null>;
  /** The size the user chose to see; every query that takes one is passed this. */
  metricRef: MutableRefObject<SizeMetric>;
  log: (msg: string) => void;
  /** Reports a failed command to the user and the log; returns the message. */
  fail: (err: unknown) => string;
}
