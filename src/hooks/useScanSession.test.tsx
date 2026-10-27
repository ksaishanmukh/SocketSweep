// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import * as ipc from "../lib/ipc";
import type { Row, Stats, View } from "../lib/types";
import { useScanSession } from "./useScanSession";

vi.mock("../lib/ipc", () => ({
  listDevices: vi.fn(),
  connect: vi.fn(),
  disconnect: vi.fn(),
  scan: vi.fn(),
  getView: vi.fn(),
  getBreadcrumbs: vi.fn(),
  getTreemap: vi.fn(),
  largestFiles: vi.fn(),
  typeBreakdown: vi.fn(),
  appBreakdown: vi.fn(),
  search: vi.fn(),
  deleteNode: vi.fn(),
  onScanProgress: vi.fn(() => Promise.resolve(() => {})),
  errorMessage: (err: unknown) =>
    typeof err === "string" ? err : err instanceof Error ? err.message : String(err),
}));

const api = vi.mocked(ipc);

const STATS: Stats = { size: 100, files: 3, dirs: 2, scanning: false };

function row(id: number, over: Partial<Row> = {}): Row {
  return {
    id,
    gen: 1,
    name: `item${id}`,
    size: 10,
    isDir: false,
    files: 1,
    complete: true,
    listed: true,
    ...over,
  };
}

function view(id: number, rows: Row[] = [], over: Partial<View> = {}): View {
  return { id, gen: 1, path: `/dir${id}`, size: 100, rows, hidden: 0, complete: true, ...over };
}

