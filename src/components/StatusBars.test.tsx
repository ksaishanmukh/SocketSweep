// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ScanInfo } from "../hooks/useScanSession";
import type { CopyState } from "../hooks/useCopy";
import { copyProgress, node } from "../test/api";
import { CopyBar } from "./CopyBar";
import { ScanStatus } from "./ScanStatus";

afterEach(cleanup);

function scan(over: Partial<ScanInfo> = {}): ScanInfo {
  return {
    phase: "measuring",
    running: true,
    listed: 40,
    known: 100,
    elapsedMs: 2500,
    focusSettled: false,
    outcome: null,
    message: null,
    ...over,
  };
}

describe("ScanStatus", () => {
  it("shows progress and a Stop button while the scan runs", async () => {
    const user = userEvent.setup();
    const onStop = vi.fn();
    render(<ScanStatus scan={scan()} onStop={onStop} onRescan={() => {}} />);

    expect(screen.getByText("Measuring sizes…")).toBeTruthy();
    expect(screen.getByText("40 of 100 folders")).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "Stop" }));
    expect(onStop).toHaveBeenCalledOnce();
  });

  it("disappears after a complete scan", () => {
    const { container } = render(
      <ScanStatus
        scan={scan({ running: false, outcome: "complete" })}
        onStop={() => {}}
        onRescan={() => {}}
      />,
    );
    expect(container.textContent).toBe("");
  });

  it("explains an incomplete scan and offers a rescan", async () => {
    const user = userEvent.setup();
    const onRescan = vi.fn();
    render(
      <ScanStatus
        scan={scan({ running: false, outcome: "failed", message: "the daemon died" })}
        onStop={() => {}}
        onRescan={onRescan}
      />,
    );

    expect(screen.getByText(/The scan failed: the daemon died/)).toBeTruthy();
    expect(screen.getByText(/Sizes shown are incomplete/)).toBeTruthy();
    await user.click(screen.getByRole("button", { name: /Rescan/ }));
    expect(onRescan).toHaveBeenCalledOnce();
  });
});

function copying(over: Partial<CopyState> = {}): CopyState {
  return { node: node(2), name: "DCIM", progress: copyProgress(), cancelling: false, ...over };
}

describe("CopyBar", () => {
  it("shows files, bytes and destination from copy-progress", () => {
    render(<CopyBar copy={copying()} onCancel={() => {}} />);

    expect(screen.getByText(/1 of 4 files/)).toBeTruthy();
    expect(screen.getByText(/1\.0 KB of 4\.0 KB/)).toBeTruthy();
    expect(screen.getByText(/Desktop/)).toBeTruthy();
    expect(screen.getByRole("progressbar").getAttribute("aria-valuenow")).toBe("25");
  });

  it("is indeterminate while the phone is still listing what to copy", () => {
    render(
      <CopyBar
        copy={copying({ progress: copyProgress({ totalsKnown: false, filesTotal: 12 }) })}
        onCancel={() => {}}
      />,
    );

    expect(screen.getByText(/Counting files… 12 so far/)).toBeTruthy();
    expect(screen.getByRole("progressbar").getAttribute("aria-valuenow")).toBeNull();
  });

  it("can be cancelled once", async () => {
    const user = userEvent.setup();
    const onCancel = vi.fn();
    const { rerender } = render(<CopyBar copy={copying()} onCancel={onCancel} />);

    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onCancel).toHaveBeenCalledOnce();

    rerender(<CopyBar copy={copying({ cancelling: true })} onCancel={onCancel} />);
    expect((screen.getByRole("button", { name: "Stopping…" }) as HTMLButtonElement).disabled).toBe(
      true,
    );
  });

  it("says it is preparing before the first event", () => {
    render(<CopyBar copy={copying({ progress: null })} onCancel={() => {}} />);
    expect(screen.getByText("Preparing…")).toBeTruthy();
  });
});
