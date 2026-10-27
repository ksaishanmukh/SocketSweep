import { describe, expect, it } from "vitest";
import { size, treemapNode } from "../test/api";
import { toItems } from "./treemapItems";

describe("toItems", () => {
  it("makes tiles only for nodes with a size to draw", () => {
    const root = treemapNode(0, {
      children: [
        treemapNode(1, { size: size(500) }),
        // Not measured yet: no area to occupy, which is not the same as empty.
        treemapNode(2, { size: size(0), measure: "unknown" }),
        // Measured and genuinely empty.
        treemapNode(3, { size: size(0), measure: "final" }),
        treemapNode(4, { size: size(80), measure: "partial" }),
      ],
    });

    const { items } = toItems(root, "disk");

    expect(items.map((i) => i.id)).toEqual([1, 4]);
  });

  it("leaves unknown and zero-sized nodes out of the nested level too", () => {
    const root = treemapNode(0, {
      children: [
        treemapNode(1, {
          size: size(300),
          children: [
            treemapNode(10, { kind: "file", size: size(300) }),
            treemapNode(11, { kind: "file", size: size(0), measure: "unknown" }),
          ],
        }),
      ],
    });

    const { items } = toItems(root, "disk");

    expect(items[0].children?.map((c) => c.id)).toEqual([10]);
  });

  it("marks partial nodes as counting, so the treemap can show them as lower bounds", () => {
    const root = treemapNode(0, {
      children: [
        treemapNode(1, { size: size(500), measure: "final" }),
        treemapNode(2, { size: size(80), measure: "partial" }),
      ],
    });

    expect([...toItems(root, "disk").counting]).toEqual([2]);
  });

  it("sizes tiles by the chosen metric, and drops a node that is empty under it", () => {
    const root = treemapNode(0, {
      children: [
        treemapNode(1, { size: { apparent: 10, disk: 4096 } }),
        treemapNode(2, { size: { apparent: 500, disk: 0 } }),
      ],
    });

    expect(toItems(root, "disk").items.map((i) => [i.id, i.size])).toEqual([[1, 4096]]);
    expect(toItems(root, "apparent").items.map((i) => [i.id, i.size])).toEqual([
      [1, 10],
      [2, 500],
    ]);
  });

  it("is empty when there is no treemap yet", () => {
    expect(toItems(null, "disk").items).toEqual([]);
  });
});
