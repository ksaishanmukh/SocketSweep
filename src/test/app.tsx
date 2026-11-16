/**
 * Renders the whole app against the `src/api` mock and takes it to the
 * result screen, for tests of what a person sees and clicks.
 *
 * The calling test file must mock `src/api` itself (see `test/api.ts`),
 * because `vi.mock` only applies to the file it is written in.
 */
import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { vi } from "vitest";
import type { Overview, TreemapNode, View } from "../api";
import App from "../App";
import {
  commands,
  connected,
  emit,
  node,
  overview as makeOverview,
  progress,
  started,
  totals,
  treemapNode,
  view as makeView,
} from "./api";

/** jsdom has no layout: give the virtual list a size, and every observed element this one. */
export const MAP_BOX = { width: 800, height: 260 };

export function stubLayout() {
  vi.spyOn(HTMLElement.prototype, "offsetHeight", "get").mockReturnValue(600);
  vi.spyOn(HTMLElement.prototype, "offsetWidth", "get").mockReturnValue(800);
  vi.stubGlobal(
    "ResizeObserver",
    class {
      constructor(private callback: ResizeObserverCallback) {}
      observe(target: Element) {
        this.callback(
          [{ target, contentRect: MAP_BOX } as unknown as ResizeObserverEntry],
          this as unknown as ResizeObserver,
        );
      }
      unobserve() {}
      disconnect() {}
    },
  );
}

export async function renderBrowsing({
  view = makeView(0, [], { path: "/sdcard" }),
  treemap = treemapNode(0),
  overview = makeOverview(),
  tab = "overview",
}: {
  view?: View;
  treemap?: TreemapNode;
  overview?: Overview;
  tab?: "overview" | "explorer";
} = {}) {
  const user = userEvent.setup();
  commands.listDevices.mockResolvedValue([
    { serial: "s", model: "Pixel", state: "ready", usable: true, hint: null },
  ]);
  commands.connect.mockResolvedValue(connected());
  commands.startScan.mockResolvedValue(started());
  commands.getView.mockResolvedValue(view);
  commands.getBreadcrumbs.mockResolvedValue([{ node: node(0), name: "sdcard" }]);
  commands.getTreemap.mockResolvedValue(treemap);
  commands.overview.mockResolvedValue(overview);

  render(<App />);
  await user.click(await screen.findByRole("button", { name: "Scan this phone" }));
  await act(async () => emit.scanProgress(progress({ totals: totals({ files: 3 }) })));
  await screen.findByRole("tab", { name: "Overview" });
  if (tab === "explorer") {
    await user.click(screen.getByRole("tab", { name: "Explorer" }));
    await screen.findByRole("navigation", { name: "Breadcrumb" });
  }
  return user;
}
