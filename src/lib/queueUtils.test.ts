import { describe, expect, it } from "vitest";
import { fileBasenameNoExt, parseInputLines, shortLabel } from "./queueUtils";

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

describe("fileBasenameNoExt", () => {
  it("strips Windows directories and extension", () => {
    expect(fileBasenameNoExt("C:\\Users\\me\\Videos\\lecture 4.mp4")).toBe(
      "lecture 4",
    );
  });

  it("strips Unix directories and extension", () => {
    expect(fileBasenameNoExt("/home/me/clips/song.final.mp3")).toBe(
      "song.final",
    );
  });

  it("handles plain filename without parent", () => {
    expect(fileBasenameNoExt("clip.webm")).toBe("clip");
  });

  it("returns name unchanged when there is no extension", () => {
    expect(fileBasenameNoExt("/tmp/no-extension")).toBe("no-extension");
  });

  it("preserves leading dot files", () => {
    expect(fileBasenameNoExt(".env")).toBe(".env");
  });
});
