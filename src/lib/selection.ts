import type { Leftover, Measure, NodeRef, Row, Size, SizeMetric } from "../api";
import { isWithin, joinPath } from "./path";
import { nodeKey, sizeOf } from "./size";

/** Above either of these, the delete dialog wants DELETE typed rather than a click. */
export const TYPE_TO_CONFIRM_BYTES = 1024 ** 3;
export const TYPE_TO_CONFIRM_FILES = 100;

/**
 * Something picked for a batch action: a file, a folder, or every folder an
 * uninstalled app left behind. Captured when picked, because the rows it came
 * from keep changing while the scan runs, and its nodes carry their session,
 * so a pick from a replaced tree can be recognised and dropped.
 */
export interface Target {
  key: string;
  nodes: NodeRef[];
  name: string;
  /** Full path on the phone, for the confirmation; null when there is no single one. */
  path: string | null;
  size: Size;
  measure: Measure;
  /** Files inside; null when not known. */
  files: number | null;
  kind: "file" | "dir" | "leftover";
}

export function targetOfRow(row: Row, folderPath: string): Target {
  return {
    key: nodeKey(row.node),
    nodes: [row.node],
    name: row.name,
    path: joinPath(row.parent ?? folderPath, row.name),
    size: row.size,
    measure: row.measure,
    files: row.files,
    kind: row.kind === "dir" ? "dir" : "file",
  };
}

export function targetOfLeftover(leftover: Leftover): Target {
  const first = leftover.folders[0];
  return {
    key: `leftover:${first ? nodeKey(first) : leftover.package}`,
    nodes: leftover.folders,
    name: leftover.package,
    path: null,
    size: leftover.size,
    measure: "final",
    files: null,
    kind: "leftover",
  };
}

export function inSession(target: Target, session: number | null): boolean {
  return session !== null && target.nodes.every((n) => n.session === session);
}

/**
 * Targets that are not inside another picked folder. The host deletes or
 * copies a nested pick along with its folder, so counting it again would
 * overstate the total.
 */
export function outermost(targets: Target[]): Target[] {
  const folders = targets.filter((t) => t.kind === "dir" && t.path !== null);
  return targets.filter((t) => {
    const path = t.path;
    return path === null || !folders.some((f) => f !== t && isWithin(path, f.path!));
  });
}

export interface Totals {
  bytes: number;
  files: number;
  /** Some size is a lower bound or not known yet. */
  uncertain: boolean;
}

export function totalOf(targets: Target[], metric: SizeMetric): Totals {
  let bytes = 0;
  let files = 0;
  let uncertain = false;
  for (const t of outermost(targets)) {
    bytes += sizeOf(t.size, metric);
    files += t.files ?? 0;
    if (t.measure !== "final") uncertain = true;
  }
  return { bytes, files, uncertain };
}

/** A big batch, or one that may hold more than it shows, is confirmed by typing rather than clicking. */
export function needsTypedConfirm(targets: Target[]): boolean {
  const total = totalOf(targets, "disk");
  return (
    total.uncertain || total.bytes >= TYPE_TO_CONFIRM_BYTES || total.files >= TYPE_TO_CONFIRM_FILES
  );
}
