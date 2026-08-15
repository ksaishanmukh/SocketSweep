import { useEffect, useMemo, useRef, useState } from "react";
import { formatBytes } from "../lib/format";
import { squarifyNested, type Tile, type TreemapItem } from "../lib/squarify";

/**
 * Nested squarified treemap in plain SVG.
 *
 * Replaces Recharts, which was most of the JS bundle for one flat treemap it
 * could not nest, driven through an untyped content renderer. Owning the layout
 * also means we control how it behaves while sizes are still changing during a
 * scan, which was going to be a problem either way.
 */

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
}

export function Treemap({ items, onOpen, onSelect, selectedId }: TreemapProps) {
  const hostRef = useRef<HTMLDivElement>(null);
  const [size, setSize] = useState({ w: 0, h: 0 });
  const [hovered, setHovered] = useState<Tile | null>(null);

  // ResponsiveContainer's job, in six lines and without the dependency.
  useEffect(() => {
    const el = hostRef.current;
    if (!el) return;
    const ro = new ResizeObserver(([entry]) => {
      const { width, height } = entry.contentRect;
      setSize({ w: Math.floor(width), h: Math.floor(height) });
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  const tiles = useMemo(
    () =>
      size.w > 0 && size.h > 0
        ? squarifyNested(items, size.w, size.h, { headerHeight: HEADER_H, maxDepth: 1 })
        : [],
    [items, size.w, size.h],
  );

  // Colour by the top-level ancestor so children read as part of their parent.
  const colorOf = useMemo(() => {
    const map = new Map<number, string>();
    let next = 0;
    for (const t of tiles) {
      if (t.depth === 0) map.set(t.id, PALETTE[next++ % PALETTE.length]);
    }
    return map;
  }, [tiles]);

  return (
    <div ref={hostRef} className="relative w-full h-full">
      {tiles.length === 0 ? (
        <div className="flex items-center justify-center h-full text-sm text-zinc-600">
          Nothing to show here.
        </div>
      ) : (
        <svg
          width={size.w}
          height={size.h}
          className="block"
          role="img"
          aria-label="Storage treemap"
        >
          {tiles.map((tile, i) => {
            const parentColor =
              tile.depth === 0 ? colorOf.get(tile.id) : findAncestorColor(tiles, i, colorOf);
            const selected = tile.id === selectedId;

            return (
              <g
                key={`${tile.id}-${tile.depth}`}
                onMouseEnter={() => setHovered(tile)}
                onMouseLeave={() => setHovered((h) => (h?.id === tile.id ? null : h))}
                onClick={(e) => {
                  e.stopPropagation();
                  onSelect?.(tile.id);
                }}
                onDoubleClick={(e) => {
                  e.stopPropagation();
                  onOpen(tile.id, tile.isDir);
                }}
                className="cursor-pointer"
              >
                <rect
                  x={tile.x}
                  y={tile.y}
                  width={Math.max(0, tile.w)}
                  height={Math.max(0, tile.h)}
                  fill={parentColor ?? "#3f3f46"}
                  fillOpacity={tile.depth === 0 ? 0.85 : 0.55}
                  stroke={selected ? "#fff" : "#ffffff22"}
                  strokeWidth={selected ? 2 : 1}
                />
                {tile.w > 54 && tile.h > 24 && (
                  <text
                    x={tile.x + 5}
                    y={tile.y + 12}
                    fill="#fff"
                    fontSize={11}
                    className="pointer-events-none select-none"
                  >
                    {clip(tile.name, tile.w - 10)}
                  </text>
                )}
                {tile.depth === 0 && tile.w > 54 && tile.h > 42 && (
                  <text
                    x={tile.x + 5}
                    y={tile.y + 26}
                    fill="#e4e4e7"
                    fontSize={9}
                    fillOpacity={0.75}
                    className="pointer-events-none select-none font-mono"
                  >
                    {formatBytes(tile.size)}
                  </text>
                )}
              </g>
            );
          })}
        </svg>
      )}

      {hovered && (
        <div
          className="pointer-events-none absolute z-10 bg-zinc-900 border border-zinc-700 px-2 py-1.5 rounded shadow-xl text-xs"
          style={{
            left: Math.min(hovered.x + 8, Math.max(0, size.w - 200)),
            top: Math.min(hovered.y + 8, Math.max(0, size.h - 64)),
          }}
        >
          <p className="font-semibold text-zinc-200 max-w-48 truncate">{hovered.name}</p>
          <p className="text-zinc-400 font-mono mt-0.5">{formatBytes(hovered.size)}</p>
          {hovered.isDir && <p className="text-zinc-600 mt-0.5">Double-click to open</p>}
        </div>
      )}
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

/** Rough character budget; SVG has no ellipsis and measuring per frame is not worth it. */
function clip(name: string, pixels: number): string {
  const max = Math.floor(pixels / 6.2);
  return name.length <= max ? name : `${name.slice(0, Math.max(1, max - 1))}…`;
}
