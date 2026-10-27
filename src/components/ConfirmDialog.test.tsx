// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { Row } from "../api";
import { row as makeRow, size } from "../test/api";
import { ConfirmDialog } from "./ConfirmDialog";

afterEach(cleanup);

function row(over: Partial<Row> = {}): Row {
  return makeRow(7, { name: "notes.txt", size: size(2048), ...over });
}

function open(target: Row, path = "/sdcard/Documents/notes.txt") {
  const onCancel = vi.fn();
  const onConfirm = vi.fn();
  render(<ConfirmDialog target={target} path={path} onCancel={onCancel} onConfirm={onConfirm} />);
  return { onCancel, onConfirm };
}

describe("ConfirmDialog", () => {
  it("puts focus on Cancel so Enter cannot delete by reflex", () => {
    open(row());
    expect(screen.getByRole("button", { name: "Cancel" })).toBe(document.activeElement);
  });

  it("shows the full path of what will be deleted", () => {
    open(row(), "/sdcard/Documents/notes.txt");
    expect(screen.getByText("/sdcard/Documents/notes.txt")).toBeTruthy();
  });

  it("keeps focus inside the dialog when tabbing past either end", async () => {
    const user = userEvent.setup();
    open(row());
    const dialog = screen.getByRole("dialog");

    for (let i = 0; i < 4; i++) {
      await user.tab();
      expect(dialog.contains(document.activeElement)).toBe(true);
    }
    for (let i = 0; i < 4; i++) {
      await user.tab({ shift: true });
      expect(dialog.contains(document.activeElement)).toBe(true);
    }
  });

  it("deletes a small, measured file without typing anything", async () => {
    const user = userEvent.setup();
    const { onConfirm } = open(row());

    await user.click(screen.getByRole("button", { name: "Delete" }));
    expect(onConfirm).toHaveBeenCalledOnce();
  });

  it("states what deleting frees as disk usage, whatever size the view shows", () => {
    open(row({ size: { apparent: 1024, disk: 4096 } }));
    expect(screen.getByText("4.0 KB")).toBeTruthy();
    expect(screen.queryByText("1.0 KB")).toBeNull();
  });

  it("requires typing the name for a folder that is not measured yet", async () => {
    const user = userEvent.setup();
    const { onConfirm } = open(
      row({ name: "DCIM", kind: "dir", size: size(0), files: 0, measure: "unknown" }),
    );

    const del = screen.getByRole("button", { name: "Delete" }) as HTMLButtonElement;
    expect(screen.getByText(/real size is unknown/i)).toBeTruthy();
    expect(del.disabled).toBe(true);

    await user.type(screen.getByRole("textbox"), "DCI");
    expect(del.disabled).toBe(true);

    await user.type(screen.getByRole("textbox"), "M");
    expect(del.disabled).toBe(false);

    await user.click(del);
    expect(onConfirm).toHaveBeenCalledOnce();
  });

  it("calls a partial size a lower bound and asks to type the name of a folder", () => {
    open(row({ name: "Movies", kind: "dir", size: size(5000), files: 3, measure: "partial" }));

    expect(screen.getByText(/at least/)).toBeTruthy();
    expect(screen.getByText(/still counting/)).toBeTruthy();
    expect((screen.getByRole("button", { name: "Delete" }) as HTMLButtonElement).disabled).toBe(
      true,
    );
  });

  it("asks for typing above the size threshold even when fully measured", () => {
    open(row({ name: "big.bin", size: size(2 * 1024 ** 3) }));

    expect(screen.getByRole("textbox")).toBeTruthy();
  });
});
