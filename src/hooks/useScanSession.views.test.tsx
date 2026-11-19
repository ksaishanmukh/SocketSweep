// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { commands, type Overview, type Row, type View } from "../api";
import { targetOfLeftover, targetOfRow } from "../lib/selection";
import {
  DEFAULT_SORT,
  NO_FILTER,
  SESSION,
  connected,
  deferred,
  emit,
  node,
  overview,
  progress,
  resetApi,
  row,
  size,
  started,
  totals,
  treemapNode,
  view,
} from "../test/api";
import { useScanSession } from "./useScanSession";

vi.mock("../api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../api")>()),
  ...(await import("../test/api")).apiMock,
}));

const api = vi.mocked(commands);

async function browse(root: View = view(0, [row(1), row(2)])) {
  api.connect.mockResolvedValue(connected());
  api.startScan.mockResolvedValue(started());
  api.getView.mockResolvedValue(root);
  api.getBreadcrumbs.mockResolvedValue([{ node: root.node, name: "root" }]);
  api.getTreemap.mockResolvedValue(treemapNode(root.node.id));
  api.overview.mockResolvedValue(overview());

  const notify = vi.fn();
  const hook = renderHook(() => useScanSession(notify));
  await act(() => hook.result.current.connect());
  await act(async () => emit.scanProgress(progress()));
  await waitFor(() => expect(hook.result.current.phase).toBe("browsing"));
  api.focus.mockClear();
  api.getView.mockClear();
  return { result: hook.result, notify };
}

const target = (r: Row) => targetOfRow(r, "/dir0");

beforeEach(() => resetApi());
afterEach(cleanup);

describe("sorting and filtering", () => {
  it("asks the host for the new order, starting a column in its natural direction", async () => {
    const { result } = await browse();

    act(() => result.current.controls.setSortKey("name"));
    const byName = { key: "name", descending: false };
    await waitFor(() =>
      expect(api.getView).toHaveBeenLastCalledWith({
        node: node(0),
        by: "disk",
        sort: byName,
        filter: NO_FILTER,
      }),
    );
    expect(api.focus).toHaveBeenLastCalledWith({
      node: node(0),
      by: "disk",
      sort: byName,
      filter: NO_FILTER,
    });

    // The active column flips.
    act(() => result.current.controls.setSortKey("name"));
    await waitFor(() =>
      expect(api.getView).toHaveBeenLastCalledWith({
        node: node(0),
        by: "disk",
        sort: { key: "name", descending: true },
        filter: NO_FILTER,
      }),
    );
  });

  it("sends the filters to getView and focus, and clears them again", async () => {
    const { result } = await browse();

    act(() => result.current.controls.toggleFilter({ group: "categories", value: "video" }));
    act(() => result.current.controls.toggleFilter({ group: "changed", value: "overYear" }));
    const filtered = { ...NO_FILTER, categories: ["video"], changed: ["overYear"] };
    await waitFor(() =>
      expect(api.getView).toHaveBeenLastCalledWith({
        node: node(0),
        by: "disk",
        sort: DEFAULT_SORT,
        filter: filtered,
      }),
    );
    expect(api.focus).toHaveBeenLastCalledWith({
      node: node(0),
      by: "disk",
      sort: DEFAULT_SORT,
      filter: filtered,
    });

    act(() => result.current.controls.clearFilters());
    await waitFor(() =>
      expect(api.getView).toHaveBeenLastCalledWith({
        node: node(0),
        by: "disk",
        sort: DEFAULT_SORT,
        filter: NO_FILTER,
      }),
    );
  });

  it("keeps the filters when another folder opens", async () => {
    const { result } = await browse();
    act(() => result.current.controls.toggleFilter({ group: "kinds", value: "file" }));

    await act(async () => {
      await result.current.actions.openFolder(node(4));
    });

    expect(api.getView).toHaveBeenLastCalledWith({
      node: node(4),
      by: "disk",
      sort: DEFAULT_SORT,
      filter: {
        ...NO_FILTER,
        kinds: ["file"],
      },
    });
  });
});

describe("the search box", () => {
  it("narrows the open folder once typing pauses, and empties on opening another folder", async () => {
    const { result } = await browse();

    act(() => result.current.options.setText("cam"));
    await waitFor(() =>
      expect(api.getView).toHaveBeenLastCalledWith({
        node: node(0),
        by: "disk",
        sort: DEFAULT_SORT,
        filter: {
          ...NO_FILTER,
          query: "cam",
        },
      }),
    );
    expect(api.search).not.toHaveBeenCalled();

    await act(async () => {
      await result.current.actions.openFolder(node(4));
    });
    expect(result.current.options.text).toBe("");
    expect(api.getView).toHaveBeenLastCalledWith({
      node: node(4),
      by: "disk",
      sort: DEFAULT_SORT,
      filter: NO_FILTER,
    });
  });

  it("searches the whole phone instead with Everywhere, leaving the folder whole", async () => {
    const { result } = await browse();
    api.search.mockResolvedValue([row(9, { parent: "/sdcard/DCIM" })]);

    act(() => result.current.options.setEverywhere(true));
    act(() => result.current.options.setText("cam"));

    await waitFor(() => expect(result.current.searchResults).toHaveLength(1));
    expect(api.search.mock.calls[0][0].within).toBeNull();
    expect(result.current.options.folderQuery).toBeNull();
  });
});

