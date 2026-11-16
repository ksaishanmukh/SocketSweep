// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { DEFAULT_PREFS, parsePrefs, readPrefs, serializePrefs, storePrefs } from "./prefs";

beforeEach(() => window.localStorage.clear());
afterEach(() => vi.restoreAllMocks());

describe("parsePrefs", () => {
  it("round-trips what it serializes", () => {
    const prefs = {
      metric: "apparent" as const,
      sort: { key: "name" as const, descending: false },
      filters: { categories: ["photo" as const], kinds: ["file" as const], changed: [] },
    };
    expect(parsePrefs(serializePrefs(prefs))).toEqual(prefs);
  });

  it.each([null, "", "not json", "42", "[]", "null"])("falls back on %j", (text) => {
    expect(parsePrefs(text)).toEqual(DEFAULT_PREFS);
  });

  it("keeps good fields and drops bad ones", () => {
    const text = JSON.stringify({
      metric: "bogus",
      sort: { key: "count", descending: "yes" },
      filters: { categories: ["video", "nope", 3], kinds: "file", changed: ["overYear"] },
    });
    expect(parsePrefs(text)).toEqual({
      metric: "disk",
      sort: { key: "count", descending: true },
      filters: { categories: ["video"], kinds: [], changed: ["overYear"] },
    });
  });
});

describe("storage", () => {
  it("reads the defaults when nothing is stored", () => {
    expect(readPrefs()).toEqual(DEFAULT_PREFS);
  });

  it("merges a change into what is stored", () => {
    storePrefs({ metric: "apparent" });
    storePrefs({ sort: { key: "name", descending: false } });
    expect(readPrefs()).toMatchObject({ metric: "apparent", sort: { key: "name" } });
  });

  it("survives storage that throws", () => {
    vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
      throw new Error("denied");
    });
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new Error("denied");
    });
    expect(readPrefs()).toEqual(DEFAULT_PREFS);
    expect(() => storePrefs({ metric: "apparent" })).not.toThrow();
  });
});
