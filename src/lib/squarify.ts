/**
 * Squarified treemap layout.
 *
 * Bruls, Huizing & van Wijk (2000). Lays items out in rows along the shorter
 * side of the remaining rectangle, extending a row while doing so improves its
 * worst aspect ratio. The result is tiles close to square, which are far easier
 * to compare by eye than the long slivers a naive slice-and-dice produces.
 *
 * A pure function, so its behaviour under the continuously changing sizes a
 * running scan produces is testable rather than incidental.
 */

export interface TreemapItem {
  id: number;
  name: string;
  size: number;
  isDir: boolean;
  children?: TreemapItem[];
}

interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

export interface Tile extends Rect {
  id: number;
  name: string;
  size: number;
  isDir: boolean;
  /** 0 for top-level tiles, 1 for their children, and so on. */
  depth: number;
  children?: TreemapItem[];
}

/**
 * Lay `items` out inside a `width` x `height` rectangle.
 *
 * Items with a non-positive size are dropped, having no area to occupy. Ties
 * break by name so the layout stays stable across the ~10 refreshes per second
 * a running scan produces.
 */
export function squarify(items: TreemapItem[], width: number, height: number, depth = 0): Tile[] {
  if (width <= 0 || height <= 0) return [];

  const sorted = items
    .filter((i) => i.size > 0)
    .sort((a, b) => b.size - a.size || a.name.localeCompare(b.name));
  if (sorted.length === 0) return [];

  const total = sorted.reduce((sum, i) => sum + i.size, 0);
  const scale = (width * height) / total;

  const tiles: Tile[] = [];
  // The rectangle still to be filled; shrinks as each row is placed.
  let free: Rect = { x: 0, y: 0, w: width, h: height };

  let i = 0;
  while (i < sorted.length) {
    const side = Math.min(free.w, free.h);
    if (side <= 0) break;

    const row = growRow(sorted, i, scale, side);
    const placed = placeRow(sorted, i, row, { free, side, scale, depth });
    tiles.push(...placed.tiles);
    free = placed.rest;
    i = row.end;
  }

  return tiles;
}

/**
 * Tiles big enough to be worth drawing children inside, laid out recursively.
 *
 * A tile only nests if it is large enough that the children would be legible
 * and it has a header's worth of room to spare.
 */
export function squarifyNested(
  items: TreemapItem[],
  width: number,
  height: number,
  opts: { minNestSize?: number; headerHeight?: number; maxDepth?: number } = {},
): Tile[] {
  const { minNestSize = 90, headerHeight = 16, maxDepth = 1 } = opts;

  const out: Tile[] = [];
  const walk = (list: TreemapItem[], rect: Rect, depth: number) => {
    const tiles = squarify(list, rect.w, rect.h, depth);

    for (const tile of tiles) {
      const placed: Tile = { ...tile, x: tile.x + rect.x, y: tile.y + rect.y };
      out.push(placed);

      const canNest =
        depth < maxDepth &&
        placed.children?.length &&
        placed.w >= minNestSize &&
        placed.h >= minNestSize + headerHeight;

      if (canNest) {
        walk(
          placed.children!,
          {
            x: placed.x + 1,
            y: placed.y + headerHeight,
            w: placed.w - 2,
            h: placed.h - headerHeight - 1,
          },
          depth + 1,
        );
      }
    }
  };

  walk(items, { x: 0, y: 0, w: width, h: height }, 0);
  return out;
}

/** The items `start..end` that share a row, and their total area. */
interface Row {
  end: number;
  sum: number;
}

/** Grow the row while the worst aspect ratio keeps improving. */
function growRow(sorted: TreemapItem[], start: number, scale: number, side: number): Row {
  let rowSum = sorted[start].size * scale;
  let rowMax = rowSum;
  let rowMin = rowSum;
  let end = start + 1;

  while (end < sorted.length) {
    const area = sorted[end].size * scale;
    const nextSum = rowSum + area;
    const nextMin = Math.min(rowMin, area);
    const nextMax = Math.max(rowMax, area);

    if (worstRatio(nextMax, nextMin, nextSum, side) > worstRatio(rowMax, rowMin, rowSum, side)) {
      break;
    }
    rowSum = nextSum;
    rowMin = nextMin;
    rowMax = nextMax;
    end++;
  }
  return { end, sum: rowSum };
}

/** Place a row along the free rectangle, and return what is left of it. */
function placeRow(
  sorted: TreemapItem[],
  start: number,
  row: Row,
  { free, side, scale, depth }: { free: Rect; side: number; scale: number; depth: number },
): { tiles: Tile[]; rest: Rect } {
  const { x, y, w, h } = free;
  const horizontal = w >= h;
  // How far the row eats into the free rectangle.
  const thickness = row.sum / side;
  const tiles: Tile[] = [];
  let offset = 0;

  for (let k = start; k < row.end; k++) {
    const area = sorted[k].size * scale;
    // Last tile in the row absorbs rounding so rows meet their edge exactly.
    const length = k === row.end - 1 ? side - offset : area / thickness;

    tiles.push({
      id: sorted[k].id,
      name: sorted[k].name,
      size: sorted[k].size,
      isDir: sorted[k].isDir,
      children: sorted[k].children,
      depth,
      x: horizontal ? x : x + offset,
      y: horizontal ? y + offset : y,
      w: horizontal ? thickness : length,
      h: horizontal ? length : thickness,
    });
    offset += length;
  }

  const rest = horizontal
    ? { x: x + thickness, y, w: w - thickness, h }
    : { x, y: y + thickness, w, h: h - thickness };
  return { tiles, rest };
}

/**
 * The worst aspect ratio in a row, given the row's total area and the length of
 * the side it is being laid along. Lower is squarer.
 */
function worstRatio(rowMax: number, rowMin: number, sum: number, side: number): number {
  if (sum <= 0 || rowMin <= 0 || side <= 0) return Infinity;
  const s2 = sum * sum;
  const w2 = side * side;
  return Math.max((w2 * rowMax) / s2, s2 / (w2 * rowMin));
}
