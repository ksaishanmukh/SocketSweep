// @vitest-environment jsdom
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  CHECK_INTERVAL_MS,
  checkForUpdate,
  dismiss,
  isNewer,
  newerRelease,
  parseVersion,
} from "./update";

const PAGE = "https://github.com/ksaishanmukh/SocketSweep/releases/tag/v2.1.0";
const body = (extra = {}) => ({ tag_name: "v2.1.0", html_url: PAGE, ...extra });
const answer = (data: unknown, ok = true) =>
  vi.fn().mockResolvedValue({ ok, json: async () => data }) as unknown as typeof fetch;

beforeEach(() => window.localStorage.clear());

describe("versions", () => {
  it("parses plain versions only", () => {
    expect(parseVersion("v1.2.3")).toEqual([1, 2, 3]);
    expect(parseVersion("1.2.3-rc.1")).toBeNull();
    expect(parseVersion("1.2")).toBeNull();
  });

  it("compares numerically, not as text", () => {
    expect(isNewer("1.10.0", "1.9.0")).toBe(true);
    expect(isNewer("2.0.0", "1.99.99")).toBe(true);
    expect(isNewer("1.0.0", "1.0.0")).toBe(false);
    expect(isNewer("1.0.0", "1.0.1")).toBe(false);
    expect(isNewer("junk", "1.0.0")).toBe(false);
  });
});

describe("newerRelease", () => {
  it("strips the v and keeps the page", () => {
    expect(newerRelease(body(), "2.0.0")).toEqual({ version: "2.1.0", url: PAGE });
  });

  it.each([
    ["older", body(), "2.1.0"],
    ["prerelease", body({ prerelease: true }), "2.0.0"],
    ["draft", body({ draft: true }), "2.0.0"],
    ["foreign link", body({ html_url: "https://evil.example/x" }), "2.0.0"],
    ["no tag", { html_url: PAGE }, "2.0.0"],
    ["not an object", "x", "2.0.0"],
  ])("ignores %s", (_name, data, current) => {
    expect(newerRelease(data, current)).toBeNull();
  });
});

describe("checkForUpdate", () => {
  it("asks GitHub and returns a newer release", async () => {
    const fetcher = answer(body());
    expect(await checkForUpdate("2.0.0", 1000, fetcher)).toEqual({ version: "2.1.0", url: PAGE });
    expect(fetcher).toHaveBeenCalledWith(expect.stringContaining("api.github.com"), {
      headers: { Accept: "application/vnd.github+json" },
    });
  });

  it("asks at most once a day", async () => {
    const fetcher = answer(body());
    await checkForUpdate("2.0.0", 1000, fetcher);
    expect(await checkForUpdate("2.0.0", 2000, fetcher)).toBeNull();
    expect(fetcher).toHaveBeenCalledTimes(1);
    await checkForUpdate("2.0.0", 1000 + CHECK_INTERVAL_MS, fetcher);
    expect(fetcher).toHaveBeenCalledTimes(2);
  });

  it("stays quiet about a dismissed version but not a newer one", async () => {
    dismiss("2.1.0");
    expect(await checkForUpdate("2.0.0", 1000, answer(body()))).toBeNull();
    const newer = body({ tag_name: "v2.2.0" });
    expect(await checkForUpdate("2.0.0", 1000 + CHECK_INTERVAL_MS, answer(newer))).toMatchObject({
      version: "2.2.0",
    });
  });

  it("fails silently on errors, rate limits and bad bodies", async () => {
    const offline = vi.fn().mockRejectedValue(new TypeError("offline")) as unknown as typeof fetch;
    expect(await checkForUpdate("2.0.0", 1, offline)).toBeNull();
    expect(await checkForUpdate("2.0.0", 1 + CHECK_INTERVAL_MS, answer({}, false))).toBeNull();
    const broken = {
      ok: true,
      json: async () => {
        throw new Error("bad json");
      },
    };
    const fetcher = vi.fn().mockResolvedValue(broken) as unknown as typeof fetch;
    expect(await checkForUpdate("2.0.0", 1 + 2 * CHECK_INTERVAL_MS, fetcher)).toBeNull();
  });
});
