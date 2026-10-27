// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { commands } from "../api";
import {
  SESSION,
  commandError,
  connected,
  deferred,
  emit,
  listenerCount,
  node,
  progress,
  resetApi,
  row,
  size,
  started,
  totals,
  treemapNode,
  view,
} from "../test/api";
import type { Row, ScanStarted, View } from "../api";
import { useScanSession } from "./useScanSession";

vi.mock("../api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../api")>()),
  ...(await import("../test/api")).apiMock,
}));

const api = vi.mocked(commands);

/** Connects, starts the scan, and lets the skeleton arrive: the user is browsing. */
async function browse(root: View = view(0, [row(1), row(2)])) {
  api.connect.mockResolvedValue(connected());
  api.startScan.mockResolvedValue(started());
  api.getView.mockResolvedValue(root);
  api.getBreadcrumbs.mockResolvedValue([{ node: root.node, name: "root" }]);
  api.getTreemap.mockResolvedValue(treemapNode(root.node.id));
  api.largestFiles.mockResolvedValue([]);
  api.typeBreakdown.mockResolvedValue([]);
  api.appBreakdown.mockResolvedValue([]);

  const notify = vi.fn();
  const hook = renderHook(() => useScanSession(notify));
  await act(() => hook.result.current.connect());
  await act(async () => emit.scanProgress(progress()));
  await waitFor(() => expect(hook.result.current.phase).toBe("browsing"));
  return { result: hook.result, notify, unmount: hook.unmount };
}

beforeEach(() => resetApi());
afterEach(cleanup);

