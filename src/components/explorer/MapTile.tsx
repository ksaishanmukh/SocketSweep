import type { CSSProperties } from "react";
import type { SizeMetric, TreemapNode } from "../../api";
import { categoryTint } from "../../lib/categories";
import { formatBytes } from "../../lib/format";
import { tileLabels } from "../../lib/mapTiles";
import { nodeKey, sizeOf } from "../../lib/size";
import type { Tile } from "../../lib/squarify";
import type { Hover, SetHover } from "./hover";

/** Half the gap between tiles, so neighbours never touch. */
const INSET = 1;

/**
 * One child of the open folder, tinted by its type. A folder opens on click;
 * a file is there to be seen and pointed at.
 */
export function MapTile({
  tile,
  node,
  metric,
  faded,
  hover,
  onHover,
  onOpen,
}: {
  tile: Tile;
  node: TreemapNode;
  metric: SizeMetric;
  faded: boolean;
  hover: Hover | null;
  onHover: SetHover;
  onOpen: (node: TreemapNode) => void;
}) {
  const key = nodeKey(node.node);
  const pointed = hover?.key !== key ? null : hover.from === "map" ? "strong" : "hint";
  const size = formatBytes(sizeOf(node.size, metric));
  const isDir = node.kind === "dir";
  const counting = node.measure === "partial";
  const labels = tileLabels(tile.w, tile.h);
  return (
    <button
      type="button"
      className="map-tile absolute overflow-hidden rounded text-left text-fg px-1.5 py-1"
      style={tileStyle(tile, node, faded, pointed)}
      data-pointed={pointed ?? undefined}
      aria-label={`${node.name}, ${counting ? "at least " : ""}${size}${isDir ? ", folder" : ""}`}
      title={`${node.name} · ${size}${counting ? " (still counting)" : ""}`}
      data-faded={faded || undefined}
      onPointerEnter={() => onHover({ key, from: "map" })}
      onPointerLeave={() => onHover(null)}
      onFocus={() => onHover({ key, from: "map" })}
      onBlur={() => onHover(null)}
      onClick={() => isDir && onOpen(node)}
    >
      {labels.name && (
        <span className="block truncate text-[12px] font-medium leading-4">{node.name}</span>
      )}
      {labels.size && (
        <span className="block truncate num text-[11px] leading-4 text-muted">
          {size}
          {counting ? "+" : ""}
        </span>
      )}
    </button>
  );
}

/**
 * Pointed at from the map, a tile is lit up and outlined; from its list row,
 * it only gets a faint inner line, so moving down the list does not make the
 * map flicker.
 */
function tileStyle(
  tile: Tile,
  node: TreemapNode,
  faded: boolean,
  pointed: "strong" | "hint" | null,
): CSSProperties {
  return {
    left: tile.x + INSET,
    top: tile.y + INSET,
    width: Math.max(0, tile.w - 2 * INSET),
    height: Math.max(0, tile.h - 2 * INSET),
    background: categoryTint(node.category, pointed === "strong" ? 48 : 26),
    opacity: faded ? 0.22 : 1,
    outline: pointed === "strong" ? "2px solid var(--text)" : undefined,
    outlineOffset: -2,
    boxShadow: pointed === "hint" ? "inset 0 0 0 1px var(--muted)" : undefined,
    cursor: node.kind === "dir" ? "pointer" : "default",
  };
}
