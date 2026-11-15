import type { NodeRef, SizeMetric, TreemapNode, View } from "../api";
import { nodeKey, sizeOf } from "./size";
import type { TreemapItem } from "./squarify";

/** A tile needs this much room before its name fits without crowding the edges. */
const NAME_MIN = { w: 56, h: 24 };
/** And this much before a second line, the size, fits under the name. */
const SIZE_MIN = { w: 70, h: 46 };

/**
 * The open folder's children worth a tile. A node not measured yet has no
 * area to occupy (its 0 is not "empty"), and an empty one has none either.
 */
export function mapItems(
  node: TreemapNode | null,
  metric: SizeMetric,
): { items: TreemapItem[]; byId: Map<number, TreemapNode> } {
  const byId = new Map<number, TreemapNode>();
  const items: TreemapItem[] = [];
  for (const child of node?.children ?? []) {
    const bytes = sizeOf(child.size, metric);
    if (child.measure === "unknown" || bytes <= 0) continue;
    byId.set(child.node.id, child);
    items.push({ id: child.node.id, name: child.name, size: bytes, isDir: child.kind === "dir" });
  }
  return { items, byId };
}

/** Which labels a tile of this size shows. Each is only drawn where it fits, so labels never overlap. */
export function tileLabels(w: number, h: number): { name: boolean; size: boolean } {
  const name = w > NAME_MIN.w && h > NAME_MIN.h;
  return { name, size: name && w > SIZE_MIN.w && h > SIZE_MIN.h };
}

/**
 * Whether a tile should fade because the list's filters leave it out.
 *
 * The rows on screen are the host's answer, so a tile missing from them is
 * filtered out, but only once every match has been loaded; while more are
 * still to come a missing tile may just be further down, and stays lit.
 */
export function fadedTiles(view: View, filtering: boolean): (node: NodeRef) => boolean {
  if (!filtering || view.hidden > 0) return () => false;
  const shown = new Set(view.rows.map((r) => nodeKey(r.node)));
  return (node) => !shown.has(nodeKey(node));
}