describe("connecting and scanning", () => {
  it("is browsable as soon as the skeleton is listed, while sizes are still filling in", async () => {
    const { result } = await browse();

    expect(result.current.phase).toBe("browsing");
    expect(result.current.view?.rows).toHaveLength(2);
    expect(result.current.scanInfo?.running).toBe(true);
    expect(result.current.totals?.files).toBe(3);
    expect(api.startScan).toHaveBeenCalledOnce();
  });

  it("waits on the setup screen, with a status, until the skeleton arrives", async () => {
    api.connect.mockResolvedValue(connected());
    api.startScan.mockResolvedValue(started());
    const { result } = renderHook(() => useScanSession(vi.fn()));

    await act(() => result.current.connect());

    expect(result.current.phase).toBe("connecting");
    expect(result.current.status).toBe("Starting scan…");
    expect(api.getView).not.toHaveBeenCalled();
  });

  it("opens the root with focus, so the phone measures it first", async () => {
    await browse();

    expect(api.focus).toHaveBeenCalledWith({ session: SESSION, id: 0 });
    expect(api.getView).toHaveBeenCalledWith({ session: SESSION, id: 0 }, undefined, "disk");
  });

  it("keeps an event that beats startScan's own reply", async () => {
    api.connect.mockResolvedValue(connected());
    const answer = deferred<ScanStarted>();
    api.startScan.mockReturnValue(answer.promise);
    api.getView.mockResolvedValue(view(0, [row(1)]));
    api.getBreadcrumbs.mockResolvedValue([]);
    api.getTreemap.mockResolvedValue(treemapNode(0));
    api.largestFiles.mockResolvedValue([]);
    api.typeBreakdown.mockResolvedValue([]);
    api.appBreakdown.mockResolvedValue([]);
    const { result } = renderHook(() => useScanSession(vi.fn()));

    let connecting!: Promise<void>;
    act(() => {
      connecting = result.current.connect();
    });
    await waitFor(() => expect(api.startScan).toHaveBeenCalled());
    await act(async () => emit.scanProgress(progress()));
    expect(result.current.phase).toBe("connecting");

    await act(async () => {
      answer.resolve(started());
      await connecting;
    });
    await waitFor(() => expect(result.current.phase).toBe("browsing"));
  });

  it("ignores progress from a session that is not the current one", async () => {
    const { result } = await browse();

    act(() =>
      emit.scanProgress(progress({ session: 99, totals: totals({ files: 12345 }), view: null })),
    );

    expect(result.current.totals?.files).toBe(3);
  });

  it("shows the focused folder's refreshed rows, but not those of a folder that is not open", async () => {
    const { result } = await browse();

    act(() => emit.scanProgress(progress({ view: view(0, [row(1), row(2), row(3)]) })));
    expect(result.current.view?.rows).toHaveLength(3);

    act(() => emit.scanProgress(progress({ view: view(7, [row(70)]) })));
    expect(result.current.view?.node.id).toBe(0);
  });

  it("says the scan ended, and with what outcome", async () => {
    const { result } = await browse();

    act(() =>
      emit.scanProgress(
        progress({
          phase: "finished",
          finished: { outcome: "complete", elapsedMs: 4200, message: null },
        }),
      ),
    );

    expect(result.current.scanInfo).toMatchObject({
      running: false,
      outcome: "complete",
      elapsedMs: 4200,
    });
    // Everything is redrawn from final numbers.
    await waitFor(() => expect(api.largestFiles.mock.calls.length).toBeGreaterThan(1));
  });

  it("keeps browsing when the scan fails after the skeleton, and flags the outcome", async () => {
    const { result, notify } = await browse();

    act(() =>
      emit.scanProgress(
        progress({
          phase: "finished",
          finished: { outcome: "failed", elapsedMs: 900, message: "the daemon died" },
        }),
      ),
    );

    expect(result.current.phase).toBe("browsing");
    expect(result.current.scanInfo).toMatchObject({ running: false, outcome: "failed" });
    expect(notify).toHaveBeenCalledWith("the daemon died", "error");
  });

  it("goes to the failed screen when the scan ends before anything was listed", async () => {
    api.connect.mockResolvedValue(connected());
    api.startScan.mockResolvedValue(started());
    const { result } = renderHook(() => useScanSession(vi.fn()));
    await act(() => result.current.connect());

    act(() =>
      emit.scanProgress(
        progress({
          skeleton: null,
          phase: "finished",
          finished: { outcome: "failed", elapsedMs: 10, message: "permission denied" },
        }),
      ),
    );

    expect(result.current.phase).toBe("failed");
    expect(result.current.failure).toBe("permission denied");
  });

  it("goes to the failed screen when startScan is refused", async () => {
    api.connect.mockResolvedValue(connected());
    api.startScan.mockRejectedValue(commandError("daemon", "The phone did not start the scan."));
    const { result } = renderHook(() => useScanSession(vi.fn()));

    await act(() => result.current.connect());

    expect(result.current.phase).toBe("failed");
    expect(result.current.failure).toBe("The phone did not start the scan.");
  });

  it("returns to setup with the host's message when connect fails", async () => {
    api.connect.mockRejectedValue(commandError("device", "No phone is plugged in."));
    const notify = vi.fn();
    const { result } = renderHook(() => useScanSession(notify));

    await act(() => result.current.connect());

    expect(result.current.phase).toBe("setup");
    expect(notify).toHaveBeenCalledWith("No phone is plugged in.", "error");
  });

  it("stops listening when it unmounts", async () => {
    const { unmount } = await browse();
    expect(listenerCount().scanProgress).toBe(1);

    unmount();

    await waitFor(() =>
      expect(listenerCount()).toEqual({ scanProgress: 0, copyProgress: 0, connectionLost: 0 }),
    );
  });
});

