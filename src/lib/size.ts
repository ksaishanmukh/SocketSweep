import type { NodeRef, Size, SizeMetric } from "../api";

/** The number a [`Size`] shows under the chosen way of counting. */
export function sizeOf(size: Size, metric: SizeMetric): number {
  return size[metric];
}

export const METRIC_LABEL: Record<SizeMetric, string> = {
  disk: "Disk usage",
  apparent: "File size",
};

export const METRIC_HINT: Record<SizeMetric, string> = {
  disk: "Space allocated on the phone: what deleting frees",
  apparent: "Sum of file lengths: what a file manager shows",
};

/** Two references name the same node only if they come from the same session too. */
export function sameNode(a: NodeRef | null | undefined, b: NodeRef | null | undefined): boolean {
  return !!a && !!b && a.session === b.session && a.id === b.id;
}

/** A stable React key for a node. */
export function nodeKey(node: NodeRef): string {
  return `${node.session}:${node.id}`;
}
