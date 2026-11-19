// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, screen, waitFor, within } from "@testing-library/react";
import { commands } from "./api";
import { renderBrowsing, stubLayout } from "./test/app";
import {
  SESSION,
  emit,
  node,
  overview,
  resetApi,
  row,
  size,
  stagedItem,
  stagedSummary,
  stagedTotals,
  treemapNode,
  view,
} from "./test/api";

vi.mock("./api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./api")>()),
  ...(await import("./test/api")).apiMock,
}));

const api = vi.mocked(commands);

const ROWS = [
  row(1, { name: "DCIM", kind: "dir", size: size(4096), files: 40 }),
  row(2, { name: "Movies", kind: "dir", size: size(2048), files: 3 }),
  row(3, { name: "a.zip", size: size(1024) }),
];
const ROOT = view(0, ROWS, { path: "/sdcard", size: size(7168), totalChildren: 3 });

const DCIM = stagedItem(1, { name: "DCIM", path: "/sdcard/DCIM", kind: "dir", size: size(4096) });
const ZIP = stagedItem(3, { name: "a.zip", path: "/sdcard/a.zip", size: size(1024) });

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

/** The host's queue, answering the staging commands the way it would. */
function hostQueue(items: ReturnType<typeof stagedItem>[]) {
  const queue = stagedSummary(items);
  api.stageDelete.mockResolvedValue(queue.totals);
  api.stagedSummary.mockResolvedValue(queue);
  return queue;
}

async function explorer() {
  return renderBrowsing({ view: ROOT, treemap: treemapNode(0), tab: "explorer" });
}

/** Puts `items` in the queue as the host reports it, without going through the UI. */
async function queued(items: ReturnType<typeof stagedItem>[]) {
  const queue = hostQueue(items);
  await act(async () => emit.stagedChanged({ session: SESSION, totals: queue.totals }));
  return queue;
}

const bar = () => screen.getByRole("region", { name: "Pending delete" });

describe("staging from the selection", () => {
  it("queues the picked rows, clears the selection and shows the pending bar", async () => {
    const user = await explorer();
    hostQueue([DCIM, ZIP]);

    await user.click(screen.getByRole("checkbox", { name: "Select DCIM" }));
    await user.click(screen.getByRole("checkbox", { name: "Select a.zip" }));
    await user.click(screen.getByRole("button", { name: "Delete 2 items" }));

    expect(api.stageDelete).toHaveBeenCalledWith([node(1), node(3)]);
    expect(api.deleteStaged).not.toHaveBeenCalled();
    const summary = await screen.findByText(/Pending delete · 2 items/);
    expect(summary.textContent).toContain("5.0 KB");
    expect(screen.queryByRole("region", { name: "Selection" })).toBeNull();
  });

  it("keeps the picks when the host refuses to stage them", async () => {
    const user = await explorer();
    api.stageDelete.mockRejectedValue({ code: "stale", message: "Results are out of date." });

    await user.click(screen.getByRole("checkbox", { name: "Select a.zip" }));
    await user.click(screen.getByRole("button", { name: "Delete 1 item" }));

    await waitFor(() => expect(api.stageDelete).toHaveBeenCalled());
    expect(screen.getByRole("region", { name: "Selection" })).toBeTruthy();
    expect(screen.queryByRole("region", { name: "Pending delete" })).toBeNull();
  });
});

