import { describe, it, expect } from "vitest";
import { formatBytes, formatNumber } from "./format";

describe("formatBytes", () => {
  it("formats whole byte counts without a decimal", () => {
    expect(formatBytes(0)).toBe("0 B");
    expect(formatBytes(1)).toBe("1 B");
    expect(formatBytes(512)).toBe("512 B");
    expect(formatBytes(1023)).toBe("1023 B");
  });

  it("steps up through the units at 1024 boundaries", () => {
    expect(formatBytes(1024)).toBe("1.0 KB");
    expect(formatBytes(1536)).toBe("1.5 KB");
    expect(formatBytes(1024 ** 2)).toBe("1.0 MB");
    expect(formatBytes(1024 ** 3)).toBe("1.0 GB");
    expect(formatBytes(1024 ** 4)).toBe("1.0 TB");
  });

  it("matches the sizes shown in the app's own screenshot", () => {
    expect(formatBytes(42_524_697_395)).toBe("39.6 GB");
    expect(formatBytes(983_355_392)).toBe("937.8 MB");
  });

  // Regressions — each of these rendered "undefined" or "NaN" before.
  it("clamps at the largest known unit instead of running off the table", () => {
    expect(formatBytes(1024 ** 5)).toBe("1.0 PB");
    expect(formatBytes(1024 ** 6)).toBe("1024.0 PB");
  });

  it("handles negative sizes", () => {
    expect(formatBytes(-1024)).toBe("-1.0 KB");
  });

  it("handles fractional sub-byte values", () => {
    expect(formatBytes(0.5)).toBe("0 B");
  });

  it("handles non-finite input", () => {
    expect(formatBytes(NaN)).toBe("—");
    expect(formatBytes(Infinity)).toBe("—");
  });
});

describe("formatNumber", () => {
  it("groups thousands", () => {
    expect(formatNumber(56_642)).toBe("56,642");
    expect(formatNumber(0)).toBe("0");
  });

  it("handles non-finite input", () => {
    expect(formatNumber(NaN)).toBe("—");
  });
});
