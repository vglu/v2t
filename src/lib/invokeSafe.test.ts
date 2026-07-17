import { beforeEach, describe, expect, it, vi } from "vitest";
import { browserQueueJobFinish } from "./invokeSafe";

const hoisted = vi.hoisted(() => {
  let releaseImport!: () => void;
  const importBarrier = new Promise<void>((resolve) => {
    releaseImport = resolve;
  });
  return {
    importBarrier,
    releaseImport: () => releaseImport(),
    invoke: vi.fn(),
  };
});

vi.mock("@tauri-apps/api/core", async () => {
  await hoisted.importBarrier;
  return { invoke: hoisted.invoke };
});

describe("browserQueueJobFinish", () => {
  beforeEach(() => {
    hoisted.invoke.mockReset();
    hoisted.invoke.mockResolvedValue({
      transcriptPath: "C:\\out\\track.txt",
      summary: "Saved",
    });
  });

  it("stop between deferred import and invoke skips invoke with Job cancelled", async () => {
    let stop = false;
    const finishPromise = browserQueueJobFinish({
      jobId: "job-1",
      tracks: [
        {
          wavPath: "C:\\work\\track.wav",
          transcriptPath: "C:\\out\\track.txt",
          skipTranscribe: false,
        },
      ],
      results: [{ text: "hello", segments: [] }],
      workDir: "C:\\work",
      deleteAudioAfter: true,
      outputDir: "C:\\out",
      exportWebVtt: true,
      shouldStop: () => stop,
    });

    // Import is still blocked; outer QueuePanel gate already passed.
    stop = true;
    hoisted.releaseImport();

    await expect(finishPromise).rejects.toThrow(/^Job cancelled$/);
    expect(hoisted.invoke).not.toHaveBeenCalled();
  });

  it("sends typed timed results and the job-specific export flag", async () => {
    // Ensure import barrier is open for subsequent tests (no-op if already released).
    hoisted.releaseImport();

    const results = [
      {
        text: "hello",
        segments: [{ startMs: 0, endMs: 1_000, text: "hello" }],
      },
    ];

    await browserQueueJobFinish({
      jobId: "job-1",
      tracks: [
        {
          wavPath: "C:\\work\\track.wav",
          transcriptPath: "C:\\out\\track.txt",
          skipTranscribe: false,
        },
      ],
      results,
      workDir: "C:\\work",
      deleteAudioAfter: true,
      outputDir: "C:\\out",
      exportWebVtt: true,
      shouldStop: () => false,
    });

    expect(hoisted.invoke).toHaveBeenCalledWith("browser_queue_job_finish", {
      jobId: "job-1",
      tracks: [
        {
          wavPath: "C:\\work\\track.wav",
          transcriptPath: "C:\\out\\track.txt",
          skipTranscribe: false,
        },
      ],
      results,
      workDir: "C:\\work",
      deleteAudioAfter: true,
      outputDir: "C:\\out",
      exportWebVtt: true,
    });
  });
});
