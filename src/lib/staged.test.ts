import { describe, expect, it } from "vitest";
import { size, stagedItem } from "../test/api";
import { afterDelete, targetOfStaged } from "./staged";

describe("afterDelete", () => {
  it("gives the size now and after, in the chosen metric", () => {
    const now = { apparent: 3000, disk: 4096 };
    const after = { apparent: 1000, disk: 4096 };
    expect(afterDelete(now, after, "final", "apparent")).toEqual({ now: 3000, after: 1000 });
    // Disk usage does not move, so there is nothing to show under it.
    expect(afterDelete(now, after, "final", "disk")).toBeNull();
  });

  it("says nothing when nothing staged lies inside", () => {
    expect(afterDelete(size(100), size(100), "final", "disk")).toBeNull();
  });

  it("says nothing about a size that is not known yet", () => {
    expect(afterDelete(size(0), size(0), "unknown", "disk")).toBeNull();
    expect(afterDelete(size(100), size(0), "unknown", "disk")).toBeNull();
  });

  it("still shows the change on a size that is still counting", () => {
    expect(afterDelete(size(100), size(40), "partial", "disk")).toEqual({ now: 100, after: 40 });
  });
});

describe("targetOfStaged", () => {
  it("carries the item's path and size into the confirmation", () => {
    const item = stagedItem(4, {
      kind: "dir",
      name: "DCIM",
      path: "/sdcard/DCIM",
      size: size(2048),
      measure: "partial",
      files: 12,
    });
    expect(targetOfStaged(item)).toEqual({
      key: "1:4",
      nodes: [item.node],
      name: "DCIM",
      path: "/sdcard/DCIM",
      size: size(2048),
      measure: "partial",
      files: 12,
      kind: "dir",
    });
  });

  it("calls anything that is not a folder a file", () => {
    expect(targetOfStaged(stagedItem(5, { kind: "symlink" })).kind).toBe("file");
  });
});
