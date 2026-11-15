import { describe, expect, it, vi } from "vitest";
import { deferred } from "../test/api";
import { createIconStore } from "./iconStore";
import { appHue, appLetter, appName } from "./appIdentity";

describe("icon store", () => {
  it("fetches each package once and serves it as a data URL", async () => {
    const fetch = vi.fn().mockResolvedValue("AAAA");
    const store = createIconStore(fetch);
    store.want("com.a");
    store.want("com.a");
    await vi.waitFor(() => expect(store.get("com.a")).toBe("data:image/png;base64,AAAA"));
    expect(fetch).toHaveBeenCalledOnce();
  });

  it("asks again for a miss only after retryMisses", async () => {
    const fetch = vi.fn().mockResolvedValueOnce(null).mockResolvedValueOnce("BBBB");
    const store = createIconStore(fetch);
    store.want("com.a");
    await vi.waitFor(() => expect(fetch).toHaveBeenCalledTimes(1));
    await Promise.resolve();
    store.want("com.a");
    expect(fetch).toHaveBeenCalledTimes(1);
    store.retryMisses();
    store.want("com.a");
    await vi.waitFor(() => expect(store.get("com.a")).not.toBeNull());
  });

  it("drops an answer that arrives after clear", async () => {
    const answer = deferred<string | null>();
    const store = createIconStore(() => answer.promise);
    store.want("com.a");
    store.clear();
    answer.resolve("CCCC");
    await answer.promise;
    expect(store.get("com.a")).toBeNull();
  });
});

describe("app identity", () => {
  it("names an app by its label, else by its package, never by a guess", () => {
    expect(appName({ package: "com.whatsapp", label: "WhatsApp", hasIcon: false })).toBe(
      "WhatsApp",
    );
    expect(appName({ package: "com.whatsapp", label: null, hasIcon: false })).toBe("com.whatsapp");
  });

  it("letters a tile from the label or the package's last segment, in a stable colour", () => {
    expect(appLetter({ package: "com.whatsapp", label: null, hasIcon: false })).toBe("W");
    expect(appLetter({ package: "com.x", label: "élan", hasIcon: false })).toBe("É");
    expect(appHue("com.a")).toBe(appHue("com.a"));
    expect(appHue("com.a")).not.toBe(appHue("com.b"));
  });
});
