import { describe, expect, it, vi } from "vitest";

// The real bindings call into Tauri only when a command runs, but there is no
// reason to load it to test the error helpers.
vi.mock("../bindings", () => ({ commands: {}, events: {} }));

import { errorCode, errorMessage, isCommandError } from "./index";

describe("errorMessage", () => {
  it("uses the already-worded message of a CommandError", () => {
    expect(errorMessage({ code: "stale", message: "Results are out of date." })).toBe(
      "Results are out of date.",
    );
  });

  it("still copes with a bare string, an Error and anything else", () => {
    expect(errorMessage("boom")).toBe("boom");
    expect(errorMessage(new Error("bang"))).toBe("bang");
    expect(errorMessage(42)).toBe("42");
    expect(errorMessage(null)).toBe("null");
  });
});

describe("errorCode", () => {
  it("reads the contract's code", () => {
    expect(errorCode({ code: "cancelled", message: "stopped" })).toBe("cancelled");
  });

  it("is null for anything that is not a CommandError", () => {
    expect(errorCode("cancelled")).toBeNull();
    expect(errorCode({ message: "no code" })).toBeNull();
    expect(isCommandError(undefined)).toBe(false);
  });
});
