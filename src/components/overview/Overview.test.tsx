// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, screen, waitFor, within } from "@testing-library/react";
import { commands, type AppUsage } from "../../api";
import { renderBrowsing, stubLayout } from "../../test/app";
import {
  DEFAULT_SORT,
  NO_FILTER,
  SESSION,
  emit,
  node,
  overview,
  resetApi,
  row,
  size,
  view,
} from "../../test/api";

vi.mock("../../api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../api")>()),
  ...(await import("../../test/api")).apiMock,
}));

const api = vi.mocked(commands);

function app(over: Partial<AppUsage> = {}): AppUsage {
  return {
    package: "com.whatsapp",
    label: null,
    installed: true,
    hasIcon: false,
    size: size(5000),
    files: 10,
    node: node(40),
    ...over,
  };
}

const card = (name: RegExp) => screen.getByRole("region", { name });

beforeEach(() => {
  resetApi();
  stubLayout();
});
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe("what's using the space", () => {
  it("opens the Explorer at the root narrowed to a category, from a segment or the legend", async () => {
    const user = await renderBrowsing({
      overview: overview({
        categories: [
          { category: "video", size: size(700) },
          { category: "photo", size: size(300) },
        ],
      }),
    });
    api.getView.mockClear();

    const bar = screen.getByRole("group", { name: "Space by type" });
    await user.click(within(bar).getByRole("button", { name: /^Videos, 700 B/ }));

    expect(screen.getByRole("tab", { name: "Explorer" }).getAttribute("aria-selected")).toBe(
      "true",
    );
    await waitFor(() =>
      expect(api.getView).toHaveBeenLastCalledWith({
        node: node(0),
        by: "disk",
        sort: DEFAULT_SORT,
        filter: {
          ...NO_FILTER,
          categories: ["video"],
        },
      }),
    );
    expect(screen.getByRole("button", { name: "Remove filter Videos" })).toBeTruthy();

    await user.click(screen.getByRole("tab", { name: "Overview" }));
    const legend = within(card(/What’s using the space/)).getByRole("list");
    await user.click(within(legend).getByRole("button", { name: /Photos/ }));
    await waitFor(() =>
      expect(api.getView).toHaveBeenLastCalledWith({
        node: node(0),
        by: "disk",
        sort: DEFAULT_SORT,
        filter: {
          ...NO_FILTER,
          categories: ["photo"],
        },
      }),
    );
  });
});

describe("biggest files", () => {
  it("lists each with its folder, which reveals it in the Explorer, and can be picked", async () => {
    const user = await renderBrowsing({
      overview: overview({
        biggestFiles: [
          row(9, {
            name: "movie.mkv",
            size: size(9000),
            parent: "/sdcard/Movies",
            parentNode: node(5),
          }),
        ],
      }),
    });
    api.getView.mockResolvedValue(view(5, [], { path: "/sdcard/Movies" }));

    await user.click(screen.getByRole("checkbox", { name: "Select movie.mkv" }));
    expect(screen.getByText(/1 selected/)).toBeTruthy();

    await user.click(screen.getByRole("button", { name: /\/sdcard\/Movies/ }));
    await waitFor(() =>
      expect(api.getView).toHaveBeenLastCalledWith({
        node: node(5),
        by: "disk",
        sort: DEFAULT_SORT,
        filter: NO_FILTER,
      }),
    );
    expect(screen.getByRole("tab", { name: "Explorer" }).getAttribute("aria-selected")).toBe(
      "true",
    );
  });
});

