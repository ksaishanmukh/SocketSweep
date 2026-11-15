// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, screen, waitFor, within } from "@testing-library/react";
import { commands } from "../../api";
import { renderBrowsing, stubLayout } from "../../test/app";
import {
  DEFAULT_SORT,
  NO_FILTER,
  node,
  resetApi,
  row,
  size,
  treemapNode,
  view,
} from "../../test/api";

vi.mock("../../api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../api")>()),
  ...(await import("../../test/api")).apiMock,
}));

const api = vi.mocked(commands);

const ROWS = [
  row(1, { name: "DCIM", kind: "dir", size: size(600), category: "photo", files: 40 }),
  row(2, { name: "Movies", kind: "dir", size: size(300), category: "video", files: 3 }),
  row(3, { name: "a.zip", size: size(100), category: "archive" }),
];
const ROOT = view(0, ROWS, { path: "/sdcard", size: size(1000), totalChildren: 3 });
const MAP = treemapNode(0, {
  children: [
    treemapNode(1, { name: "DCIM", size: size(600), category: "photo" }),
    treemapNode(2, { name: "Movies", size: size(300), category: "video" }),
    treemapNode(3, { name: "a.zip", kind: "file", size: size(100), category: "archive" }),
  ],
});

function explorer() {
  return renderBrowsing({ view: ROOT, treemap: MAP, tab: "explorer" });
}

const tile = (name: RegExp) =>
  within(screen.getByRole("group", { name: "Folder map" })).getByRole("button", { name });

beforeEach(() => {
  resetApi();
  stubLayout();
  // The map remembers its height; every test starts from the default.
  localStorage.clear();
});
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe("the map", () => {
  it("draws a tile per child and opens a folder tile on click", async () => {
    const user = await explorer();
    await waitFor(() => expect(tile(/^DCIM/)).toBeTruthy());
    api.getView.mockClear();

    await user.click(tile(/^DCIM/));

    expect(api.getView).toHaveBeenCalledWith(node(1), undefined, "disk", DEFAULT_SORT, NO_FILTER);
  });

  it("lights up a pointed tile and marks its row, but only hints at a row's tile", async () => {
    await explorer();
    await waitFor(() => expect(tile(/^DCIM/)).toBeTruthy());

    fireEvent.pointerEnter(tile(/^DCIM/));
    expect(tile(/^DCIM/).getAttribute("data-pointed")).toBe("strong");
    const dcimRow = screen.getByRole("button", { name: "DCIM" }).closest('[role="row"]')!;
    expect(dcimRow.getAttribute("data-hovered")).toBe("true");
    fireEvent.pointerLeave(tile(/^DCIM/));

    const moviesRow = screen.getByRole("button", { name: "Movies" }).closest('[role="row"]')!;
    fireEvent.pointerEnter(moviesRow);
    expect(tile(/^Movies/).getAttribute("data-pointed")).toBe("hint");
  });

  it("keeps filtered-out children on the map, faded", async () => {
    const user = await explorer();
    api.getView.mockResolvedValue(view(0, [ROWS[0]], { path: "/sdcard", totalChildren: 3 }));

    await user.click(screen.getByRole("button", { name: /^Filters/ }));
    await user.click(screen.getByRole("checkbox", { name: "Photos" }));

    await waitFor(() => expect(tile(/^Movies/).getAttribute("data-faded")).toBe("true"));
    expect(tile(/^DCIM/).getAttribute("data-faded")).toBeNull();
  });

  it("collapses to just its grip when dragged to the top", async () => {
    await explorer();
    const grip = screen.getByRole("separator", { name: /Map height/ });

    fireEvent.pointerDown(grip, { button: 0, clientY: 300 });
    fireEvent.pointerMove(grip, { clientY: 100 });
    fireEvent.pointerUp(grip, { clientY: 100 });

    expect(grip.getAttribute("aria-valuenow")).toBe("0");
    expect(screen.queryByRole("group", { name: "Folder map" })).toBeNull();
  });
});

