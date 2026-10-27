import type { SizeMetric, TreemapNode } from "../api";
import { sizeOf } from "./size";
import type { TreemapItem } from "./squarify";

/**
 * Tiles only for nodes with a size to draw: a node that is not measured yet
 * has no area to occupy (it would be 0, which is not the same as empty), and
 * squarify drops zero-sized items anyway. Partial ones are returned in
 * `counting` so the treemap can mark them as lower bounds.
 */
export function toItems(
  node: TreemapNode | null,
  metric: SizeMetric,
): { items: TreemapItem[]; counting: Set<number> } {
  const counting = new Set<number>();
  const map = (n: TreemapNode): TreemapItem => {
    if (n.measure === "partial") counting.add(n.node.id);
    const kids = n.children.filter((c) => c.measure !== "unknown" && sizeOf(c.size, metric) > 0);
    return {
      id: n.node.id,
      name: n.name,
      size: sizeOf(n.size, metric),
      isDir: n.kind === "dir",
      children: kids.length ? kids.map(map) : undefined,
    };
  };
  const items = node
    ? node.children.filter((c) => c.measure !== "unknown" && sizeOf(c.size, metric) > 0).map(map)
    : [];
  return { items, counting };
}