describe("history", () => {
  it("goes back and forward through opened folders", async () => {
    const { result } = await browse();
    api.getView.mockImplementation(async ({ node }) => view(node!.id));

    await act(async () => {
      await result.current.actions.openFolder(node(4));
    });
    await act(async () => {
      await result.current.actions.openFolder(node(5));
    });
    expect(result.current.nav.canBack).toBe(true);

    await act(() => result.current.nav.back());
    expect(result.current.nav.view?.node.id).toBe(4);
    expect(result.current.nav.canForward).toBe(true);

    await act(() => result.current.nav.forward());
    expect(result.current.nav.view?.node.id).toBe(5);
    expect(result.current.nav.canForward).toBe(false);
  });

  it("is forgotten with the tree", async () => {
    const { result } = await browse();
    await act(async () => {
      await result.current.actions.openFolder(node(4));
    });

    api.startScan.mockResolvedValue(started(SESSION + 1));
    await act(() => result.current.scan());

    expect(result.current.nav.canBack).toBe(false);
  });
});

describe("jumps from the overview", () => {
  it("shows a category as the root narrowed to that type, in the Explorer", async () => {
    const { result } = await browse();
    act(() => result.current.options.setText("old"));

    await act(() => result.current.actions.showCategory("photo"));

    expect(result.current.mode).toBe("explorer");
    expect(api.getView).toHaveBeenLastCalledWith({
      node: node(0),
      by: "disk",
      sort: DEFAULT_SORT,
      filter: {
        ...NO_FILTER,
        categories: ["photo"],
      },
    });
  });
});

describe("the overview", () => {
  it("drops a reply that comes back after a rescan has replaced the tree", async () => {
    const { result } = await browse();
    const late = deferred<Overview>();
    api.overview.mockReturnValueOnce(late.promise);
    act(() => result.current.actions.setMode("overview"));

    api.startScan.mockResolvedValue(started(SESSION + 1));
    await act(() => result.current.scan());
    await act(async () => late.resolve(overview({ biggestFiles: [row(9)] })));

    expect(result.current.overview).toBeNull();
  });
});

describe("apps-updated", () => {
  it("fetches the overview and the open folder again, so names appear", async () => {
    const { result } = await browse();
    api.overview.mockClear();
    api.getView.mockResolvedValue(
      view(0, [row(1, { app: { package: "com.a", label: "A", installed: true, hasIcon: false } })]),
    );

    act(() => emit.appsUpdated({ session: SESSION, status: "ready", icons: 0 }));

    await waitFor(() => expect(result.current.nav.view?.rows[0].app?.label).toBe("A"));
    expect(api.overview).toHaveBeenCalled();
  });

  it("ignores an update for another session", async () => {
    await browse();
    api.overview.mockClear();

    act(() => emit.appsUpdated({ session: SESSION + 1, status: "ready", icons: 3 }));

    await new Promise((r) => setTimeout(r, 20));
    expect(api.overview).not.toHaveBeenCalled();
    expect(api.getView).not.toHaveBeenCalled();
  });
});

describe("batch delete", () => {
  it("deletes the whole queue in one call and forgets the picks that went", async () => {
    const { result, notify } = await browse();
    const a = target(row(1, { name: "a.bin" }));
    const b = target(row(2, { name: "b.bin" }));
    const leftover = targetOfLeftover({
      package: "com.gone",
      size: size(50),
      folders: [node(7), node(8)],
    });
    act(() => result.current.selection.setMany([a, b, leftover], true));
    expect(result.current.selection.items).toHaveLength(3);

    api.deleteStaged.mockResolvedValue({
      deleted: 3,
      items: 9,
      size: size(70),
      failures: [
        {
          node: node(2),
          path: "/dir0/b.bin",
          error: { code: "daemon", message: "Permission denied" },
        },
      ],
      totals: totals(),
      view: view(0, [row(2)]),
    });
    await act(() => result.current.remove(result.current.selection.items));

    expect(api.deleteStaged).toHaveBeenCalledOnce();
    expect(result.current.selection.items.map((t) => t.key)).toEqual([b.key]);
    expect(notify).toHaveBeenCalledWith(
      expect.stringMatching(
        /Deleted 3 of 4 items\. Could not delete \/dir0\/b\.bin \(Permission denied\)/,
      ),
      "error",
    );
  });

  it("never shows or acts on picks from a replaced tree", async () => {
    const { result } = await browse();
    act(() => result.current.selection.setMany([target(row(1))], true));

    api.startScan.mockResolvedValue(started(SESSION + 1));
    await act(() => result.current.scan());

    expect(result.current.selection.items).toEqual([]);
  });
});