describe("filters", () => {
  it("shows a badge and removable chips, and asks the host for each change", async () => {
    const user = await explorer();

    await user.click(screen.getByRole("button", { name: /^Filters/ }));
    const panel = screen.getByRole("dialog", { name: "Filters" });
    await user.click(within(panel).getByRole("checkbox", { name: "Videos" }));
    await user.click(within(panel).getByRole("checkbox", { name: "Over a year ago" }));

    await waitFor(() =>
      expect(api.getView).toHaveBeenLastCalledWith(node(0), undefined, "disk", DEFAULT_SORT, {
        ...NO_FILTER,
        categories: ["video"],
        changed: ["overYear"],
      }),
    );
    expect(within(panel).getByText(/of 3 items match/)).toBeTruthy();

    await user.click(within(panel).getByRole("button", { name: "Done" }));
    expect(screen.queryByRole("dialog", { name: "Filters" })).toBeNull();
    expect(screen.getByRole("button", { name: /^Filters/ }).textContent).toContain("2");

    await user.click(screen.getByRole("button", { name: "Remove filter Videos" }));
    await waitFor(() =>
      expect(api.getView).toHaveBeenLastCalledWith(node(0), undefined, "disk", DEFAULT_SORT, {
        ...NO_FILTER,
        changed: ["overYear"],
      }),
    );
    await user.click(screen.getByRole("button", { name: "Clear all" }));
    expect(screen.queryByRole("list", { name: "Active filters" })).toBeNull();
  });

  it("closes the panel on Escape and on a click outside it", async () => {
    const user = await explorer();
    const button = screen.getByRole("button", { name: /^Filters/ });

    await user.click(button);
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("dialog", { name: "Filters" })).toBeNull();
    expect(document.activeElement).toBe(button);

    await user.click(button);
    await user.click(screen.getByRole("navigation", { name: "Breadcrumb" }));
    expect(screen.queryByRole("dialog", { name: "Filters" })).toBeNull();
  });

  it("offers to clear search and filters when nothing matches", async () => {
    const user = await explorer();
    api.getView.mockResolvedValue(view(0, [], { path: "/sdcard", totalChildren: 3 }));
    await user.type(screen.getByRole("searchbox"), "zzz");

    await user.click(await screen.findByRole("button", { name: "Clear search and filters" }));

    expect((screen.getByRole("searchbox") as HTMLInputElement).value).toBe("");
  });
});

describe("sorting and selecting", () => {
  it("sorts by a column, flipping it on a second click", async () => {
    const user = await explorer();

    await user.click(screen.getByRole("button", { name: "Name" }));
    await waitFor(() =>
      expect(api.getView).toHaveBeenLastCalledWith(
        node(0),
        undefined,
        "disk",
        { key: "name", descending: false },
        NO_FILTER,
      ),
    );
    await user.click(screen.getByRole("button", { name: /^Name/ }));
    await waitFor(() =>
      expect(api.getView).toHaveBeenLastCalledWith(
        node(0),
        undefined,
        "disk",
        { key: "name", descending: true },
        NO_FILTER,
      ),
    );
    await user.click(screen.getByRole("button", { name: "Last changed" }));
    await waitFor(() =>
      expect(api.getView).toHaveBeenLastCalledWith(
        node(0),
        undefined,
        "disk",
        { key: "changed", descending: true },
        NO_FILTER,
      ),
    );
  });

  it("selects every shown row, and is indeterminate when only some are picked", async () => {
    const user = await explorer();
    const all = screen.getByRole("checkbox", { name: "Select all 3 shown" }) as HTMLInputElement;

    await user.click(screen.getByRole("checkbox", { name: "Select DCIM" }));
    expect(all.indeterminate).toBe(true);
    expect(all.getAttribute("aria-label")).toBe("Unselect all shown");
    expect(screen.getByText(/1 selected/)).toBeTruthy();

    // A mixed box clears.
    await user.click(all);
    expect(screen.queryByText(/selected ·/)).toBeNull();

    await user.click(screen.getByRole("checkbox", { name: "Select all 3 shown" }));
    expect(screen.getByText(/3 selected/)).toBeTruthy();
    expect(screen.getByRole("button", { name: "Delete 3 items" })).toBeTruthy();
    expect(screen.getByRole("button", { name: /Copy to Desktop/ })).toBeTruthy();
  });
});
