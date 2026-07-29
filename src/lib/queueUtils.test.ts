import { describe, expect, it } from "vitest";
import {
  fileBasenameNoExt,
  isUnsupportedMediaUrl,
  parseInputLines,
  shortLabel,
} from "./queueUtils";

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

describe("isUnsupportedMediaUrl", () => {
  it("marks tiktok photo posts as unsupported", () => {
    expect(isUnsupportedMediaUrl("https://www.tiktok.com/photo/123")).toBe(
      true,
    );
    expect(
      isUnsupportedMediaUrl("https://m.tiktok.com/photo/abc?lang=en"),
    ).toBe(true);
  });

  it("marks tiktok profile root links without /video/ as unsupported", () => {
    expect(isUnsupportedMediaUrl("https://www.tiktok.com/@user")).toBe(true);
    expect(isUnsupportedMediaUrl("https://tiktok.com/@user/")).toBe(true);
    expect(
      isUnsupportedMediaUrl("https://tiktok.com/@user?lang=en"),
    ).toBe(true);
  });

  it("does not mark supported video URLs as unsupported", () => {
    expect(
      isUnsupportedMediaUrl("https://www.tiktok.com/@user/video/123"),
    ).toBe(false);
    expect(
      isUnsupportedMediaUrl("https://www.youtube.com/watch?v=abc"),
    ).toBe(false);
  });
});