describe("the pending bar", () => {
  it("states the count and size in a polite live region", async () => {
    await explorer();
    await queued([DCIM, ZIP]);

    const summary = within(bar()).getByText(/Pending delete · 2 items/);
    expect(summary.getAttribute("aria-live")).toBe("polite");
    expect(summary.textContent).toContain("5.0 KB");
  });

  it("says 'at least' while a queued folder is still being measured", async () => {
    await explorer();
    hostQueue([DCIM]);
    await act(async () =>
      emit.stagedChanged({
        session: SESSION,
        totals: stagedTotals({ items: 1, size: size(4096), measure: "partial" }),
      }),
    );

    expect(within(bar()).getByText(/at least 4\.0 KB/)).toBeTruthy();
  });

  it("stays while the user opens folders and changes tabs", async () => {
    const user = await explorer();
    await queued([ZIP]);
    api.getView.mockResolvedValue(view(1, [row(10)], { path: "/sdcard/DCIM" }));

    await user.click(screen.getByRole("button", { name: "DCIM" }));
    await waitFor(() =>
      expect(api.focus).toHaveBeenCalledWith(expect.objectContaining({ node: node(1) })),
    );
    await user.click(screen.getByRole("tab", { name: "Overview" }));

    expect(within(bar()).getByText(/Pending delete · 1 item/)).toBeTruthy();
  });

  it("is gone once the host reports an empty queue", async () => {
    await explorer();
    await queued([ZIP]);
    expect(bar()).toBeTruthy();

    await act(async () => emit.stagedChanged({ session: SESSION, totals: stagedTotals() }));

    expect(screen.queryByRole("region", { name: "Pending delete" })).toBeNull();
  });

  it("ignores a queue reported for another session", async () => {
    await explorer();
    await act(async () =>
      emit.stagedChanged({ session: SESSION + 1, totals: stagedTotals({ items: 4 }) }),
    );

    expect(screen.queryByRole("region", { name: "Pending delete" })).toBeNull();
  });

  it("re-reads the folder when the host drops a queued node, but not for its own echo", async () => {
    const user = await explorer();
    hostQueue([DCIM, ZIP]);
    await user.click(screen.getByRole("checkbox", { name: "Select a.zip" }));
    await user.click(screen.getByRole("button", { name: "Delete 1 item" }));
    await screen.findByText(/Pending delete · 2 items/);

    // The echo of what the command already returned changes nothing.
    api.getView.mockClear();
    await act(async () =>
      emit.stagedChanged({ session: SESSION, totals: stagedSummary([DCIM, ZIP]).totals }),
    );
    expect(api.getView).not.toHaveBeenCalled();

    // A scan removed one of them.
    await act(async () =>
      emit.stagedChanged({ session: SESSION, totals: stagedSummary([DCIM]).totals }),
    );
    await waitFor(() => expect(api.getView).toHaveBeenCalled());
    expect(within(bar()).getByText(/Pending delete · 1 item ·/)).toBeTruthy();
  });
});

describe("reviewing and undoing", () => {
  it("lists every queued item with its path and an Undo", async () => {
    const user = await explorer();
    await queued([DCIM, ZIP]);

    await user.click(within(bar()).getByRole("button", { name: "Review" }));

    const list = await screen.findByRole("list", { name: "Items pending delete" });
    expect(within(list).getByText("/sdcard/DCIM")).toBeTruthy();
    expect(within(list).getByText("/sdcard/a.zip")).toBeTruthy();
    expect(
      within(list).getAllByRole("button", { name: /^Remove .* from pending delete$/ }),
    ).toHaveLength(2);
    expect(
      within(bar()).getByRole("button", { name: "Review" }).getAttribute("aria-expanded"),
    ).toBe("true");
  });

  it("takes one item out of the queue and refreshes the list", async () => {
    const user = await explorer();
    await queued([DCIM, ZIP]);
    await user.click(within(bar()).getByRole("button", { name: "Review" }));
    await screen.findByRole("list", { name: "Items pending delete" });

    const rest = stagedSummary([ZIP]);
    api.unstageDelete.mockResolvedValue(rest.totals);
    api.stagedSummary.mockResolvedValue(rest);
    await user.click(screen.getByRole("button", { name: "Remove DCIM from pending delete" }));

    expect(api.unstageDelete).toHaveBeenCalledWith([node(1)]);
    await waitFor(() => expect(screen.queryByText("/sdcard/DCIM")).toBeNull());
    expect(within(bar()).getByText(/Pending delete · 1 item/)).toBeTruthy();
  });

  it("closes the list with Escape", async () => {
    const user = await explorer();
    await queued([ZIP]);
    await user.click(within(bar()).getByRole("button", { name: "Review" }));
    await screen.findByRole("list", { name: "Items pending delete" });

    await user.keyboard("{Escape}");

    expect(screen.queryByRole("list", { name: "Items pending delete" })).toBeNull();
  });

  it("empties the queue with Clear and removes the bar", async () => {
    const user = await explorer();
    await queued([DCIM, ZIP]);
    api.clearStaged.mockResolvedValue(stagedTotals());

    await user.click(within(bar()).getByRole("button", { name: "Clear" }));

    expect(api.clearStaged).toHaveBeenCalledOnce();
    await waitFor(() =>
      expect(screen.queryByRole("region", { name: "Pending delete" })).toBeNull(),
    );
    expect(api.deleteStaged).not.toHaveBeenCalled();
  });
});