describe("space by app", () => {
  it("shows package names until the phone reports labels, then the labels", async () => {
    await renderBrowsing({ overview: overview({ apps: [app()], appsStatus: "pending" }) });
    const apps = card(/Space by app/);
    expect(within(apps).getByText("com.whatsapp")).toBeTruthy();
    expect(within(apps).getByText("W")).toBeTruthy();

    api.overview.mockResolvedValue(
      overview({ apps: [app({ label: "WhatsApp", hasIcon: true })], appsStatus: "ready" }),
    );
    api.appIcon.mockResolvedValue("iVBORw0KGgo=");
    act(() => emit.appsUpdated({ session: SESSION, status: "ready", icons: 1 }));

    expect(await within(apps).findByText("WhatsApp")).toBeTruthy();
    expect(within(apps).getByText("com.whatsapp")).toBeTruthy();
    await waitFor(() => expect(api.appIcon).toHaveBeenCalledWith("com.whatsapp"));
    await waitFor(() => expect(apps.querySelector("img")?.getAttribute("src")).toMatch(/^data:/));
  });

  it("opens the app's folder", async () => {
    const user = await renderBrowsing({
      overview: overview({ apps: [app({ label: "WhatsApp" })] }),
    });
    api.getView.mockResolvedValue(view(40));

    await user.click(within(card(/Space by app/)).getByRole("button", { name: /WhatsApp/ }));

    await waitFor(() =>
      expect(api.getView).toHaveBeenLastCalledWith({
        node: node(40),
        by: "disk",
        sort: DEFAULT_SORT,
        filter: NO_FILTER,
      }),
    );
  });
});

describe("leftovers", () => {
  it("waits for the phone to say which apps are installed", async () => {
    await renderBrowsing({ overview: overview({ appsStatus: "pending" }) });
    expect(within(card(/Leftovers/)).getByText("Looking up apps…")).toBeTruthy();
  });

  it("says when there are none", async () => {
    await renderBrowsing({ overview: overview({ appsStatus: "ready" }) });
    expect(within(card(/Leftovers/)).getByText(/None on this phone\./)).toBeTruthy();
  });

  it("lists them with a total, and picks every folder an app left", async () => {
    const user = await renderBrowsing({
      overview: overview({
        leftovers: {
          total: size(3 * 1024),
          apps: [{ package: "com.gone", size: size(3 * 1024), folders: [node(7), node(8)] }],
        },
      }),
    });
    const leftovers = card(/Leftovers/);
    expect(within(leftovers).getByText(/Usually safe to delete/)).toBeTruthy();
    expect(within(leftovers).getAllByText("3.0 KB").length).toBeGreaterThan(0);

    await user.click(within(leftovers).getByRole("checkbox", { name: /com\.gone/ }));
    await user.click(screen.getByRole("button", { name: "Delete 1 item" }));

    const dialog = screen.getByRole("dialog");
    expect(within(dialog).getByText(/Everything com\.gone left behind \(2 folders\)/)).toBeTruthy();
  });
});

describe("folders that couldn't be read", () => {
  it("lists their full paths, and the header link jumps to them", async () => {
    const user = await renderBrowsing({
      overview: overview({
        unreadable: [{ node: node(30), path: "/sdcard/Android/data/com.android.secret" }],
      }),
      tab: "explorer",
    });

    await user.click(screen.getByRole("button", { name: "1 folder couldn’t be read" }));

    const unreadable = await screen.findByRole("region", { name: /1 folder couldn’t be read/ });
    expect(within(unreadable).getByText("/sdcard/Android/data/com.android.secret")).toBeTruthy();
    expect(within(unreadable).getByText(/private, even over USB debugging/)).toBeTruthy();
    expect(document.activeElement?.id).toBe("unreadable-title");
  });
});

describe("since last scan", () => {
  it("introduces the first scan of a phone", async () => {
    await renderBrowsing();
    expect(within(card(/Since last scan/)).getByText(/first scan of Pixel/)).toBeTruthy();
  });

  it("shows how much it grew", async () => {
    await renderBrowsing({
      overview: overview({
        sinceLastScan: {
          previous: { size: size(1024), files: 2, dirs: 1, at: 1_700_000_000 },
          apparentDelta: 2048,
          diskDelta: 2048,
          filesDelta: 5,
        },
      }),
    });
    expect(within(card(/Since last scan/)).getByText("+2.0 KB")).toBeTruthy();
  });
});
