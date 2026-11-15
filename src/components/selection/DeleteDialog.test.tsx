// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { Row } from "../../api";
import { targetOfRow, type Target } from "../../lib/selection";
import { row, size } from "../../test/api";
import { DeleteDialog } from "./DeleteDialog";

afterEach(cleanup);

const pick = (id: number, over: Partial<Row> = {}) =>
  targetOfRow(row(id, { name: `f${id}.txt`, size: size(2048), ...over }), "/sdcard/Documents");

function open(targets: Target[]) {
  const onCancel = vi.fn();
  const onConfirm = vi.fn();
  render(<DeleteDialog targets={targets} onCancel={onCancel} onConfirm={onConfirm} />);
  return { onCancel, onConfirm };
}

const deleteButton = () => screen.getByRole("button", { name: "Delete" }) as HTMLButtonElement;

describe("DeleteDialog", () => {
  it("puts focus on Cancel so Enter cannot delete by reflex", () => {
    open([pick(1)]);
    expect(screen.getByRole("button", { name: "Cancel" })).toBe(document.activeElement);
  });

  it("lists every full path with its size, and the total freed", () => {
    open([pick(1), pick(2, { size: size(1024) })]);

    expect(screen.getByText("/sdcard/Documents/f1.txt")).toBeTruthy();
    expect(screen.getByText("/sdcard/Documents/f2.txt")).toBeTruthy();
    expect(screen.getByRole("heading", { name: "Delete 2 items?" })).toBeTruthy();
    expect(screen.getByText(/This frees/).textContent).toMatch(/3\.0 KB.*It can’t be undone\./);
  });

  it("keeps focus inside the dialog when tabbing past either end", async () => {
    const user = userEvent.setup();
    open([pick(1)]);

    await user.tab();
    expect(deleteButton()).toBe(document.activeElement);
    await user.tab();
    expect(screen.getByRole("button", { name: "Cancel" })).toBe(document.activeElement);
    await user.tab({ shift: true });
    expect(deleteButton()).toBe(document.activeElement);
  });

  it("deletes a small, measured batch without typing, and cancels on Escape", async () => {
    const user = userEvent.setup();
    const { onConfirm, onCancel } = open([pick(1), pick(2)]);

    expect(screen.queryByText(/Type/)).toBeNull();
    await user.click(deleteButton());
    expect(onConfirm).toHaveBeenCalledOnce();
    await user.keyboard("{Escape}");
    expect(onCancel).toHaveBeenCalledOnce();
  });

  it("states sizes as disk usage, whatever size the view shows", () => {
    open([pick(1, { size: { apparent: 10, disk: 4096 } })]);
    expect(screen.getByText(/This frees/).textContent).toContain("4.0 KB");
  });

  it("wants DELETE typed for a batch still being counted, and says it is a lower bound", async () => {
    const user = userEvent.setup();
    const { onConfirm } = open([pick(1), pick(2, { kind: "dir", measure: "partial" })]);

    expect(screen.getByText(/This frees/).textContent).toMatch(/at least .* \(still counting\)/);
    expect(deleteButton().disabled).toBe(true);
    await user.type(screen.getByRole("textbox"), "delete");
    expect(deleteButton().disabled).toBe(true);
    await user.clear(screen.getByRole("textbox"));
    await user.type(screen.getByRole("textbox"), "DELETE");
    await user.click(deleteButton());
    expect(onConfirm).toHaveBeenCalledOnce();
  });

  it("wants DELETE typed for a big or many-file batch even when fully measured", () => {
    open([pick(1, { size: size(1024 ** 3) })]);
    expect(deleteButton().disabled).toBe(true);
    cleanup();
    open([pick(1, { kind: "dir", files: 150 })]);
    expect(deleteButton().disabled).toBe(true);
    expect(screen.getByText(/150 files/)).toBeTruthy();
  });
});