/** A promise the test settles by hand, to put a reply "in flight". */
function deferred<T>() {
  let resolve!: (v: T) => void;
  let reject!: (e: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

async function connected(root: View = view(0, [row(1), row(2)])) {
  api.connect.mockResolvedValue({ serial: "s", model: "Pixel", root: "/sdcard", previous: null });
  api.scan.mockResolvedValue(STATS);
  api.getView.mockResolvedValue(root);
  api.getBreadcrumbs.mockResolvedValue([{ id: root.id, gen: 1, name: "root" }]);
  api.getTreemap.mockResolvedValue({
    id: root.id,
    gen: 1,
    name: "root",
    size: 100,
    isDir: true,
    listed: true,
    children: [],
  });
  api.largestFiles.mockResolvedValue([]);
  api.typeBreakdown.mockResolvedValue([]);
  api.appBreakdown.mockResolvedValue([]);

  const notify = vi.fn();
  const hook = renderHook(() => useScanSession(notify));
  await act(() => hook.result.current.connect());
  expect(hook.result.current.phase).toBe("result");
  return { result: hook.result, notify };
}

beforeEach(() => {
  vi.resetAllMocks();
  api.onScanProgress.mockImplementation(() => Promise.resolve(() => {}));
});
afterEach(cleanup);

describe("search results", () => {
  it("are not shown after a rescan, even for the same query", async () => {
    const { result } = await connected();

    api.search.mockResolvedValueOnce([row(1, { name: "old-hit" })]);
    act(() => result.current.setQuery("foo"));
    await waitFor(() => expect(result.current.searchResults?.[0]?.name).toBe("old-hit"));

    await act(() => result.current.scan());
    expect(result.current.query).toBe("");

    // The next search never answers; whatever is shown must not be left over.
    api.search.mockReturnValueOnce(new Promise(() => {}));
    act(() => result.current.setQuery("foo"));
    await act(() => new Promise((r) => setTimeout(r, 250)));
    expect(result.current.searchResults).toBeNull();
  });

  it("from before a rescan are ignored when they arrive late", async () => {
    const { result } = await connected();

    const late = deferred<Row[]>();
    api.search.mockReturnValueOnce(late.promise);
    act(() => result.current.setQuery("foo"));
    await waitFor(() => expect(api.search).toHaveBeenCalledTimes(1));

    await act(() => result.current.scan());

    api.search.mockReturnValueOnce(new Promise(() => {}));
    act(() => result.current.setQuery("foo"));
    await waitFor(() => expect(api.search).toHaveBeenCalledTimes(2));

    await act(async () => late.resolve([row(1, { name: "old-hit" })]));
    expect(result.current.searchResults).toBeNull();
  });
});

describe("delete", () => {
  it("passes the row generation along with its id", async () => {
    const { result } = await connected();
    const target = row(2, { gen: 4 });
    api.deleteNode.mockResolvedValue({ items: 1, stats: STATS, view: view(0, [row(1)]) });

    await act(() => result.current.remove(target));

    expect(api.deleteNode).toHaveBeenCalledWith(2, 4);
  });

  it("moves to the surviving view when an ancestor of the open folder is deleted", async () => {
    const { result } = await connected();

    api.getView.mockResolvedValue(view(9, [row(10)]));
    await act(() => result.current.open(9).then(() => undefined));
    expect(result.current.view?.id).toBe(9);

    api.deleteNode.mockResolvedValue({ items: 5, stats: STATS, view: view(0, [row(1)]) });
    api.getBreadcrumbs.mockResolvedValue([{ id: 0, gen: 1, name: "root" }]);

    await act(() => result.current.remove(row(5, { isDir: true })));

    expect(result.current.view?.id).toBe(0);
    await waitFor(() => expect(result.current.crumbs).toEqual([{ id: 0, gen: 1, name: "root" }]));
    expect(api.getBreadcrumbs).toHaveBeenLastCalledWith(0);
    expect(api.getTreemap).toHaveBeenLastCalledWith(0, 2);
  });

  it("reports a stale-generation refusal and leaves the screen alone", async () => {
    const { result, notify } = await connected();
    const before = result.current.view;
    api.deleteNode.mockRejectedValue("Results are out of date — rescan.");

    await act(() => result.current.remove(row(2, { gen: 0 })));

    expect(notify).toHaveBeenCalledWith("Results are out of date — rescan.", "error");
    expect(result.current.view).toBe(before);
  });
});

describe("a scan that fails", () => {
  it("drops the old tree and offers a rescan instead of a mixed result", async () => {
    const { result, notify } = await connected();
    expect(result.current.view).not.toBeNull();

    api.scan.mockRejectedValue("the daemon closed the connection before the scan finished");
    await act(() => result.current.scan());

    expect(result.current.phase).toBe("failed");
    expect(result.current.failure).toMatch(/daemon closed/);
    expect(result.current.view).toBeNull();
    expect(result.current.stats).toBeNull();
    expect(result.current.crumbs).toEqual([]);
    expect(notify).toHaveBeenCalledWith(expect.stringMatching(/daemon closed/), "error");
  });

  it("fails when the root folder cannot be opened", async () => {
    const { result } = await connected();

    api.getView.mockRejectedValue("boom");
    await act(() => result.current.scan());

    expect(result.current.phase).toBe("failed");
    expect(result.current.view).toBeNull();
  });
});

describe("disconnect", () => {
  it("ignores a scan reply that arrives afterwards and resets the session", async () => {
    const { result } = await connected();

    const running = deferred<Stats>();
    api.scan.mockReturnValueOnce(running.promise);
    let scanning!: Promise<void>;
    act(() => {
      scanning = result.current.scan();
    });
    expect(result.current.phase).toBe("scanning");

    api.disconnect.mockResolvedValue();
    await act(() => result.current.disconnect());
    expect(result.current.phase).toBe("setup");

    await act(async () => {
      running.reject("the scan was cancelled");
      await scanning;
    });
    expect(result.current.phase).toBe("setup");
    expect(result.current.failure).toBeNull();
    expect(result.current.mode).toBe("treemap");
    expect(result.current.elapsedMs).toBeNull();
  });

  it("bumps the generation so id-bearing state elsewhere can tell it went stale", async () => {
    const { result } = await connected();
    const before = result.current.generation;

    api.disconnect.mockResolvedValue();
    await act(() => result.current.disconnect());

    expect(result.current.generation).toBeGreaterThan(before);
  });
});

describe("navigation", () => {
  it("lets the latest click win when replies arrive out of order", async () => {
    const { result } = await connected();

    const slow = deferred<View>();
    api.getView.mockReturnValueOnce(slow.promise).mockResolvedValueOnce(view(2, [row(20)]));
    api.getBreadcrumbs.mockResolvedValue([]);

    let first!: Promise<boolean>;
    act(() => {
      first = result.current.open(1);
    });
    await act(() => result.current.open(2).then(() => undefined));
    expect(result.current.view?.id).toBe(2);

    await act(async () => {
      slow.resolve(view(1));
      await first;
    });
    expect(result.current.view?.id).toBe(2);
  });
});