describe("deleting what is queued", () => {
  it("shows the queued paths first, and deletes nothing if cancelled", async () => {
    const user = await explorer();
    await queued([DCIM, ZIP]);

    await user.click(within(bar()).getByRole("button", { name: "Delete now…" }));

    const dialog = screen.getByRole("dialog");
    expect(within(dialog).getByRole("heading", { name: "Delete 2 items?" })).toBeTruthy();
    expect(within(dialog).getByText("/sdcard/DCIM")).toBeTruthy();
    expect(within(dialog).getByText("/sdcard/a.zip")).toBeTruthy();
    await user.click(within(dialog).getByRole("button", { name: "Cancel" }));

    expect(screen.queryByRole("dialog")).toBeNull();
    expect(api.deleteStaged).not.toHaveBeenCalled();
    expect(bar()).toBeTruthy();
  });

  it("runs deleteStaged on confirmation and reports what was freed", async () => {
    const user = await explorer();
    await queued([ZIP]);
    api.deleteStaged.mockResolvedValue({
      deleted: 1,
      items: 1,
      size: size(1024),
      failures: [],
      totals: { size: size(6144), files: 2, dirs: 2, errors: 0 },
      view: view(0, [ROWS[0], ROWS[1]], { path: "/sdcard" }),
    });

    await user.click(within(bar()).getByRole("button", { name: "Delete now…" }));
    await user.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Delete" }));

    await waitFor(() => expect(api.deleteStaged).toHaveBeenCalledOnce());
    expect(await screen.findByText(/Deleted a\.zip, freeing 1\.0 KB/)).toBeTruthy();
    expect(api.deleteMany).not.toHaveBeenCalled();

    // The host announces the emptied queue.
    await act(async () => emit.stagedChanged({ session: SESSION, totals: stagedTotals() }));
    expect(screen.queryByRole("region", { name: "Pending delete" })).toBeNull();
  });
});

