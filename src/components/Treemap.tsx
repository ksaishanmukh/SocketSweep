import { useMemo, useRef, useState } from "react";
import { useElementSize } from "../hooks/useElementSize";
import { formatBytes } from "../lib/format";
import { squarifyNested, type Tile, type TreemapItem } from "../lib/squarify";
import { TreemapTile } from "./TreemapTile";

/** Nested squarified treemap in plain SVG. */

/** Distinct hues, assigned by top-level tile so a subtree reads as one block. */
const PALETTE = [
  "#0d9488",
  "#0284c7",
  "#4f46e5",
  "#7c3aed",
  "#c026d3",
  "#e11d48",
  "#ea580c",
  "#ca8a04",
];

const HEADER_H = 16;

export interface TreemapProps {
  items: TreemapItem[];
  onOpen: (id: number, isDir: boolean) => void;
  onSelect?: (id: number) => void;
  selectedId?: number;
  /** Tiles whose size is still a growing lower bound, marked with a "+". */
  counting?: ReadonlySet<number>;
  emptyMessage?: string;
}

export function Treemap({
  items,
  onOpen,
  onSelect,
  selectedId,
  counting,
  emptyMessage = "Nothing to show here.",
}: TreemapProps) {
  const hostRef = useRef<HTMLDivElement>(null);
  const size = useElementSize(hostRef);
  // Remembering which items the tile came from drops a stale tooltip after drilling in.
  const [hover, setHover] = useState<{ tile: Tile; items: TreemapItem[] } | null>(null);
  const hovered = hover?.items === items ? hover.tile : null;

  const tiles = useMemo(
    () =>
      size.w > 0 && size.h > 0
        ? squarifyNested(items, size.w, size.h, { headerHeight: HEADER_H, maxDepth: 1 })
        : [],
    [items, size.w, size.h],
  );
  const colorOf = useTopLevelColors(tiles);

  return (
    <div ref={hostRef} className="relative w-full h-full">
      {tiles.length === 0 ? (
        <div className="flex items-center justify-center h-full text-sm text-zinc-600">
          {emptyMessage}
        </div>
      ) : (
        <svg
          width={size.w}
          height={size.h}
          className="block"
          role="group"
          aria-label="Storage treemap"
        >
          {tiles.map((tile, i) => (
            <TreemapTile
              key={`${tile.id}-${tile.depth}`}
              tile={tile}
              color={tile.depth === 0 ? colorOf.get(tile.id) : findAncestorColor(tiles, i, colorOf)}
              selected={tile.id === selectedId}
              counting={counting?.has(tile.id) ?? false}
              onOpen={onOpen}
              onSelect={onSelect}
              onHoverStart={(t) => setHover({ tile: t, items })}
              onHoverEnd={(t) => setHover((h) => (h?.tile.id === t.id ? null : h))}
            />
          ))}
        </svg>
      )}

      {hovered && (
        <TreemapTooltip tile={hovered} counting={counting?.has(hovered.id) ?? false} size={size} />
      )}
    </div>
  );
}

/** Colour by the top-level ancestor so children read as part of their parent. */
function useTopLevelColors(tiles: Tile[]) {
  return useMemo(() => {
    const map = new Map<number, string>();
    let next = 0;
    for (const t of tiles) {
      if (t.depth === 0) map.set(t.id, PALETTE[next++ % PALETTE.length]);
    }
    return map;
  }, [tiles]);
}

function TreemapTooltip({
  tile,
  counting,
  size,
}: {
  tile: Tile;
  counting: boolean;
  size: { w: number; h: number };
}) {
  return (
    <div
      className="pointer-events-none absolute z-10 bg-zinc-900 border border-zinc-700 px-2 py-1.5 rounded shadow-xl text-xs"
      style={{
        left: Math.min(tile.x + 8, Math.max(0, size.w - 200)),
        top: Math.min(tile.y + 8, Math.max(0, size.h - 64)),
      }}
    >
      <p className="font-semibold text-zinc-200 max-w-48 truncate">{tile.name}</p>
      <p className="text-zinc-400 font-mono mt-0.5">
        {formatBytes(tile.size)}
        {counting ? "+" : ""}
      </p>
      {counting && <p className="text-amber-400/80 mt-0.5">Still counting</p>}
      {tile.isDir && <p className="text-zinc-600 mt-0.5">Double-click to open</p>}
    </div>
  );
}

/**
 * Tiles come out of the layout in parent-then-children order, so the nearest
 * preceding depth-0 tile is this tile's top-level ancestor.
 */
function findAncestorColor(
  tiles: Tile[],
  index: number,
  colors: Map<number, string>,
): string | undefined {
  for (let i = index; i >= 0; i--) {
    if (tiles[i].depth === 0) return colors.get(tiles[i].id);
  }
  return undefined;
}
