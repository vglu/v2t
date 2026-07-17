import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  extractAsrTranscript,
  transcribeBrowserTracks,
} from "./browserWhisper";

const transcriber = vi.fn();
const pipeline = vi.fn().mockResolvedValue(transcriber);

vi.mock("@xenova/transformers", () => ({
  pipeline,
  env: {
    useFS: true,
    useFSCache: true,
    allowLocalModels: true,
    allowRemoteModels: false,
    backends: {
      onnx: {
        wasm: {
          wasmPaths: "",
          simd: true,
          numThreads: 4,
        },
      },
    },
  },
}));

vi.mock("@tauri-apps/api/core", () => ({
  convertFileSrc: (path: string) => `asset://${path}`,
}));

describe("Browser Whisper timed transcripts", () => {
  beforeEach(() => {
    pipeline.mockClear();
    transcriber.mockReset();
  });

  it("converts factual chunk seconds to rounded integer milliseconds", () => {
    expect(
      extractAsrTranscript(
        {
          text: " Hello there everyone ",
          chunks: [
            {
              text: " Hello there everyone ",
              timestamp: [0, 1.2345],
            },
            {
              text: " and goodbye for now ",
              timestamp: [1.2345, 2.0004],
            },
          ],
        },
        true,
      ),
    ).toEqual({
      text: "Hello there everyone",
      segments: [
        { startMs: 0, endMs: 1235, text: "Hello there everyone" },
        { startMs: 1235, endMs: 2000, text: "and goodbye for now" },
      ],
    });
  });

  it("groups short word-piece chunks into cues with factual start/end only", () => {
    expect(
      extractAsrTranscript(
        {
          text: "Hello world today",
          chunks: [
            { text: "Hello", timestamp: [0, 0.4] },
            { text: "world", timestamp: [0.45, 0.9] },
            { text: "today", timestamp: [0.95, 1.4] },
          ],
        },
        true,
      ),
    ).toEqual({
      text: "Hello world today",
      segments: [{ startMs: 0, endMs: 1400, text: "Hello world today" }],
    });
  });

  it("breaks word-piece groups on large gaps without inventing times", () => {
    expect(
      extractAsrTranscript(
        {
          text: "one two three",
          chunks: [
            { text: "one", timestamp: [0, 0.3] },
            { text: "two", timestamp: [0.35, 0.7] },
            { text: "three", timestamp: [1.5, 1.9] },
          ],
        },
        true,
      ),
    ).toEqual({
      text: "one two three",
      segments: [
        { startMs: 0, endMs: 700, text: "one two" },
        { startMs: 1500, endMs: 1900, text: "three" },
      ],
    });
  });

  it("passes through speaker when present on a chunk", () => {
    expect(
      extractAsrTranscript(
        {
          text: "Hello there everyone",
          chunks: [
            {
              text: "Hello there everyone",
              timestamp: [0, 1.5],
              speaker: " Alice ",
            },
          ],
        },
        true,
      ),
    ).toEqual({
      text: "Hello there everyone",
      segments: [
        {
          startMs: 0,
          endMs: 1500,
          text: "Hello there everyone",
          speaker: "Alice",
        },
      ],
    });
  });

  it.each([
    [{ text: "body" }, "no factual segment timestamps"],
    [{ text: "body", chunks: [] }, "no factual segment timestamps"],
    [
      { text: "body", chunks: [{ text: "body", timestamp: [0, Number.NaN] }] },
      "finite",
    ],
    [
      { text: "body", chunks: [{ text: "body", timestamp: [-1, 1] }] },
      "non-negative",
    ],
    [
      { text: "body", chunks: [{ text: "body", timestamp: [1, 1] }] },
      "greater than start",
    ],
    [
      { text: "body", chunks: [{ text: " ", timestamp: [0, 1] }] },
      "nonempty text",
    ],
    [
      {
        text: "body",
        chunks: [
          { text: "later", timestamp: [2, 3] },
          { text: "earlier", timestamp: [1, 1.5] },
        ],
      },
      "chronological",
    ],
    [
      { text: "body", chunks: [{ text: "body", timestamp: [0, null] }] },
      "open-ended or missing timestamp",
    ],
    [
      { text: "body", chunks: [{ text: "body", timestamp: [null, 1] }] },
      "open-ended or missing timestamp",
    ],
  ])("rejects invalid enabled payload %#", (payload, message) => {
    expect(() => extractAsrTranscript(payload, true)).toThrow(
      new RegExp(`Browser Whisper WebVTT.*${message}`, "i"),
    );
  });

  it("keeps legacy text-only extraction when export is disabled", () => {
    expect(extractAsrTranscript({ text: " legacy " }, false)).toEqual({
      text: "legacy",
      segments: [],
    });
  });

  it("requests chunks in the same enabled model call that returns text", async () => {
    transcriber.mockResolvedValue({
      text: "hello",
      chunks: [{ text: "hello", timestamp: [0, 1] }],
    });

    const result = await transcribeBrowserTracks({
      whisperModelId: "base",
      tracks: [
        {
          wavPath: "C:\\work\\track.wav",
          transcriptPath: "C:\\out\\track.txt",
          skipTranscribe: false,
        },
      ],
      language: null,
      exportWebVtt: true,
      shouldStop: () => false,
    });

    expect(transcriber).toHaveBeenCalledTimes(1);
    expect(transcriber).toHaveBeenCalledWith(
      "asset://C:\\work\\track.wav",
      expect.objectContaining({ return_timestamps: true }),
    );
    expect(result[0]).toEqual({
      text: "hello",
      segments: [{ startMs: 0, endMs: 1000, text: "hello" }],
    });
  });

  it("does not request or require chunks when export is disabled", async () => {
    transcriber.mockResolvedValue({ text: "legacy" });

    const result = await transcribeBrowserTracks({
      whisperModelId: "base",
      tracks: [
        {
          wavPath: "C:\\work\\track.wav",
          transcriptPath: "C:\\out\\track.txt",
          skipTranscribe: false,
        },
      ],
      language: null,
      exportWebVtt: false,
      shouldStop: () => false,
    });

    expect(transcriber).toHaveBeenCalledWith(
      "asset://C:\\work\\track.wav",
      expect.not.objectContaining({ return_timestamps: expect.anything() }),
    );
    expect(result[0]).toEqual({ text: "legacy", segments: [] });
  });

  it("throws Job cancelled after dynamic import when shouldStop is true", async () => {
    await expect(
      transcribeBrowserTracks({
        whisperModelId: "base",
        tracks: [
          {
            wavPath: "C:\\work\\track.wav",
            transcriptPath: "C:\\out\\track.txt",
            skipTranscribe: false,
          },
        ],
        language: null,
        exportWebVtt: false,
        shouldStop: () => true,
      }),
    ).rejects.toThrow(/^Job cancelled$/);
    expect(pipeline).not.toHaveBeenCalled();
  });

  it("throws Job cancelled after pipeline load when shouldStop flips true", async () => {
    let stop = false;
    pipeline.mockImplementation(async () => {
      stop = true;
      return transcriber;
    });

    await expect(
      transcribeBrowserTracks({
        whisperModelId: "base",
        tracks: [
          {
            wavPath: "C:\\work\\track.wav",
            transcriptPath: "C:\\out\\track.txt",
            skipTranscribe: false,
          },
        ],
        language: null,
        exportWebVtt: false,
        shouldStop: () => stop,
      }),
    ).rejects.toThrow(/^Job cancelled$/);
    expect(transcriber).not.toHaveBeenCalled();
  });

  it("checks shouldStop before each transcriber call", async () => {
    let stop = false;
    transcriber.mockResolvedValue({
      text: "one",
      chunks: [{ text: "one", timestamp: [0, 1] }],
    });

    await expect(
      transcribeBrowserTracks({
        whisperModelId: "base",
        tracks: [
          {
            wavPath: "C:\\work\\a.wav",
            transcriptPath: "C:\\out\\a.txt",
            skipTranscribe: false,
          },
          {
            wavPath: "C:\\work\\b.wav",
            transcriptPath: "C:\\out\\b.txt",
            skipTranscribe: false,
          },
        ],
        language: null,
        exportWebVtt: true,
        shouldStop: () => stop,
        onProgress: (message) => {
          if (/track 2\//i.test(message)) {
            stop = true;
          }
        },
      }),
    ).rejects.toThrow(/Job cancelled/);
    expect(transcriber).toHaveBeenCalledTimes(1);
  });

  it("stop during inflight transcriber does not push result after resolve", async () => {
    let stop = false;
    let resolveAsr!: (value: unknown) => void;
    const asrDeferred = new Promise((resolve) => {
      resolveAsr = resolve;
    });
    transcriber.mockReturnValue(asrDeferred);

    const run = transcribeBrowserTracks({
      whisperModelId: "base",
      tracks: [
        {
          wavPath: "C:\\work\\track.wav",
          transcriptPath: "C:\\out\\track.txt",
          skipTranscribe: false,
        },
      ],
      language: null,
      exportWebVtt: true,
      shouldStop: () => stop,
    });

    await waitForTranscriberCalled();
    stop = true;
    resolveAsr({
      text: "late",
      chunks: [{ text: "late", timestamp: [0, 1] }],
    });

    await expect(run).rejects.toThrow(/Job cancelled/);
  });
});

async function waitForTranscriberCalled(): Promise<void> {
  for (let i = 0; i < 50; i++) {
    if (transcriber.mock.calls.length > 0) return;
    await Promise.resolve();
  }
  throw new Error("transcriber was not called");
}
