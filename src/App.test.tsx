// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { commands } from "./api";
import {
  connected,
  emit,
  node,
  progress,
  resetApi,
  row,
  size,
  started,
  totals,
  treemapNode,
  view,
} from "./test/api";
import App from "./App";

vi.mock("./api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./api")>()),
  ...(await import("./test/api")).apiMock,
}));

const api = vi.mocked(commands);

beforeEach(() => {
  resetApi();
  // jsdom has no layout: give the virtual list and the treemap a size.
  vi.spyOn(HTMLElement.prototype, "offsetHeight", "get").mockReturnValue(500);
  vi.spyOn(HTMLElement.prototype, "offsetWidth", "get").mockReturnValue(500);
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      unobserve() {}
      disconnect() {}
    },
  );
});
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

async function browse() {
  const user = userEvent.setup();
  api.connect.mockResolvedValue(connected());
  api.startScan.mockResolvedValue(started());
  api.getView.mockResolvedValue(
    view(
      0,
      [
        row(1, { name: "Movies", kind: "dir", size: size(4096), measure: "partial" }),
        row(2, { name: "Pending", kind: "dir", size: size(0), measure: "unknown" }),
        row(3, { name: "notes.txt", size: size(2048), measure: "final" }),
      ],
      { path: "/sdcard" },
    ),
  );
  api.getBreadcrumbs.mockResolvedValue([{ node: node(0), name: "sdcard" }]);
  api.getTreemap.mockResolvedValue(treemapNode(0));
  api.largestFiles.mockResolvedValue([]);
  api.typeBreakdown.mockResolvedValue([]);
  api.appBreakdown.mockResolvedValue([]);

  render(<App />);
  await user.click(await screen.findByRole("button", { name: /Connect Device/ }));
  await act(async () => emit.scanProgress(progress({ totals: totals({ files: 3 }) })));
  await screen.findByRole("navigation", { name: "Breadcrumb" });
  return user;
}

describe("App", () => {
  it("is browsable while the scan runs, with measure states in the list", async () => {
    await browse();

    expect(screen.getByText("Measuring sizes…")).toBeTruthy();
    expect(screen.getByText("measuring…")).toBeTruthy();
    expect(screen.getByTitle(/still counting/i)).toBeTruthy();
    expect(screen.getByText("2.0 KB")).toBeTruthy();
    // Disk usage is the default; the toggle switches to what a file manager shows.
    expect(screen.getByRole("radio", { name: "Disk usage" }).getAttribute("aria-checked")).toBe(
      "true",
    );
  });

  it("deletes the confirmed row by NodeRef, showing the full path first", async () => {
    const user = await browse();
    api.delete.mockResolvedValue({ items: 1, totals: totals(), view: view(0, [row(1)]) });

    await user.click(screen.getByRole("button", { name: /^Delete notes\.txt/ }));
    const dialog = screen.getByRole("dialog");
    expect(within(dialog).getByText("/sdcard/notes.txt")).toBeTruthy();
    await user.click(within(dialog).getByRole("button", { name: "Delete" }));

    await waitFor(() => expect(api.delete).toHaveBeenCalledWith(node(3)));
  });

  it("copies a row to the computer and shows the progress bar", async () => {
    const user = await browse();
    let finish!: (v: {
      destination: string;
      files: number;
      bytes: number;
      skipped: number;
      renamed: number;
    }) => void;
    api.copyToDesktop.mockReturnValue(new Promise((resolve) => (finish = resolve)));

    await user.click(screen.getByRole("button", { name: "Copy notes.txt to computer" }));

    expect(api.copyToDesktop).toHaveBeenCalledWith(node(3));
    expect(await screen.findByText(/Copying notes\.txt/)).toBeTruthy();

    await act(async () =>
      finish({
        destination: "C:\\Desktop\\notes.txt",
        files: 1,
        bytes: 2048,
        skipped: 0,
        renamed: 0,
      }),
    );
    expect(await screen.findByText(/Copied notes\.txt to C:\\Desktop\\notes\.txt/)).toBeTruthy();
    expect(screen.queryByText(/Copying notes\.txt/)).toBeNull();
  });

  it("returns to the setup screen with a toast when the connection is lost", async () => {
    await browse();

    act(() => emit.connectionLost({ serial: "s", message: "The phone stopped answering." }));

    expect(await screen.findByRole("button", { name: /Connect Device/ })).toBeTruthy();
    expect(screen.getByText(/The phone stopped answering\./)).toBeTruthy();
    expect(screen.queryByRole("navigation", { name: "Breadcrumb" })).toBeNull();
  });
});
