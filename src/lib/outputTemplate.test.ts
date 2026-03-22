import { describe, expect, it } from "vitest";
import {
  formatOutputFilename,
  sanitizeFilenameSegment,
} from "./outputTemplate";

describe("sanitizeFilenameSegment", () => {
  it("strips illegal characters", () => {
    expect(sanitizeFilenameSegment('a<b>c:d"e')).toBe("a_b_c_d_e");
  });

  it("uses untitled for empty result", () => {
    expect(sanitizeFilenameSegment("   ")).toBe("untitled");
  });
});

describe("formatOutputFilename", () => {
  it("replaces placeholders", () => {
    const out = formatOutputFilename(
      "{title}_{date}_{index}_t{track}_{source}.txt",
      {
        title: "My / Talk",
        date: "2025-01-01",
        index: 3,
        track: 2,
        source: "youtube",
      },
    );
    expect(out).toBe("My _ Talk_2025-01-01_3_t2_youtube.txt");
  });
});
