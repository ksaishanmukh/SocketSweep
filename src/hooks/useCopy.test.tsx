// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, renderHook } from "@testing-library/react";
import { commands, type CopyResult } from "../api";
import {
  SESSION,
  commandError,
  copyProgress,
  deferred,
  emit,
  node,
  resetApi,
  row,
} from "../test/api";
import { targetOfRow } from "../lib/selection";
import type { Row } from "../api";
import { useCopy } from "./useCopy";

const pick = (r: Row) => targetOfRow(r, "/sdcard");

vi.mock("../api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../api")>()),
  ...(await import("../test/api")).apiMock,
}));

const api = vi.mocked(commands);

const RESULT: CopyResult = {
  destination: "C:\\Users\\me\\Desktop\\item2",
  files: 4,
  bytes: 4096,
  skipped: 0,
  renamed: 0,
};

function setup() {
  const notify = vi.fn();
  const log = vi.fn();
  const sessionRef = { current: SESSION as number | null };
  const hook = renderHook(() => useCopy({ sessionRef, log, notify }));
  return { ...hook, notify, log, sessionRef };
}

beforeEach(() => resetApi());
afterEach(cleanup);

describe("copy to computer", () => {
  it("copies by NodeRef, letting the host choose the destination", async () => {
    const { result } = setup();
    api.copyMany.mockResolvedValue(RESULT);

    await act(() => result.current.start([pick(row(2))]));

    expect(api.copyMany).toHaveBeenCalledWith([node(2)]);
  });

  it("follows copy-progress events while the copy runs, then toasts success", async () => {
    const { result, notify } = setup();
    const running = deferred<CopyResult>();
    api.copyMany.mockReturnValue(running.promise);

    let copying!: Promise<void>;
    act(() => {
      copying = result.current.start([pick(row(2, { name: "DCIM" }))]);
    });
    expect(result.current.copy).toMatchObject({ name: "DCIM", progress: null, cancelling: false });

    act(() => emit.copyProgress(copyProgress({ filesDone: 1, filesTotal: 4 })));
    expect(result.current.copy?.progress).toMatchObject({ filesDone: 1, filesTotal: 4 });

    act(() => emit.copyProgress(copyProgress({ filesDone: 3, bytesDone: 3072 })));
    expect(result.current.copy?.progress?.filesDone).toBe(3);

    await act(async () => {
      running.resolve(RESULT);
      await copying;
    });

    expect(result.current.copy).toBeNull();
    expect(notify).toHaveBeenCalledWith(expect.stringContaining("Copied DCIM to"), "success");
    expect(notify).toHaveBeenCalledWith(expect.stringContaining(RESULT.destination), "success");
    expect(notify).toHaveBeenCalledWith(expect.stringContaining("4 files"), "success");
  });

  it("mentions links that were not copied and files that were renamed", async () => {
    const { result, notify } = setup();
    api.copyMany.mockResolvedValue({ ...RESULT, skipped: 2, renamed: 1 });

    await act(() => result.current.start([pick(row(2))]));

    expect(notify).toHaveBeenCalledWith(
      expect.stringMatching(/2 links not copied, 1 renamed/),
      "success",
    );
  });

  it("ignores progress for something other than the copy it started", async () => {
    const { result } = setup();
    api.copyMany.mockReturnValue(new Promise(() => {}));
    act(() => {
      void result.current.start([pick(row(2))]);
    });

    act(() => emit.copyProgress(copyProgress({ nodes: [node(77)], filesDone: 9 })));

    expect(result.current.copy?.progress).toBeNull();
  });

  it("asks the host to stop on cancel, and reports a cancelled copy as information", async () => {
    const { result, notify } = setup();
    const running = deferred<CopyResult>();
    api.copyMany.mockReturnValue(running.promise);
    let copying!: Promise<void>;
    act(() => {
      copying = result.current.start([pick(row(2))]);
    });

    act(() => result.current.cancel());
    expect(api.cancelCopy).toHaveBeenCalledOnce();
    expect(result.current.copy?.cancelling).toBe(true);

    await act(async () => {
      running.reject(commandError("cancelled", "Copy cancelled."));
      await copying;
    });

    expect(result.current.copy).toBeNull();
    expect(notify).toHaveBeenCalledWith(expect.stringMatching(/cancelled/i), "info");
    expect(notify).not.toHaveBeenCalledWith(expect.anything(), "error");
  });

  it("toasts the host's message when the copy fails", async () => {
    const { result, notify } = setup();
    api.copyMany.mockRejectedValue(commandError("io", "Cannot use C:\\Desktop: disk full"));

    await act(() => result.current.start([pick(row(2))]));

    expect(result.current.copy).toBeNull();
    expect(notify).toHaveBeenCalledWith("Copy failed: Cannot use C:\\Desktop: disk full", "error");
  });

  it("runs one copy at a time", async () => {
    const { result, notify } = setup();
    api.copyMany.mockReturnValue(new Promise(() => {}));
    act(() => {
      void result.current.start([pick(row(2))]);
    });

    await act(() => result.current.start([pick(row(3))]));

    expect(api.copyMany).toHaveBeenCalledOnce();
    expect(notify).toHaveBeenCalledWith(expect.stringMatching(/already running/), "info");
  });

  it("refuses a row from another session without asking the host", async () => {
    const { result, notify } = setup();

    await act(() => result.current.start([pick(row(2, { node: node(2, SESSION + 1) }))]));

    expect(api.copyMany).not.toHaveBeenCalled();
    expect(notify).toHaveBeenCalledWith(expect.stringMatching(/out of date/), "error");
  });

  it("copies several picks as one copyMany, followed by its single progress stream", async () => {
    const { result, notify } = setup();
    const running = deferred<CopyResult>();
    api.copyMany.mockReturnValue(running.promise);
    let copying!: Promise<void>;
    act(() => {
      copying = result.current.start([pick(row(2)), pick(row(3))]);
    });

    expect(api.copyMany).toHaveBeenCalledWith([node(2), node(3)]);
    expect(result.current.copy?.name).toBe("2 items");
    act(() => emit.copyProgress(copyProgress({ nodes: [node(2), node(3)], filesDone: 2 })));
    expect(result.current.copy?.progress?.filesDone).toBe(2);

    await act(async () => {
      running.resolve({ ...RESULT, destination: "C:\\Users\\me\\Desktop" });
      await copying;
    });
    expect(notify).toHaveBeenCalledWith(expect.stringContaining("Copied 2 items to"), "success");
  });

  it("forgets the copy when the session is torn down", async () => {
    const { result } = setup();
    api.copyMany.mockReturnValue(new Promise(() => {}));
    act(() => {
      void result.current.start([pick(row(2))]);
    });
    expect(result.current.copy).not.toBeNull();

    act(() => result.current.reset());

    expect(result.current.copy).toBeNull();
  });
});
