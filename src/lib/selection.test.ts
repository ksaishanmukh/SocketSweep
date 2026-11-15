import { describe, expect, it } from "vitest";
import { node, row, size } from "../test/api";
import {
  TYPE_TO_CONFIRM_BYTES,
  inSession,
  needsTypedConfirm,
  outermost,
  targetOfLeftover,
  targetOfRow,
  totalOf,
} from "./selection";

describe("targets", () => {
  it("takes a row's full path from its folder, or from its own parent on cross-tree results", () => {
    expect(targetOfRow(row(1, { name: "a.zip" }), "/sdcard/Download").path).toBe(
      "/sdcard/Download/a.zip",
    );
    expect(targetOfRow(row(1, { name: "a.zip", parent: "/sdcard/x" }), "/other").path).toBe(
      "/sdcard/x/a.zip",
    );
  });

  it("makes one pick of every folder a leftover app left", () => {
    const t = targetOfLeftover({ package: "com.gone", size: size(9), folders: [node(3), node(4)] });
    expect(t.nodes).toEqual([node(3), node(4)]);
    expect(t.kind).toBe("leftover");
  });

  it("belongs only to the session its nodes came from", () => {
    expect(inSession(targetOfRow(row(1), "/"), 1)).toBe(true);
    expect(inSession(targetOfRow(row(1, { node: node(1, 2) }), "/"), 1)).toBe(false);
    expect(inSession(targetOfRow(row(1), "/"), null)).toBe(false);
  });
});

describe("totals", () => {
  it("does not count something twice when its folder is picked too", () => {
    const folder = targetOfRow(row(1, { name: "DCIM", kind: "dir", size: size(100) }), "/sdcard");
    const inside = targetOfRow(row(2, { name: "a.jpg", size: size(40) }), "/sdcard/DCIM");
    const sibling = targetOfRow(row(3, { name: "DCIM-old", size: size(5) }), "/sdcard");
    expect(outermost([folder, inside, sibling])).toEqual([folder, sibling]);
    expect(totalOf([folder, inside, sibling], "disk").bytes).toBe(105);
  });

  it("asks for typing when the batch is big, has many files or is not fully measured", () => {
    const small = targetOfRow(row(1, { size: size(10) }), "/");
    const big = targetOfRow(row(2, { size: size(TYPE_TO_CONFIRM_BYTES) }), "/");
    const many = targetOfRow(row(3, { kind: "dir", files: 100 }), "/");
    const counting = targetOfRow(row(4, { kind: "dir", measure: "partial" }), "/");
    expect(needsTypedConfirm([small])).toBe(false);
    expect(needsTypedConfirm([small, big])).toBe(true);
    expect(needsTypedConfirm([many])).toBe(true);
    expect(needsTypedConfirm([counting])).toBe(true);
  });
});
