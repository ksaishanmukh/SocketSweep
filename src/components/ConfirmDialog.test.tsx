// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ConfirmDialog } from "./ConfirmDialog";
import type { Row } from "../lib/types";

afterEach(cleanup);

function row(over: Partial<Row> = {}): Row {
  return {
    id: 7,
    gen: 1,
    name: "notes.txt",
    size: 2048,
    isDir: false,
    files: 1,
    complete: true,
    listed: true,
    ...over,
  };
}

function open(target: Row) {
  const onCancel = vi.fn();
  const onConfirm = vi.fn();
  render(<ConfirmDialog target={target} dir="/sdcard" onCancel={onCancel} onConfirm={onConfirm} />);
  return { onCancel, onConfirm };
}

describe("ConfirmDialog", () => {
  it("puts focus on Cancel so Enter cannot delete by reflex", () => {
    open(row());
    expect(screen.getByRole("button", { name: "Cancel" })).toBe(document.activeElement);
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

  it("deletes a small, scanned file without typing anything", async () => {
    const user = userEvent.setup();
    const { onConfirm } = open(row());

    await user.click(screen.getByRole("button", { name: "Delete" }));
    expect(onConfirm).toHaveBeenCalledOnce();
  });

  it("requires typing the name for a folder the scanner never read", async () => {
    const user = userEvent.setup();
    const { onConfirm } = open(
      row({ name: "DCIM", isDir: true, size: 0, files: 0, listed: false }),
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
});
