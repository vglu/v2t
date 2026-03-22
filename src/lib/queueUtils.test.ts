import { describe, expect, it } from "vitest";
import { parseInputLines, shortLabel } from "./queueUtils";

describe("parseInputLines", () => {
  it("splits and trims", () => {
    expect(parseInputLines(" a \n\nb\t")).toEqual(["a", "b"]);
  });

  it("returns empty for whitespace", () => {
    expect(parseInputLines("  \n  ")).toEqual([]);
  });
});

describe("shortLabel", () => {
  it("truncates long strings", () => {
    const s = "x".repeat(60);
    expect(shortLabel(s, 10).length).toBeLessThanOrEqual(10);
    expect(shortLabel(s, 10).endsWith("…")).toBe(true);
  });
});