describe("navigation", () => {
  it("calls focus for the folder it opens, then loads it by the chosen metric", async () => {
    const { result } = await browse();
    api.focus.mockClear();
    api.getView.mockResolvedValue(view(9, [row(10)]));
    api.getBreadcrumbs.mockResolvedValue([{ node: node(9), name: "nine" }]);

    await act(async () => {
      await result.current.open(node(9));
    });

    expect(api.focus).toHaveBeenCalledWith(node(9));
    expect(api.getView).toHaveBeenLastCalledWith(node(9), undefined, "disk");
    expect(api.getBreadcrumbs).toHaveBeenLastCalledWith(node(9));
    await waitFor(() => expect(api.getTreemap).toHaveBeenLastCalledWith(node(9), 2, "disk"));
    expect(result.current.view?.node).toEqual(node(9));
  });

  it("focuses the destination of a search reveal", async () => {
    const { result } = await browse();
    api.focus.mockClear();
    api.getView.mockResolvedValue(view(5, []));

    await act(async () => {
      await result.current.reveal(row(8, { parent: "/dir5", parentNode: node(5) }));
    });

    expect(api.focus).toHaveBeenCalledWith(node(5));
    expect(result.current.mode).toBe("treemap");
  });

  it("lets the latest click win when replies arrive out of order", async () => {
    const { result } = await browse();

    const slow = deferred<View>();
    api.getView.mockReturnValueOnce(slow.promise).mockResolvedValueOnce(view(2, [row(20)]));
    api.getBreadcrumbs.mockResolvedValue([]);

    let first!: Promise<boolean>;
    act(() => {
      first = result.current.open(node(1));
    });
    await act(async () => {
      await result.current.open(node(2));
    });
    expect(result.current.view?.node.id).toBe(2);

    await act(async () => {
      slow.resolve(view(1));
      await first;
    });
    expect(result.current.view?.node.id).toBe(2);
  });

  it("refuses to open a node from another session, without asking the host", async () => {
    const { result } = await browse();
    api.focus.mockClear();
    api.getView.mockClear();

    let opened = true;
    await act(async () => {
      opened = await result.current.open(node(3, SESSION + 1));
    });

    expect(opened).toBe(false);
    expect(api.focus).not.toHaveBeenCalled();
    expect(api.getView).not.toHaveBeenCalled();
  });

  it("drops a reply that comes back after a rescan has replaced the tree", async () => {
    const { result } = await browse();

    const late = deferred<View>();
    api.getView.mockReturnValueOnce(late.promise);
    let opening!: Promise<boolean>;
    act(() => {
      opening = result.current.open(node(4));
    });

    // Rescan: a new session, so node 4 of the old tree names nothing now.
    api.startScan.mockResolvedValue(started(SESSION + 1));
    await act(() => result.current.scan());

    await act(async () => {
      late.resolve(view(4, [row(40)]));
      expect(await opening).toBe(false);
    });
    expect(result.current.view).toBeNull();
  });

  it("asks the host for the next page and keeps the folder's rows", async () => {
    const many = Array.from({ length: 3 }, (_, i) => row(i + 1));
    const { result } = await browse(view(0, many, { hidden: 4 }));

    api.getView.mockResolvedValue(view(0, [...many, row(4)], { hidden: 3 }));
    await act(() => result.current.loadMore());

    expect(api.getView).toHaveBeenLastCalledWith(node(0), 503, "disk");
    expect(result.current.view?.rows).toHaveLength(4);
  });
});

describe("sizes", () => {
  it("shows disk usage by default and re-queries with apparent size on toggle", async () => {
    const { result } = await browse();
    expect(result.current.metric).toBe("disk");

    api.getView.mockClear();
    api.getTreemap.mockClear();
    api.largestFiles.mockClear();
    api.typeBreakdown.mockClear();
    api.appBreakdown.mockClear();
    act(() => result.current.setMetric("apparent"));

    expect(result.current.metric).toBe("apparent");
    await waitFor(() => expect(api.getView).toHaveBeenCalledWith(node(0), 2, "apparent"));
    await waitFor(() => expect(api.getTreemap).toHaveBeenCalledWith(node(0), 2, "apparent"));
    await waitFor(() => expect(api.largestFiles).toHaveBeenCalledWith(100, "apparent"));
    expect(api.typeBreakdown).toHaveBeenCalledWith("apparent");
    expect(api.appBreakdown).toHaveBeenCalledWith(50, "apparent");
  });
});

