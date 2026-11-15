// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, screen, waitFor, within } from "@testing-library/react";
import { commands, type CopyResult } from "./api";
import { renderBrowsing, stubLayout } from "./test/app";
import { deferred, emit, node, resetApi, row, size, totals, view } from "./test/api";

vi.mock("./api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./api")>()),
  ...(await import("./test/api")).apiMock,
}));

const api = vi.mocked(commands);

const ROOT = view(
  0,
  [
    row(1, { name: "Movies", kind: "dir", size: size(4096), measure: "partial", files: 12 }),
    row(2, { name: "Pending", kind: "dir", size: size(0), measure: "unknown" }),
    row(3, { name: "notes.txt", size: size(2048), measure: "final" }),
  ],
  { path: "/sdcard" },
);

beforeEach(() => {
  resetApi();
  stubLayout();
});
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe("App", () => {
  it("opens on the Overview with the totals and scan progress in the header", async () => {
    await renderBrowsing({ view: ROOT });

    expect(screen.getByRole("tab", { name: "Overview" }).getAttribute("aria-selected")).toBe(
      "true",
    );
    expect(screen.getByText(/Measuring ·/)).toBeTruthy();
    expect(screen.getByText("5 of 20 folders")).toBeTruthy();
    expect(screen.getByText(/Connected over USB · \/sdcard/)).toBeTruthy();
    // Disk usage is the default; the toggle switches to what a file manager shows.
    expect(screen.getByRole("radio", { name: "Disk usage" }).getAttribute("aria-checked")).toBe(
      "true",
    );
  });

  it("is browsable in the Explorer while the scan runs, with measure states in the list", async () => {
    await renderBrowsing({ view: ROOT, tab: "explorer" });

    expect(screen.getByText("measuring…")).toBeTruthy();
    expect(screen.getByTitle(/still counting/i).textContent).toContain("4.0 KB");
    expect(screen.getByText("2.0 KB")).toBeTruthy();
    expect(screen.getByText("12 files")).toBeTruthy();
    expect(screen.getByText("TXT file")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Phone storage" })).toBeTruthy();
  });

  it("deletes a row by NodeRef after showing its full path", async () => {
    const user = await renderBrowsing({ view: ROOT, tab: "explorer" });
    api.deleteMany.mockResolvedValue({
      deleted: 1,
      items: 1,
      size: size(2048),
      failures: [],
      totals: totals(),
      view: view(0, [row(1)]),
    });

    await user.click(screen.getByRole("button", { name: /^Delete notes\.txt/ }));
    const dialog = screen.getByRole("dialog");
    expect(within(dialog).getByText("/sdcard/notes.txt")).toBeTruthy();
    await user.click(within(dialog).getByRole("button", { name: "Delete" }));

    await waitFor(() => expect(api.deleteMany).toHaveBeenCalledWith([node(3)]));
    expect(await screen.findByText(/Deleted notes\.txt, freeing 2\.0 KB/)).toBeTruthy();
  });

  it("copies a row to the computer and shows the progress bar", async () => {
    const user = await renderBrowsing({ view: ROOT, tab: "explorer" });
    const copying = deferred<CopyResult>();
    api.copyToDesktop.mockReturnValue(copying.promise);

    await user.click(screen.getByRole("button", { name: "Copy notes.txt to Desktop" }));

    expect(api.copyToDesktop).toHaveBeenCalledWith(node(3));
    expect(await screen.findByText(/Copying notes\.txt/)).toBeTruthy();
    await act(async () =>
      copying.resolve({
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
    await renderBrowsing({ view: ROOT });

    act(() => emit.connectionLost({ serial: "s", message: "The phone stopped answering." }));

    expect(await screen.findByRole("button", { name: /Connect Device/ })).toBeTruthy();
    expect(screen.getByText(/The phone stopped answering\./)).toBeTruthy();
    expect(screen.queryByRole("tab", { name: "Overview" })).toBeNull();
  });
});
