import { formatBytes } from "../lib/format";
import type { Tile } from "../lib/squarify";

export function TreemapTile({
  tile,
  color,
  selected,
  counting,
  onOpen,
  onSelect,
  onHoverStart,
  onHoverEnd,
}: {
  tile: Tile;
  /** The top-level ancestor's colour, so a subtree reads as one block. */
  color: string | undefined;
  selected: boolean;
  /** The size is still a growing lower bound. */
  counting: boolean;
  onOpen: (id: number, isDir: boolean) => void;
  onSelect?: (id: number) => void;
  onHoverStart: (tile: Tile) => void;
  onHoverEnd: (tile: Tile) => void;
}) {
  return (
    <g
      onMouseEnter={() => onHoverStart(tile)}
      onMouseLeave={() => onHoverEnd(tile)}
      onClick={(e) => {
        e.stopPropagation();
        onSelect?.(tile.id);
      }}
      onDoubleClick={(e) => {
        e.stopPropagation();
        onOpen(tile.id, tile.isDir);
      }}
      onKeyDown={(e) => {
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          e.stopPropagation();
          onOpen(tile.id, tile.isDir);
        }
      }}
      onFocus={() => onSelect?.(tile.id)}
      tabIndex={0}
      role="button"
      aria-label={`${tile.name}, ${formatBytes(tile.size)}${counting ? ", still counting" : ""}`}
      className="treemap-tile cursor-pointer"
    >
      <rect
        x={tile.x}
        y={tile.y}
        width={Math.max(0, tile.w)}
        height={Math.max(0, tile.h)}
        fill={color ?? "#3f3f46"}
        fillOpacity={tile.depth === 0 ? 0.85 : 0.55}
        stroke={selected ? "#fff" : "#ffffff22"}
        strokeWidth={selected ? 2 : 1}
      />
      <TileLabels tile={tile} counting={counting} />
    </g>
  );
}

function TileLabels({ tile, counting }: { tile: Tile; counting: boolean }) {
  return (
    <>
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
          {counting ? "+" : ""}
        </text>
      )}
    </>
  );
}

/** Rough character budget; SVG has no ellipsis and measuring per frame is not worth it. */
function clip(name: string, pixels: number): string {
  const max = Math.floor(pixels / 6.2);
  return name.length <= max ? name : `${name.slice(0, Math.max(1, max - 1))}…`;
}
