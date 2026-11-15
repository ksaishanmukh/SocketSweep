import type { MutableRefObject } from "react";
import type { SizeMetric, ViewFilter, ViewSort } from "../api";

export type Notify = (msg: string, type?: "error" | "success" | "info") => void;

/** How the open folder's rows are asked for: their order and what they are narrowed to. */
export interface ViewOptions {
  sort: ViewSort;
  filter: ViewFilter;
}

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
  /** The list's sort and filter; every view query and `focus` is passed these. */
  optionsRef: MutableRefObject<ViewOptions>;
  log: (msg: string) => void;
  /** Reports a failed command to the user and the log; returns the message. */
  fail: (err: unknown) => string;
}
