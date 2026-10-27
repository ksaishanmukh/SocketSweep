/**
 * The GUI's only door to the host.
 *
 * Everything that talks to the Rust side goes through here: the typed
 * `commands`, the `events` listeners and every contract type, all generated
 * into `src/bindings.ts`. Nothing else may import that file (ESLint enforces
 * it), which is what lets tests replace this one module with a mock.
 */

export { commands, events } from "../bindings";
export type {
  AppUsage,
  Connected,
  ConnectionLost,
  CopyProgress,
  CopyResult,
  Crumb,
  Deleted,
  Device,
  DeviceState,
  EntryKind,
  FocusState,
  Measure,
  NodeRef,
  Row,
  ScanFinished,
  ScanOutcome,
  ScanPhase,
  ScanProgress,
  ScanRecord,
  ScanStarted,
  Size,
  SizeMetric,
  SkeletonReady,
  Totals,
  TreemapNode,
  TypeGroup,
  View,
} from "../bindings";

/**
 * How every command fails (`ErrorCode` and `CommandError` in
 * `crates/gui-contract`).
 *
 * These two types are declared by hand because the generated bindings do not
 * export them: specta only emits types reachable from a command's arguments
 * or result, and the error type appears only in prose. Keep them in step with
 * `ErrorCode` in the contract crate.
 */
export type ErrorCode =
  | "notConnected"
  | "noScan"
  | "stale"
  | "removed"
  | "busy"
  | "device"
  | "daemon"
  | "cancelled"
  | "invalid"
  | "io";

export interface CommandError {
  code: ErrorCode;
  /** For the user, already worded. */
  message: string;
}

export function isCommandError(err: unknown): err is CommandError {
  return (
    typeof err === "object" &&
    err !== null &&
    typeof (err as CommandError).code === "string" &&
    typeof (err as CommandError).message === "string"
  );
}

/** The contract's error code, or null for anything that is not a `CommandError`. */
export function errorCode(err: unknown): ErrorCode | null {
  return isCommandError(err) ? err.code : null;
}

/** Text to show a person for whatever a command rejected with. */
export function errorMessage(err: unknown): string {
  if (isCommandError(err)) return err.message;
  if (typeof err === "string") return err;
  if (err instanceof Error) return err.message;
  return String(err);
}
