import type { Measure, Size, SizeMetric, StagedItem } from "../api";
import type { Target } from "./selection";
import { nodeKey, sizeOf } from "./size";

/** A staged item as a delete target, for the confirmation dialog. */
export function targetOfStaged(item: StagedItem): Target {
  return {
    key: nodeKey(item.node),
    nodes: [item.node],
    name: item.name,
    path: item.path,
    size: item.size,
    measure: item.measure,
    files: item.files,
    kind: item.kind === "dir" ? "dir" : "file",
  };
}

/**
 * A size and what it becomes once the pending deletes are carried out, or
 * null when nothing staged lies inside it (or the size is not known yet, so a
 * difference would mean nothing).
 */
export function afterDelete(
  size: Size,
  projected: Size,
  measure: Measure,
  metric: SizeMetric,
): { now: number; after: number } | null {
  if (measure === "unknown") return null;
  const now = sizeOf(size, metric);
  const after = sizeOf(projected, metric);
  return after === now ? null : { now, after };
}
