import { describe, expect, it } from "vitest";
import { node } from "../test/api";
import { EMPTY_HISTORY, canGoBack, canGoForward, currentEntry, step, visit } from "./history";

describe("history", () => {
  it("moves back and forward between visited folders", () => {
    let h = visit(visit(visit(EMPTY_HISTORY, node(0)), node(1)), node(2));
    expect(canGoBack(h)).toBe(true);
    h = step(h, -1);
    expect(currentEntry(h)).toEqual(node(1));
    expect(canGoForward(h)).toBe(true);
    h = step(h, 1);
    expect(currentEntry(h)).toEqual(node(2));
    expect(canGoForward(h)).toBe(false);
  });

  it("drops what was ahead when going somewhere new", () => {
    const h = visit(step(visit(visit(EMPTY_HISTORY, node(0)), node(1)), -1), node(5));
    expect(h.entries).toEqual([node(0), node(5)]);
    expect(canGoForward(h)).toBe(false);
  });

  it("does not count reopening the current folder as a step, and stops at either end", () => {
    const h = visit(visit(EMPTY_HISTORY, node(0)), node(0));
    expect(h.entries).toHaveLength(1);
    expect(step(h, -1)).toBe(h);
    expect(step(h, 1)).toBe(h);
  });
});