describe("search", () => {
  it("is not shown after a rescan, even for the same query", async () => {
    const { result } = await browse();

    api.search.mockResolvedValueOnce([row(1, { name: "old-hit" })]);
    act(() => result.current.setQuery("foo"));
    await waitFor(() => expect(result.current.searchResults?.[0]?.name).toBe("old-hit"));

    api.startScan.mockResolvedValue(started(SESSION + 1));
    await act(() => result.current.scan());
    expect(result.current.query).toBe("");

    // The next search never answers; whatever is shown must not be left over.
    api.search.mockReturnValueOnce(new Promise(() => {}));
    act(() => result.current.setQuery("foo"));
    await act(() => new Promise((r) => setTimeout(r, 250)));
    expect(result.current.searchResults).toBeNull();
  });

  it("from before a rescan are ignored when they arrive late", async () => {
    const { result } = await browse();

    const late = deferred<Row[]>();
    api.search.mockReturnValueOnce(late.promise);
    act(() => result.current.setQuery("foo"));
    await waitFor(() => expect(api.search).toHaveBeenCalledTimes(1));

    api.startScan.mockResolvedValue(started(SESSION + 1));
    await act(() => result.current.scan());
    // The new session's skeleton arrives, so there is a tree to search again.
    api.getView.mockResolvedValue(view(0, [row(1)], { node: node(0, SESSION + 1) }));
    api.getBreadcrumbs.mockResolvedValue([{ node: node(0, SESSION + 1), name: "root" }]);
    api.getTreemap.mockResolvedValue(treemapNode(0, { node: node(0, SESSION + 1) }));
    await act(async () => emit.scanProgress(progress({ session: SESSION + 1 })));
    await waitFor(() => expect(result.current.phase).toBe("browsing"));

    api.search.mockReturnValueOnce(new Promise(() => {}));
    act(() => result.current.setQuery("foo"));
    await waitFor(() => expect(api.search).toHaveBeenCalledTimes(2));

    await act(async () => late.resolve([row(1, { name: "old-hit" })]));
    expect(result.current.searchResults).toBeNull();
  });
});

describe("delete", () => {
  it("deletes by the row's NodeRef and shows the totals and view the host returns", async () => {
    const { result } = await browse();
    const target = row(2, { node: node(2) });
    api.delete.mockResolvedValue({
      items: 1,
      totals: totals({ files: 2, size: size(90) }),
      view: view(0, [row(1)]),
    });

    await act(() => result.current.remove(target));

    expect(api.delete).toHaveBeenCalledWith(node(2));
    expect(result.current.totals?.files).toBe(2);
    expect(result.current.view?.rows).toHaveLength(1);
  });

  it("moves to the surviving view when an ancestor of the open folder is deleted", async () => {
    const { result } = await browse();

    api.getView.mockResolvedValue(view(9, [row(10)]));
    await act(async () => {
      await result.current.open(node(9));
    });
    expect(result.current.view?.node.id).toBe(9);

    api.delete.mockResolvedValue({ items: 5, totals: totals(), view: view(0, [row(1)]) });
    api.getBreadcrumbs.mockResolvedValue([{ node: node(0), name: "root" }]);
    await act(() => result.current.remove(row(5, { kind: "dir" })));

    expect(result.current.view?.node.id).toBe(0);
    await waitFor(() => expect(result.current.crumbs).toEqual([{ node: node(0), name: "root" }]));
    expect(api.getBreadcrumbs).toHaveBeenLastCalledWith(node(0));
    expect(api.getTreemap).toHaveBeenLastCalledWith(node(0), 2, "disk");
  });

  it("re-fetches the rows that were loaded when the host returns a capped view", async () => {
    const many = Array.from({ length: 4 }, (_, i) => row(i + 1));
    const { result } = await browse(view(0, many, { hidden: 6 }));
    expect(result.current.view?.rows).toHaveLength(4);

    api.delete.mockResolvedValue({
      items: 1,
      totals: totals(),
      view: view(0, many.slice(1, 3), { hidden: 7 }),
    });
    api.getView.mockResolvedValue(view(0, many.slice(1), { hidden: 6 }));
    await act(() => result.current.remove(many[0]));

    await waitFor(() => expect(result.current.view?.rows).toHaveLength(3));
    expect(api.getView).toHaveBeenLastCalledWith(node(0), 4, "disk");
  });

  it("drops search hits inside a deleted folder, but not look-alike siblings", async () => {
    const { result } = await browse();
    api.search.mockResolvedValue([
      row(7, { name: "a", parent: "/dir0/photos" }),
      row(8, { name: "b", parent: "/dir0/photos/2020" }),
      row(9, { name: "c", parent: "/dir0/photos-old" }),
      row(10, { name: "photos", kind: "dir", parent: "/dir0" }),
    ]);
    act(() => result.current.setQuery("a"));
    await waitFor(() => expect(result.current.searchResults).toHaveLength(4));

    api.delete.mockResolvedValue({ items: 3, totals: totals(), view: view(0, [row(1)]) });
    api.search.mockReturnValue(new Promise(() => {}));
    await act(() =>
      result.current.remove(
        row(10, { name: "photos", kind: "dir", parent: "/dir0" }),
        "/dir0/photos",
      ),
    );

    expect(result.current.searchResults?.map((r) => r.node.id)).toEqual([9]);
  });

  it("reports a stale refusal and refreshes what is on screen", async () => {
    const { result, notify } = await browse();
    api.delete.mockRejectedValue(commandError("stale", "Results are out of date — rescan."));
    api.getView.mockClear();

    await act(() => result.current.remove(row(2)));

    expect(notify).toHaveBeenCalledWith("Results are out of date — rescan.", "error");
    await waitFor(() => expect(api.getView).toHaveBeenCalled());
  });

  it("does not even ask the host to delete a row from another session", async () => {
    const { result, notify } = await browse();

    await act(() => result.current.remove(row(2, { node: node(2, SESSION + 1) })));

    expect(api.delete).not.toHaveBeenCalled();
    expect(notify).toHaveBeenCalledWith(expect.stringMatching(/out of date/), "error");
  });
});

