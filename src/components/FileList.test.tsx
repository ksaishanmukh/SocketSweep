// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";
import { FileList } from "./FileList";
import type { Row } from "../lib/types";

afterEach(cleanup);

describe("FileList", () => {
  it("shows an unread folder's size as unknown, not 0 B", () => {
    // jsdom has no layout, so the virtualizer needs a size to mount any row.
    vi.spyOn(HTMLElement.prototype, "offsetHeight", "get").mockReturnValue(500);
    vi.spyOn(HTMLElement.prototype, "offsetWidth", "get").mockReturnValue(500);
    const unread: Row = {
      id: 1,
      gen: 1,
      name: "deep",
      size: 0,
      isDir: true,
      files: 0,
      complete: true,
      listed: false,
    };
    render(<FileList rows={[unread]} totalSize={100} onOpen={() => {}} onDelete={() => {}} />);
    expect(screen.getByTitle("Not scanned").textContent).toBe("—");
    expect(screen.queryByText("0 B")).toBeNull();
  });
});
