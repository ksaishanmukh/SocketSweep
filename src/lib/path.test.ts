import { describe, expect, it } from "vitest";
import { folderName } from "./path";

describe("folderName", () => {
  it("names the last folder of a Windows or Unix path", () => {
    expect(folderName("C:\\Users\\me\\Desktop")).toBe("Desktop");
    expect(folderName("/home/me/Desktop/")).toBe("Desktop");
  });

  it("falls back to the whole path when there is no segment", () => {
    expect(folderName("/")).toBe("/");
  });
});
