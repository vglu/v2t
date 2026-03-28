import { describe, expect, it } from "vitest";
import {
  formatOutputFilename,
  sanitizeFilenameSegment,
  videoFilenameFromTranscriptTemplate,
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

  it("replaces {ext}", () => {
    expect(
      formatOutputFilename("{title}_{date}.{ext}", {
        title: "Clip",
        date: "2026-03-22",
        index: 1,
        track: 1,
        source: "url",
        ext: "txt",
      }),
    ).toBe("Clip_2026-03-22.txt");
    expect(
      formatOutputFilename("{title}_{date}.{ext}", {
        title: "Clip",
        date: "2026-03-22",
        index: 1,
        track: 1,
        source: "url",
        ext: "mp4",
      }),
    ).toBe("Clip_2026-03-22.mp4");
  });

  it("derives mp4 from legacy .txt template", () => {
    const v = videoFilenameFromTranscriptTemplate("{title}_{date}.txt", {
      title: "X",
      date: "2026-01-01",
      index: 1,
      track: 1,
      source: "s",
    });
    expect(v).toBe("X_2026-01-01.mp4");
  });
});
