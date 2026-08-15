/**
 * Squarified treemap layout.
 *
 * Bruls, Huizing & van Wijk (2000). Lays items out in rows along the shorter
 * side of the remaining rectangle, extending a row while doing so improves its
 * worst aspect ratio. The result is tiles close to square, which are far easier
 * to compare by eye than the long slivers a naive slice-and-dice produces.
 *
 * Replaces Recharts, which was ~500KB of the bundle for one single-level
 * treemap, could not nest, and whose content renderer had to be fed untyped
 * props. This is a pure function, so it is testable and we own its behaviour
 * under the continuously changing sizes a running scan produces.
 */

export interface TreemapItem {
  id: number;
  name: string;
  size: number;
  isDir: boolean;
  children?: TreemapItem[];
}

export interface Rect {
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
 * The worst aspect ratio in a row, given the row's total area and the length of
 * the side it is being laid along. Lower is squarer.
 */
function worstRatio(rowMax: number, rowMin: number, sum: number, side: number): number {
  if (sum <= 0 || rowMin <= 0 || side <= 0) return Infinity;
  const s2 = sum * sum;
  const w2 = side * side;
  return Math.max((w2 * rowMax) / s2, s2 / (w2 * rowMin));
}

/**
 * Lay `items` out inside a `width` x `height` rectangle.
 *
 * Items with a non-positive size are dropped: they have no area, so they would
 * produce degenerate tiles. Ties are broken by name so the layout does not
 * reshuffle between the ~10 refreshes per second a running scan produces.
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
  let x = 0;
  let y = 0;
  let w = width;
  let h = height;

  let i = 0;
  while (i < sorted.length) {
    const side = Math.min(w, h);
    if (side <= 0) break;

    // Grow the row while the worst aspect ratio keeps improving.
    let rowSum = sorted[i].size * scale;
    let rowMax = rowSum;
    let rowMin = rowSum;
    let end = i + 1;

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

    // Place it. `thickness` is how far the row eats into the free rectangle.
    const horizontal = w >= h;
    const thickness = rowSum / side;
    let offset = 0;

    for (let k = i; k < end; k++) {
      const area = sorted[k].size * scale;
      // Last tile in the row absorbs rounding so rows meet their edge exactly.
      const length = k === end - 1 ? side - offset : area / thickness;

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

    if (horizontal) {
      x += thickness;
      w -= thickness;
    } else {
      y += thickness;
      h -= thickness;
    }
    i = end;
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