describe("disconnect and connection loss", () => {
  it("ignores a startScan reply that arrives after a disconnect", async () => {
    api.connect.mockResolvedValue(connected());
    const answer = deferred<ScanStarted>();
    api.startScan.mockReturnValue(answer.promise);
    const { result } = renderHook(() => useScanSession(vi.fn()));

    let connecting!: Promise<void>;
    act(() => {
      connecting = result.current.connect();
    });
    await waitFor(() => expect(api.startScan).toHaveBeenCalled());

    await act(() => result.current.disconnect());
    expect(result.current.phase).toBe("setup");

    await act(async () => {
      answer.resolve(started());
      await connecting;
    });
    expect(result.current.phase).toBe("setup");
    expect(result.current.sessionId).toBeNull();
  });

  it("clears the session on disconnect", async () => {
    const { result } = await browse();
    expect(result.current.sessionId).toBe(SESSION);

    await act(() => result.current.disconnect());

    expect(api.disconnect).toHaveBeenCalled();
    expect(result.current.phase).toBe("setup");
    expect(result.current.sessionId).toBeNull();
    expect(result.current.view).toBeNull();
    expect(result.current.totals).toBeNull();
    expect(result.current.mode).toBe("treemap");
  });

  it("goes back to setup with a toast when the connection is lost", async () => {
    const { result, notify } = await browse();

    act(() => emit.connectionLost({ serial: "s", message: "The phone stopped answering." }));

    expect(result.current.phase).toBe("setup");
    expect(result.current.view).toBeNull();
    expect(result.current.sessionId).toBeNull();
    expect(notify).toHaveBeenCalledWith(
      expect.stringContaining("The phone stopped answering."),
      "error",
    );
  });

  it("drops replies still in flight when the connection is lost", async () => {
    const { result } = await browse();
    const slow = deferred<View>();
    api.getView.mockReturnValueOnce(slow.promise);
    let opening!: Promise<boolean>;
    act(() => {
      opening = result.current.open(node(6));
    });

    act(() => emit.connectionLost({ serial: "s", message: "unplugged" }));
    await act(async () => {
      slow.resolve(view(6));
      await opening;
    });

    expect(result.current.view).toBeNull();
    expect(result.current.phase).toBe("setup");
  });

  it("does not toast a loss that follows the user's own disconnect", async () => {
    const { result, notify } = await browse();
    await act(() => result.current.disconnect());
    notify.mockClear();

    act(() => emit.connectionLost({ serial: "s", message: "gone" }));

    expect(notify).not.toHaveBeenCalled();
  });
});
