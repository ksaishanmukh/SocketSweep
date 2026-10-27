// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { Row } from "../api";
import { row as makeRow, size } from "../test/api";
import { FileList } from "./FileList";

afterEach(cleanup);

beforeEach(() => {
  // jsdom has no layout, so the virtualizer needs a size to mount any row.
  vi.spyOn(HTMLElement.prototype, "offsetHeight", "get").mockReturnValue(500);
  vi.spyOn(HTMLElement.prototype, "offsetWidth", "get").mockReturnValue(500);
});

function list(rows: Row[], props: Partial<Parameters<typeof FileList>[0]> = {}) {
  const onOpen = vi.fn();
  const onDelete = vi.fn();
  render(
    <FileList
      rows={rows}
      totalSize={100}
      metric="disk"
      onOpen={onOpen}
      onDelete={onDelete}
      {...props}
    />,
  );
  return { onOpen, onDelete };
}

describe("FileList measure states", () => {
  it("shows an unmeasured folder as measuring, never as 0 B, and draws no size bar", () => {
    list([makeRow(1, { name: "deep", kind: "dir", size: size(0), measure: "unknown" })]);

    expect(screen.getByText("measuring…")).toBeTruthy();
    expect(screen.queryByText("0 B")).toBeNull();
    expect(document.querySelector(".size-bar-inner")).toBeNull();
  });

  it("shows a partial size as a number with a still-counting marker", () => {
    list([makeRow(1, { name: "DCIM", kind: "dir", size: size(2048), measure: "partial" })]);

    const cell = screen.getByTitle(/still counting/i);
    expect(cell.textContent).toContain("2.0 KB");
    expect(cell.textContent).toContain("still counting");
    expect(cell.getAttribute("data-measure")).toBe("partial");
  });

  it("shows a final size as the plain number", () => {
    list([makeRow(1, { name: "a.bin", size: size(2048), measure: "final" })]);

    expect(screen.getByText("2.0 KB")).toBeTruthy();
    expect(screen.queryByTitle(/still counting/i)).toBeNull();
    expect(screen.queryByText("measuring…")).toBeNull();
  });

  it("draws a size bar for measured rows only", () => {
    list(
      [
        makeRow(1, { name: "known", kind: "dir", size: size(50), measure: "partial" }),
        makeRow(2, { name: "unknown", kind: "dir", size: size(0), measure: "unknown" }),
      ],
      { showBar: true },
    );

    expect(document.querySelectorAll(".size-bar-inner")).toHaveLength(1);
  });

  it("shows the size the metric asks for", () => {
    const r = makeRow(1, { size: { apparent: 1024, disk: 4096 } });
    list([r], { metric: "apparent" });

    expect(screen.getByText("1.0 KB")).toBeTruthy();
    expect(screen.queryByText("4.0 KB")).toBeNull();
  });

  it("flags a folder that could not be fully read", () => {
    list([makeRow(1, { kind: "dir", unreadable: true })]);

    expect(screen.getByLabelText("Some folders could not be read")).toBeTruthy();
  });
});

describe("FileList actions", () => {
  it("offers Copy to computer on files and folders, next to Delete", async () => {
    const user = userEvent.setup();
    const file = makeRow(1, { name: "a.mp4" });
    const folder = makeRow(2, { name: "DCIM", kind: "dir" });
    const onCopy = vi.fn();
    list([file, folder], { onCopy });

    await user.click(screen.getByRole("button", { name: "Copy a.mp4 to computer" }));
    await user.click(screen.getByRole("button", { name: "Copy DCIM to computer" }));

    expect(onCopy).toHaveBeenNthCalledWith(1, file);
    expect(onCopy).toHaveBeenNthCalledWith(2, folder);
    expect(screen.getAllByRole("button", { name: /^Delete / })).toHaveLength(2);
  });

  it("names the destination in the Copy label when the host reported one", () => {
    list([makeRow(1, { name: "a.mp4" })], { onCopy: vi.fn(), copyTarget: "Desktop" });

    expect(screen.getByRole("button", { name: "Copy a.mp4 to Desktop" })).toBeTruthy();
  });

  it("disables Copy while another copy runs", () => {
    list([makeRow(1, { name: "a.mp4" })], { onCopy: vi.fn(), copyDisabled: true });

    const button = screen.getByRole("button", { name: "Copy a.mp4 to computer" });
    expect((button as HTMLButtonElement).disabled).toBe(true);
  });

  it("hands the whole row, with its NodeRef, to Delete", async () => {
    const user = userEvent.setup();
    const target = makeRow(5, { name: "old.zip" });
    const { onDelete } = list([target]);

    await user.click(screen.getByRole("button", { name: /^Delete old\.zip/ }));

    expect(onDelete).toHaveBeenCalledWith(target);
    expect(onDelete.mock.calls[0][0].node).toEqual({ session: 1, id: 5 });
  });

  it("describes a partial size as a lower bound to assistive technology", () => {
    list([makeRow(1, { name: "big", kind: "dir", size: size(2048), measure: "partial" })]);

    expect(screen.getByRole("button", { name: /Delete big, at least 2\.0 KB/ })).toBeTruthy();
  });
});
