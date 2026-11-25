// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, screen, waitFor } from "@testing-library/react";
import { commands } from "../../api";
import { renderBrowsing, stubLayout } from "../../test/app";
import {
  DEFAULT_SORT,
  NO_FILTER,
  node,
  resetApi,
  row,
  size,
  stagedSummary,
  treemapNode,
  view,
} from "../../test/api";

vi.mock("../../api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../api")>()),
  ...(await import("../../test/api")).apiMock,
}));

const api = vi.mocked(commands);

const ROWS = [
  row(1, { name: "DCIM", kind: "dir", size: size(600), files: 40 }),
  row(2, { name: "Movies", kind: "dir", size: size(300), files: 3 }),
  row(3, { name: "a.zip", size: size(100) }),
  row(4, { name: "b.zip", size: size(50) }),
];
const ROOT = view(0, ROWS, { path: "/sdcard", size: size(1050), totalChildren: 4 });

beforeEach(() => {
  resetApi();
  stubLayout();
  localStorage.clear();
});
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

async function explorer() {
  const user = await renderBrowsing({ view: ROOT, treemap: treemapNode(0), tab: "explorer" });
  await screen.findByRole("checkbox", { name: "Select DCIM" });
  return user;
}

const rowOf = (name: string) =>
  screen.getByRole("checkbox", { name: `Select ${name}` }).closest<HTMLElement>('[role="row"]')!;
const picked = (name: string) => rowOf(name).getAttribute("aria-selected") === "true";

/** Puts focus on a row by tabbing to the list's one stop. */
async function focusFirstRow(user: Awaited<ReturnType<typeof explorer>>) {
  rowOf("DCIM").focus();
  await waitFor(() => expect(document.activeElement).toBe(rowOf("DCIM")));
  return user;
}

describe("the list's keys", () => {
  it("makes the active row the only tab stop and numbers the rows", async () => {
    await explorer();
    expect(rowOf("DCIM").tabIndex).toBe(0);
    expect(rowOf("Movies").tabIndex).toBe(-1);
    expect(rowOf("DCIM").getAttribute("aria-rowindex")).toBe("2");
    expect(screen.getByRole("grid", { name: "Files and folders" })).toBeTruthy();
  });

  it("moves with the arrows, Home and End", async () => {
    const user = await focusFirstRow(await explorer());
    await user.keyboard("{ArrowDown}");
    await waitFor(() => expect(document.activeElement).toBe(rowOf("Movies")));
    expect(rowOf("Movies").tabIndex).toBe(0);
    expect(rowOf("DCIM").tabIndex).toBe(-1);

    await user.keyboard("{End}");
    await waitFor(() => expect(document.activeElement).toBe(rowOf("b.zip")));
    await user.keyboard("{ArrowDown}");
    expect(document.activeElement).toBe(rowOf("b.zip"));
    await user.keyboard("{Home}");
    await waitFor(() => expect(document.activeElement).toBe(rowOf("DCIM")));
  });

  it("opens a folder on Enter and leaves a file alone", async () => {
    const user = await focusFirstRow(await explorer());
    api.getView.mockClear();
    await user.keyboard("{Enter}");
    await waitFor(() =>
      expect(api.getView).toHaveBeenCalledWith({
        node: node(1),
        by: "disk",
        sort: DEFAULT_SORT,
        filter: NO_FILTER,
      }),
    );

    api.getView.mockClear();
    await user.keyboard("{End}");
    await waitFor(() => expect(document.activeElement).toBe(rowOf("b.zip")));
    await user.keyboard("{Enter}");
    expect(api.getView).not.toHaveBeenCalled();
  });

  it("goes back on Backspace", async () => {
    const user = await focusFirstRow(await explorer());
    await user.keyboard("{Enter}");
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Back" })).toHaveProperty("disabled", false),
    );
    api.getView.mockClear();
    await focusFirstRow(user);
    await user.keyboard("{Backspace}");
    await waitFor(() => expect(api.getView).toHaveBeenCalled());
  });

  it("toggles the active row with Space and extends with Shift+arrows", async () => {
    const user = await focusFirstRow(await explorer());
    await user.keyboard(" ");
    expect(picked("DCIM")).toBe(true);
    await user.keyboard(" ");
    expect(picked("DCIM")).toBe(false);

    await user.keyboard("{Shift>}{ArrowDown}{ArrowDown}{/Shift}");
    await waitFor(() => expect(document.activeElement).toBe(rowOf("a.zip")));
    expect([picked("DCIM"), picked("Movies"), picked("a.zip"), picked("b.zip")]).toEqual([
      true,
      true,
      true,
      false,
    ]);

    await user.keyboard("{Shift>}{ArrowUp}{/Shift}");
    await waitFor(() => expect(picked("a.zip")).toBe(false));
    expect(picked("Movies")).toBe(true);
  });

  it("selects every row on Ctrl+A and clears on Escape", async () => {
    const user = await focusFirstRow(await explorer());
    await user.keyboard("{Control>}a{/Control}");
    expect(ROWS.map((r) => picked(r.name))).toEqual([true, true, true, true]);
    await user.keyboard("{Escape}");
    expect(ROWS.map((r) => picked(r.name))).toEqual([false, false, false, false]);
  });

  it("stages the selection on Delete, or the active row when nothing is selected", async () => {
    const queue = stagedSummary([]);
    api.stageDelete.mockResolvedValue(queue.totals);
    const user = await focusFirstRow(await explorer());

    await user.keyboard("{Delete}");
    await waitFor(() => expect(api.stageDelete).toHaveBeenCalledWith([node(1)]));

    api.stageDelete.mockClear();
    await user.keyboard("{ArrowDown} {ArrowDown} {Delete}");
    await waitFor(() => expect(api.stageDelete).toHaveBeenCalledTimes(1));
    expect(api.stageDelete).toHaveBeenCalledWith([node(2), node(3)]);
  });
});

describe("the Explorer's shortcuts", () => {
  it("focuses the search box on Ctrl+F", async () => {
    const user = await explorer();
    await user.keyboard("{Control>}f{/Control}");
    expect(document.activeElement).toBe(screen.getByRole("searchbox"));
  });

  it("opens the shortcuts on ? and from the toolbar button, and closes on Escape", async () => {
    const user = await explorer();
    await user.keyboard("?");
    expect(screen.getByRole("dialog", { name: "Keyboard shortcuts" })).toBeTruthy();
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("dialog", { name: "Keyboard shortcuts" })).toBeNull();

    await user.click(screen.getByRole("button", { name: "Keyboard shortcuts" }));
    expect(screen.getByRole("dialog", { name: "Keyboard shortcuts" })).toBeTruthy();
  });

  it("leaves keys alone while typing in the search box", async () => {
    const user = await explorer();
    const search = screen.getByRole("searchbox");
    await user.click(search);
    await user.keyboard("a?b");
    expect((search as HTMLInputElement).value).toBe("a?b");
    expect(screen.queryByRole("dialog", { name: "Keyboard shortcuts" })).toBeNull();
    expect(api.stageDelete).not.toHaveBeenCalled();
  });
});
