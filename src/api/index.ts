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
  CommandError,
  Connected,
  ConnectionLost,
  CopyProgress,
  CopyResult,
  Crumb,
  Deleted,
  Device,
  DeviceState,
  EntryKind,
  ErrorCode,
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

import type { CommandError, ErrorCode } from "../bindings";

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
