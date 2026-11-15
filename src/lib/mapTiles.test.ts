import { describe, expect, it } from "vitest";
import { node, row, size, treemapNode, view } from "../test/api";
import { fadedTiles, mapItems, tileLabels } from "./mapTiles";

describe("mapItems", () => {
  it("makes tiles only for children with a size to draw, by the chosen metric", () => {
    const root = treemapNode(0, {
      children: [
        treemapNode(1, { size: size(500) }),
        // Not measured yet: no area, which is not the same as empty.
        treemapNode(2, { size: size(0), measure: "unknown" }),
        treemapNode(3, { size: size(0), measure: "final" }),
        treemapNode(4, { size: { apparent: 0, disk: 80 }, measure: "partial" }),
      ],
    });
    expect(mapItems(root, "disk").items.map((i) => i.id)).toEqual([1, 4]);
    expect(mapItems(root, "apparent").items.map((i) => i.id)).toEqual([1]);
    expect(mapItems(root, "disk").byId.get(4)?.measure).toBe("partial");
    expect(mapItems(null, "disk").items).toEqual([]);
  });
});

describe("tileLabels", () => {
  it("shows the name only where it fits, and the size only under room for both", () => {
    expect(tileLabels(56, 30)).toEqual({ name: false, size: false });
    expect(tileLabels(57, 25)).toEqual({ name: true, size: false });
    expect(tileLabels(70, 60)).toEqual({ name: true, size: false });
    expect(tileLabels(71, 46)).toEqual({ name: true, size: false });
    expect(tileLabels(71, 47)).toEqual({ name: true, size: true });
  });
});

describe("fadedTiles", () => {
  it("fades children the filter left out, once every match is loaded", () => {
    const v = view(0, [row(1)]);
    const faded = fadedTiles(v, true);
    expect(faded(node(1))).toBe(false);
    expect(faded(node(2))).toBe(true);
  });

  it("fades nothing without a filter, or while more matches are still to load", () => {
    expect(fadedTiles(view(0, [row(1)]), false)(node(2))).toBe(false);
    expect(fadedTiles(view(0, [row(1)], { hidden: 4 }), true)(node(2))).toBe(false);
  });
});
