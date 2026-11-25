import { describe, expect, it } from "vitest";
import { clampIndex, indicesIn, leftBehind, moveTarget, spanBetween } from "./listKeys";

describe("moveTarget", () => {
  it("steps by one and stops at both ends", () => {
    expect(moveTarget("ArrowDown", 1, 5, 3)).toBe(2);
    expect(moveTarget("ArrowUp", 1, 5, 3)).toBe(0);
    expect(moveTarget("ArrowUp", 0, 5, 3)).toBe(0);
    expect(moveTarget("ArrowDown", 4, 5, 3)).toBe(4);
  });

  it("jumps to the ends and by a page", () => {
    expect(moveTarget("Home", 3, 5, 3)).toBe(0);
    expect(moveTarget("End", 1, 5, 3)).toBe(4);
    expect(moveTarget("PageDown", 0, 10, 4)).toBe(4);
    expect(moveTarget("PageDown", 8, 10, 4)).toBe(9);
    expect(moveTarget("PageUp", 2, 10, 4)).toBe(0);
  });

  it("moves at least one row for a page that measures zero", () => {
    expect(moveTarget("PageDown", 0, 5, 0)).toBe(1);
  });

  it("ignores other keys", () => {
    expect(moveTarget("a", 0, 5, 3)).toBeNull();
    expect(moveTarget("toString", 0, 5, 3)).toBeNull();
  });
});

describe("spans", () => {
  it("orders the ends and lists the rows", () => {
    expect(spanBetween(4, 2)).toEqual([2, 4]);
    expect(indicesIn([2, 4])).toEqual([2, 3, 4]);
  });

  it("names the rows a shrinking span lets go of", () => {
    expect(leftBehind(null, [1, 2])).toEqual([]);
    expect(leftBehind([1, 4], [1, 2])).toEqual([3, 4]);
    expect(leftBehind([1, 2], [0, 4])).toEqual([]);
  });

  it("clamps into the rows that exist", () => {
    expect(clampIndex(9, 3)).toBe(2);
    expect(clampIndex(-1, 3)).toBe(0);
    expect(clampIndex(4, 0)).toBe(0);
  });
});
