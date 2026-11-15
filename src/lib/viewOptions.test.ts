import { describe, expect, it } from "vitest";
import {
  NO_FILTERS,
  activeOptions,
  filterCount,
  nextSort,
  searchFilter,
  toggleOption,
  viewFilter,
} from "./viewOptions";

describe("nextSort", () => {
  it("starts names A to Z and everything else largest or newest first", () => {
    const bySize = { key: "size" as const, descending: true };
    expect(nextSort(bySize, "name")).toEqual({ key: "name", descending: false });
    expect(nextSort(bySize, "count")).toEqual({ key: "count", descending: true });
    expect(nextSort(bySize, "changed")).toEqual({ key: "changed", descending: true });
  });

  it("flips the active column", () => {
    expect(nextSort({ key: "size", descending: true }, "size")).toEqual({
      key: "size",
      descending: false,
    });
    expect(nextSort({ key: "name", descending: false }, "name")).toEqual({
      key: "name",
      descending: true,
    });
  });
});

describe("filters", () => {
  it("toggles an option on and off within its group", () => {
    const on = toggleOption(NO_FILTERS, { group: "categories", value: "video" });
    const two = toggleOption(on, { group: "categories", value: "audio" });
    expect(two.categories).toEqual(["video", "audio"]);
    expect(toggleOption(two, { group: "categories", value: "video" }).categories).toEqual([
      "audio",
    ]);
  });

  it("counts and lists the active options in panel order", () => {
    let f = toggleOption(NO_FILTERS, { group: "changed", value: "overYear" });
    f = toggleOption(f, { group: "kinds", value: "file" });
    f = toggleOption(f, { group: "categories", value: "doc" });
    expect(filterCount(f)).toBe(3);
    expect(activeOptions(f).map((o) => o.value)).toEqual(["doc", "file", "overYear"]);
  });

  it("sends an empty search as no query, and keeps the query out of search's filter", () => {
    expect(viewFilter(NO_FILTERS, "  ").query).toBeNull();
    expect(viewFilter(NO_FILTERS, " cam ").query).toBe("cam");
    expect(searchFilter(NO_FILTERS)).toEqual({ categories: [], kinds: [], changed: [] });
    expect("query" in searchFilter(viewFilter(NO_FILTERS, "x"))).toBe(false);
  });
});
