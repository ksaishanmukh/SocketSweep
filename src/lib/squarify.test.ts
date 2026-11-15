import { describe, it, expect } from "vitest";
import { squarify, type TreemapItem, type Tile } from "./squarify";

const item = (id: number, size: number, name = `n${id}`): TreemapItem => ({
  id,
  name,
  size,
  isDir: false,
});

/** True if two rectangles share any area (touching edges do not count). */
function overlaps(a: Tile, b: Tile): boolean {
  const EPS = 1e-6;
  return (
    a.x < b.x + b.w - EPS && b.x < a.x + a.w - EPS && a.y < b.y + b.h - EPS && b.y < a.y + a.h - EPS
  );
}

describe("squarify", () => {
  it("returns nothing for an empty list or a zero-area rectangle", () => {
    expect(squarify([], 100, 100)).toEqual([]);
    expect(squarify([item(1, 10)], 0, 100)).toEqual([]);
    expect(squarify([item(1, 10)], 100, 0)).toEqual([]);
  });

  it("gives a single item the whole rectangle", () => {
    const [tile] = squarify([item(1, 42)], 200, 100);
    expect(tile).toMatchObject({ id: 1, x: 0, y: 0, w: 200, h: 100 });
  });

  it("drops items with no area rather than emitting degenerate tiles", () => {
    const tiles = squarify([item(1, 100), item(2, 0), item(3, -5)], 100, 100);
    expect(tiles.map((t) => t.id)).toEqual([1]);
  });

  it("allocates area in proportion to size", () => {
    const items = [item(1, 50), item(2, 30), item(3, 20)];
    const tiles = squarify(items, 400, 300);
    const total = 400 * 300;

    for (const t of tiles) {
      const expected = (t.size / 100) * total;
      // Generous tolerance: the last tile in each row absorbs rounding.
      expect(Math.abs(t.w * t.h - expected) / expected).toBeLessThan(0.02);
    }
  });

  it("covers the rectangle without overlapping", () => {
    const items = Array.from({ length: 60 }, (_, i) => item(i, 100 - i));
    const tiles = squarify(items, 800, 500);

    for (let i = 0; i < tiles.length; i++) {
      for (let j = i + 1; j < tiles.length; j++) {
        expect(overlaps(tiles[i], tiles[j])).toBe(false);
      }
    }

    const covered = tiles.reduce((sum, t) => sum + t.w * t.h, 0);
    expect(covered / (800 * 500)).toBeCloseTo(1, 1);
  });

  it("keeps every tile inside the rectangle", () => {
    const items = Array.from({ length: 40 }, (_, i) => item(i, (i % 7) + 1));
    const tiles = squarify(items, 600, 400);

    for (const t of tiles) {
      expect(t.x).toBeGreaterThanOrEqual(-1e-6);
      expect(t.y).toBeGreaterThanOrEqual(-1e-6);
      expect(t.x + t.w).toBeLessThanOrEqual(600 + 1e-6);
      expect(t.y + t.h).toBeLessThanOrEqual(400 + 1e-6);
    }
  });

  /** The entire reason for choosing squarified over slice-and-dice. */
  it("produces tiles close to square rather than slivers", () => {
    const items = Array.from({ length: 30 }, (_, i) => item(i, 30 - i));
    const tiles = squarify(items, 600, 400);

    const ratios = tiles.map((t) => Math.max(t.w / t.h, t.h / t.w));
    const median = ratios.sort((a, b) => a - b)[Math.floor(ratios.length / 2)];
    expect(median).toBeLessThan(3);

    // Slice-and-dice on the same data gives every tile a 600:13 ratio.
    const sliced = 600 / (400 / items.length);
    expect(median).toBeLessThan(sliced / 5);
  });

  it("orders tiles largest first", () => {
    const tiles = squarify([item(1, 5), item(2, 100), item(3, 40)], 300, 300);
    expect(tiles.map((t) => t.id)).toEqual([2, 3, 1]);
  });

  /**
   * Sizes climb throughout a scan, so equal values must not reshuffle between
   * refreshes or the treemap visibly jitters for no reason.
   */
  it("is deterministic when sizes tie", () => {
    const items = [item(1, 10, "b"), item(2, 10, "a"), item(3, 10, "c")];
    const first = squarify(items, 300, 200).map((t) => t.id);

    for (let i = 0; i < 10; i++) {
      // Re-shuffle the input; the output order must not depend on it.
      const shuffled = [...items].reverse();
      expect(squarify(shuffled, 300, 200).map((t) => t.id)).toEqual(first);
    }
    expect(first).toEqual([2, 1, 3]); // by name, since sizes are equal
  });

  it("handles one item dwarfing the rest", () => {
    const items = [item(1, 1_000_000), ...Array.from({ length: 20 }, (_, i) => item(i + 2, 1))];
    const tiles = squarify(items, 800, 600);

    expect(tiles).toHaveLength(21);
    expect(tiles[0].w * tiles[0].h).toBeGreaterThan(0.99 * 800 * 600);
    for (const t of tiles) {
      expect(t.w).toBeGreaterThanOrEqual(0);
      expect(t.h).toBeGreaterThanOrEqual(0);
      expect(Number.isFinite(t.x + t.y + t.w + t.h)).toBe(true);
    }
  });

  it("survives a wide directory without producing NaN geometry", () => {
    const items = Array.from({ length: 2000 }, (_, i) => item(i, Math.random() * 1000 + 1));
    const tiles = squarify(items, 1000, 700);
    expect(tiles).toHaveLength(2000);
    expect(tiles.every((t) => Number.isFinite(t.x + t.y + t.w + t.h))).toBe(true);
  });
});