describe("what the views show while deletes are pending", () => {
  it("strikes through a staged row, marks it and shows no size change for it", async () => {
    const staged = view(
      0,
      [
        row(1, { name: "DCIM", kind: "dir", size: size(4096), staged: true, projected: size(0) }),
        row(2, { name: "Movies", kind: "dir", size: size(2048) }),
      ],
      { path: "/sdcard" },
    );
    await renderBrowsing({ view: staged, treemap: treemapNode(0), tab: "explorer" });

    const rowEl = screen.getByRole("checkbox", { name: "Select DCIM" }).closest('[role="row"]')!;
    expect(rowEl.getAttribute("data-staged")).toBe("true");
    expect(within(rowEl as HTMLElement).getByText("pending delete")).toBeTruthy();
    expect(
      within(rowEl as HTMLElement)
        .getByText("DCIM")
        .closest(".line-through"),
    ).toBeTruthy();
    expect(within(rowEl as HTMLElement).queryByText(/→/)).toBeNull();
    expect(
      (
        within(rowEl as HTMLElement).getByRole("button", {
          name: /^Delete DCIM/,
        }) as HTMLButtonElement
      ).disabled,
    ).toBe(true);

    const other = screen.getByRole("checkbox", { name: "Select Movies" }).closest('[role="row"]')!;
    expect(other.getAttribute("data-staged")).toBeNull();
    expect(within(other as HTMLElement).queryByText("pending delete")).toBeNull();
  });

  it("shows 'now → after' on a folder holding staged items, and only there", async () => {
    const staged = view(
      0,
      [
        row(1, { name: "DCIM", kind: "dir", size: size(4096), projected: size(1024) }),
        row(2, { name: "Movies", kind: "dir", size: size(2048) }),
      ],
      { path: "/sdcard" },
    );
    await renderBrowsing({ view: staged, treemap: treemapNode(0), tab: "explorer" });

    const rowEl = screen
      .getByRole("checkbox", { name: "Select DCIM" })
      .closest('[role="row"]')! as HTMLElement;
    expect(rowEl.textContent).toMatch(/4\.0 KB → .*1\.0 KB/);
    expect(within(rowEl).getByText(/after the pending delete/)).toBeTruthy();
    const other = screen
      .getByRole("checkbox", { name: "Select Movies" })
      .closest('[role="row"]')! as HTMLElement;
    expect(other.textContent).not.toContain("→");
  });

  it("shows the apparent figures when that is the chosen size", async () => {
    const staged = view(
      0,
      [
        row(1, {
          name: "DCIM",
          kind: "dir",
          size: { apparent: 3000, disk: 4096 },
          projected: { apparent: 2000, disk: 4096 },
        }),
      ],
      { path: "/sdcard" },
    );
    const user = await renderBrowsing({ view: staged, treemap: treemapNode(0), tab: "explorer" });
    // Disk usage is unchanged by the pending delete, so nothing differs yet.
    const rowEl = () =>
      screen.getByRole("checkbox", { name: "Select DCIM" }).closest('[role="row"]')! as HTMLElement;
    expect(rowEl().textContent).not.toContain("→");

    await user.click(screen.getByRole("radio", { name: "File size" }));
    await waitFor(() => expect(rowEl().textContent).toMatch(/2\.9 KB → .*2\.0 KB/));
  });

  it("hatches a staged tile on the map and names it as pending in its label", async () => {
    const map = treemapNode(0, {
      children: [
        treemapNode(1, { name: "DCIM", size: size(600), staged: true, projected: size(0) }),
        treemapNode(2, { name: "Movies", size: size(300), projected: size(100) }),
      ],
    });
    await renderBrowsing({ view: ROOT, treemap: map, tab: "explorer" });

    const group = screen.getByRole("group", { name: "Folder map" });
    const dcim = await within(group).findByRole("button", { name: /^DCIM.*pending delete/ });
    expect(dcim.getAttribute("data-staged")).toBe("true");
    expect(dcim.style.background).toContain("var(--hatch)");
    const movies = within(group).getByRole("button", { name: /^Movies/ });
    expect(movies.getAttribute("data-staged")).toBeNull();
    expect(movies.getAttribute("aria-label")).toMatch(/100 B after the pending delete/);
  });
});

describe("the overview", () => {
  it("says what the queue would free, and what the phone would hold after", async () => {
    await renderBrowsing({
      overview: overview({
        staged: stagedTotals({ items: 2, size: size(5120) }),
        projected: size(10 * 1024),
        totals: { size: size(15 * 1024), files: 9, dirs: 3, errors: 0 },
      }),
    });

    const card = screen.getByRole("region", { name: /Pending delete/ });
    expect(within(card).getByText(/Would free/).textContent).toMatch(/5\.0 KB/);
    expect(card.textContent).toContain("from 15.0 KB to 10.0 KB");
  });

  it("leaves the card out when nothing is queued", async () => {
    await renderBrowsing({ overview: overview() });

    expect(screen.queryByText(/Would free/)).toBeNull();
  });

  it("calls a still-measured queue a lower bound", async () => {
    await renderBrowsing({
      overview: overview({
        staged: stagedTotals({ items: 1, size: size(2048), measure: "partial" }),
        projected: size(98),
      }),
    });

    expect(screen.getByText(/Would free/).textContent).toMatch(/at least 2\.0 KB/);
  });
});
