import { describe, expect, it } from "vitest";
import {
  MAP_COLLAPSE_BELOW,
  MAP_DEFAULT_HEIGHT,
  MAP_MAX_HEIGHT,
  keyHeight,
  moveDrag,
  releaseDrag,
  restoredHeight,
  startDrag,
} from "./mapGrip";

/** Drags from `from` by `dy` and lets go, with the map last open at `lastOpen`. */
function dragAndRelease(from: number, dy: number, lastOpen = from) {
  return releaseDrag(moveDrag(startDrag(from), dy), lastOpen);
}

describe("dragging the map's grip", () => {
  it("follows the pointer and stays where it is let go", () => {
    expect(moveDrag(startDrag(260), 40).height).toBe(300);
    expect(dragAndRelease(260, 40)).toBe(300);
    expect(dragAndRelease(260, -100)).toBe(160);
  });

  it("never grows past the maximum or below nothing", () => {
    expect(moveDrag(startDrag(260), 1000).height).toBe(MAP_MAX_HEIGHT);
    expect(moveDrag(startDrag(260), -1000).height).toBe(0);
  });

  it("collapses the map when let go near the top", () => {
    expect(dragAndRelease(260, -(260 - MAP_COLLAPSE_BELOW) - 1)).toBe(0);
    expect(dragAndRelease(260, -200)).toBe(0);
    // Right at the threshold the map is still big enough to keep.
    expect(dragAndRelease(260, -(260 - MAP_COLLAPSE_BELOW))).toBe(MAP_COLLAPSE_BELOW);
  });

  it("brings a collapsed map back by dragging down", () => {
    expect(dragAndRelease(0, 200, 300)).toBe(200);
    // Not far enough to be readable: it stays collapsed.
    expect(dragAndRelease(0, 40, 300)).toBe(0);
  });

  it("treats a press without movement as a click: a collapsed map comes back at its last height", () => {
    expect(dragAndRelease(0, 1, 340)).toBe(340);
    expect(dragAndRelease(0, 0, 0)).toBe(MAP_DEFAULT_HEIGHT);
    // On an open map a click changes nothing.
    expect(dragAndRelease(260, 2)).toBe(260);
  });

  it("restores to the default when the last height was not usable", () => {
    expect(restoredHeight(40)).toBe(MAP_DEFAULT_HEIGHT);
    expect(restoredHeight(9999)).toBe(MAP_MAX_HEIGHT);
  });
});

describe("the grip's keys", () => {
  it("moves with the arrows, collapsing near the top, and toggles with Enter", () => {
    expect(keyHeight("ArrowDown", 260, 260)).toBe(300);
    expect(keyHeight("ArrowUp", 260, 260)).toBe(220);
    expect(keyHeight("ArrowUp", 120, 120)).toBe(0);
    expect(keyHeight("ArrowDown", 0, 320)).toBe(320);
    expect(keyHeight("Enter", 260, 260)).toBe(0);
    expect(keyHeight("Enter", 0, 260)).toBe(260);
    expect(keyHeight("a", 260, 260)).toBeNull();
  });
});
