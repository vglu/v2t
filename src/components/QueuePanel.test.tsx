import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { QueuePanel } from "./QueuePanel";

vi.mock("@tauri-apps/api/webview", () => ({
  getCurrentWebview: () => ({
    onDragDropEvent: async () => () => {},
  }),
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn().mockResolvedValue(null),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn().mockResolvedValue(() => {}),
}));

vi.mock("@tauri-apps/plugin-opener", () => ({
  revealItemInDir: vi.fn().mockResolvedValue(undefined),
  openPath: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("../lib/browserWhisper", () => ({
  transcribeBrowserTracks: vi.fn(),
}));

vi.mock("../lib/invokeSafe", () => ({
  scanMediaFolder: vi.fn().mockResolvedValue(null),
  cancelQueueJob: vi.fn().mockResolvedValue(undefined),
  sessionLogAppendUi: vi.fn().mockResolvedValue(undefined),
  openSessionLog: vi.fn().mockResolvedValue(true),
  browserQueueJobFinish: vi.fn(),
  releaseQueueJobSlot: vi.fn().mockResolvedValue(undefined),
  processQueueItem: vi.fn().mockResolvedValue({
    kind: "done",
    transcriptPath: "/out/transcript.txt",
    summary: "Saved: /out/transcript.txt",
  }),
}));

import { transcribeBrowserTracks } from "../lib/browserWhisper";
import {
  browserQueueJobFinish,
  cancelQueueJob,
  processQueueItem,
  releaseQueueJobSlot,
} from "../lib/invokeSafe";
import { defaultAppSettings } from "../types/settings";
import type { TimedTranscript } from "../types/timedTranscript";

const settingsFixture = {
  ...defaultAppSettings,
  outputDir: "C:\\temp\\v2t-out",
  recursiveFolderScan: false,
};

describe("QueuePanel", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(processQueueItem).mockResolvedValue({
      kind: "done",
      transcriptPath: "/out/transcript.txt",
      summary: "Saved: /out/transcript.txt",
    });
    vi.mocked(browserQueueJobFinish).mockResolvedValue({
      transcriptPath: "/out/transcript.txt",
      summary: "Saved: /out/transcript.txt",
    });
  });

  it("stop queue is disabled while idle", () => {
    render(
      <QueuePanel settings={settingsFixture} readinessComplete={true} />,
    );
    expect(screen.getByTestId("stop-queue")).toBeDisabled();
  });

  it("shows friendly empty-queue hint when readiness is complete", () => {
    render(
      <QueuePanel settings={settingsFixture} readinessComplete={true} />,
    );
    // Class variant is the language-independent signal (text content varies
    // per locale once M3d catalogs ship).
    expect(screen.getByTestId("queue-empty-hint")).toHaveClass("queue-empty-hint-ok");
    expect(screen.getByTestId("queue-empty-hint")).not.toHaveClass(
      "queue-empty-hint-warn",
    );
    expect(screen.getByTestId("queue-empty-triad").children).toHaveLength(3);
  });

  it("shows setup warning when readiness is incomplete", () => {
    render(
      <QueuePanel settings={settingsFixture} readinessComplete={false} />,
    );
    expect(screen.getByTestId("queue-empty-hint")).toHaveClass(
      "queue-empty-hint-warn",
    );
    expect(screen.getByTestId("queue-empty-hint")).not.toHaveClass(
      "queue-empty-hint-ok",
    );
  });

  it("adds URLs and completes process_queue_item", async () => {
    const user = userEvent.setup();
    render(
      <QueuePanel settings={settingsFixture} readinessComplete={true} />,
    );
    await user.type(
      screen.getByTestId("url-input"),
      "https://example.com/video",
    );
    await user.click(screen.getByTestId("add-urls"));
    expect(screen.getAllByTestId("queue-row")).toHaveLength(1);
    await user.click(screen.getByTestId("start-queue"));
    await waitFor(() => {
      const el = document.querySelector('[data-testid^="job-status-"]');
      expect(el?.textContent).toBe("done");
    });
    // After the queue finishes, the panel flips data-queue-running="false".
    // Language-independent.
    await waitFor(() => {
      expect(screen.getByTestId("queue-panel")).toHaveAttribute(
        "data-queue-running",
        "false",
      );
    });
  });

  it("stop cancels jobs not yet started after the first finishes", async () => {
    const user = userEvent.setup();
    let resolveFirst!: (v: {
      kind: "done";
      transcriptPath: string;
      summary: string;
    }) => void;
    const firstP = new Promise<{
      kind: "done";
      transcriptPath: string;
      summary: string;
    }>((r) => {
      resolveFirst = r;
    });
    let callCount = 0;
    vi.mocked(processQueueItem).mockImplementation(() => {
      callCount += 1;
      if (callCount === 1) return firstP;
      return Promise.resolve({
        kind: "done",
        transcriptPath: "/out/2.txt",
        summary: "Saved: /out/2.txt",
      });
    });

    render(
      <QueuePanel settings={settingsFixture} readinessComplete={true} />,
    );
    await user.type(
      screen.getByTestId("url-input"),
      "https://example.com/a\nhttps://example.com/b",
    );
    await user.click(screen.getByTestId("add-urls"));
    expect(screen.getAllByTestId("queue-row")).toHaveLength(2);
    await user.click(screen.getByTestId("start-queue"));
    await waitFor(() => expect(processQueueItem).toHaveBeenCalledTimes(1));
    expect(screen.getByTestId("stop-queue")).not.toBeDisabled();
    await user.click(screen.getByTestId("stop-queue"));
    expect(cancelQueueJob).toHaveBeenCalled();
    resolveFirst({
      kind: "done",
      transcriptPath: "/out/1.txt",
      summary: "Saved: /out/1.txt",
    });
    await waitFor(() => {
      expect(processQueueItem).toHaveBeenCalledTimes(1);
      const statusEls = document.querySelectorAll(
        '[data-testid^="job-status-"]',
      );
      const texts = [...statusEls].map((el) => el.textContent);
      expect(texts).toEqual(["done", "cancelled"]);
    });
    // After the queue finishes, the panel flips data-queue-running="false".
    // Language-independent.
    await waitFor(() => {
      expect(screen.getByTestId("queue-panel")).toHaveAttribute(
        "data-queue-running",
        "false",
      );
    });
  });

  it("finishes with the prepared flag even if current settings differ", async () => {
    const user = userEvent.setup();
    vi.mocked(processQueueItem).mockResolvedValue({
      kind: "browserPrepared",
      tracks: [
        {
          wavPath: "C:\\work\\track.wav",
          transcriptPath: "C:\\temp\\v2t-out\\track.txt",
          skipTranscribe: false,
        },
      ],
      workDir: "C:\\work",
      deleteAudioAfter: true,
      language: "en",
      whisperModelId: "base",
      exportWebVtt: true,
    });
    vi.mocked(transcribeBrowserTracks).mockResolvedValue([
      {
        text: "hello",
        segments: [{ startMs: 0, endMs: 1000, text: "hello" }],
      },
    ]);

    render(
      <QueuePanel
        settings={{ ...settingsFixture, exportWebVtt: false }}
        readinessComplete={true}
      />,
    );
    await user.type(
      screen.getByTestId("url-input"),
      "https://example.com/browser",
    );
    await user.click(screen.getByTestId("add-urls"));
    await user.click(screen.getByTestId("start-queue"));

    await waitFor(() => expect(browserQueueJobFinish).toHaveBeenCalledTimes(1));
    expect(transcribeBrowserTracks).toHaveBeenCalledWith(
      expect.objectContaining({ exportWebVtt: true }),
    );
    expect(browserQueueJobFinish).toHaveBeenCalledWith({
      jobId: expect.any(String),
      tracks: [
        {
          wavPath: "C:\\work\\track.wav",
          transcriptPath: "C:\\temp\\v2t-out\\track.txt",
          skipTranscribe: false,
        },
      ],
      results: [
        {
          text: "hello",
          segments: [{ startMs: 0, endMs: 1000, text: "hello" }],
        },
      ],
      workDir: "C:\\work",
      deleteAudioAfter: true,
      outputDir: "C:\\temp\\v2t-out",
      exportWebVtt: true,
      shouldStop: expect.any(Function),
    });
  });

  it("keeps browser finish text-only when the prepared export flag is false", async () => {
    const user = userEvent.setup();
    vi.mocked(processQueueItem).mockResolvedValue({
      kind: "browserPrepared",
      tracks: [
        {
          wavPath: "C:\\work\\track.wav",
          transcriptPath: "C:\\temp\\v2t-out\\track.txt",
          skipTranscribe: false,
        },
      ],
      workDir: "C:\\work",
      deleteAudioAfter: false,
      language: null,
      whisperModelId: "base",
      exportWebVtt: false,
    });
    vi.mocked(transcribeBrowserTracks).mockResolvedValue([
      { text: "legacy", segments: [] },
    ]);

    render(
      <QueuePanel
        settings={{ ...settingsFixture, exportWebVtt: false }}
        readinessComplete={true}
      />,
    );
    await user.type(
      screen.getByTestId("url-input"),
      "https://example.com/legacy-browser",
    );
    await user.click(screen.getByTestId("add-urls"));
    await user.click(screen.getByTestId("start-queue"));

    await waitFor(() => expect(browserQueueJobFinish).toHaveBeenCalledTimes(1));
    expect(transcribeBrowserTracks).toHaveBeenCalledWith(
      expect.objectContaining({ exportWebVtt: false }),
    );
    expect(browserQueueJobFinish).toHaveBeenCalledWith(
      expect.objectContaining({
        results: [{ text: "legacy", segments: [] }],
        exportWebVtt: false,
      }),
    );
  });

  it("stop after WASM resolve skips finish, releases slot once, UI cancelled", async () => {
    const user = userEvent.setup();
    let resolveWasm!: (value: TimedTranscript[]) => void;
    const wasmDeferred = new Promise<TimedTranscript[]>((resolve) => {
      resolveWasm = resolve;
    });
    vi.mocked(processQueueItem).mockResolvedValue({
      kind: "browserPrepared",
      tracks: [
        {
          wavPath: "C:\\work\\track.wav",
          transcriptPath: "C:\\temp\\v2t-out\\track.txt",
          skipTranscribe: false,
        },
      ],
      workDir: "C:\\work",
      deleteAudioAfter: true,
      language: "en",
      whisperModelId: "base",
      exportWebVtt: true,
    });
    vi.mocked(transcribeBrowserTracks).mockReturnValue(wasmDeferred);

    render(
      <QueuePanel settings={settingsFixture} readinessComplete={true} />,
    );
    await user.type(
      screen.getByTestId("url-input"),
      "https://example.com/stop-before-finish",
    );
    await user.click(screen.getByTestId("add-urls"));
    await user.click(screen.getByTestId("start-queue"));

    await waitFor(() => expect(transcribeBrowserTracks).toHaveBeenCalledTimes(1));
    await user.click(screen.getByTestId("stop-queue"));
    expect(cancelQueueJob).toHaveBeenCalled();

    resolveWasm([
      {
        text: "hello",
        segments: [{ startMs: 0, endMs: 1000, text: "hello" }],
      },
    ]);

    await waitFor(() => {
      const el = document.querySelector('[data-testid^="job-status-"]');
      expect(el?.textContent).toBe("cancelled");
    });
    expect(browserQueueJobFinish).not.toHaveBeenCalled();
    expect(releaseQueueJobSlot).toHaveBeenCalledTimes(1);
    await waitFor(() => {
      expect(screen.getByTestId("queue-panel")).toHaveAttribute(
        "data-queue-running",
        "false",
      );
    });
  });

  it("stop after outer finish gate and before invoke releases once, UI cancelled", async () => {
    const user = userEvent.setup();
    let resolveFinishImport!: () => void;
    const finishImportGate = new Promise<void>((resolve) => {
      resolveFinishImport = resolve;
    });
    let finishInvokeCount = 0;

    vi.mocked(processQueueItem).mockResolvedValue({
      kind: "browserPrepared",
      tracks: [
        {
          wavPath: "C:\\work\\track.wav",
          transcriptPath: "C:\\temp\\v2t-out\\track.txt",
          skipTranscribe: false,
        },
      ],
      workDir: "C:\\work",
      deleteAudioAfter: true,
      language: "en",
      whisperModelId: "base",
      exportWebVtt: true,
    });
    vi.mocked(transcribeBrowserTracks).mockResolvedValue([
      {
        text: "hello",
        segments: [{ startMs: 0, endMs: 1000, text: "hello" }],
      },
    ]);
    vi.mocked(browserQueueJobFinish).mockImplementation(async (args) => {
      await finishImportGate;
      if (args.shouldStop()) {
        throw new Error("Job cancelled");
      }
      finishInvokeCount += 1;
      return {
        transcriptPath: "C:\\temp\\v2t-out\\track.txt",
        summary: "Saved: C:\\temp\\v2t-out\\track.txt",
      };
    });

    render(
      <QueuePanel settings={settingsFixture} readinessComplete={true} />,
    );
    await user.type(
      screen.getByTestId("url-input"),
      "https://example.com/stop-finish-toctou",
    );
    await user.click(screen.getByTestId("add-urls"));
    await user.click(screen.getByTestId("start-queue"));

    await waitFor(() => expect(browserQueueJobFinish).toHaveBeenCalledTimes(1));
    await user.click(screen.getByTestId("stop-queue"));
    expect(cancelQueueJob).toHaveBeenCalled();
    resolveFinishImport();

    await waitFor(() => {
      const el = document.querySelector('[data-testid^="job-status-"]');
      expect(el?.textContent).toBe("cancelled");
    });
    expect(finishInvokeCount).toBe(0);
    expect(releaseQueueJobSlot).toHaveBeenCalledTimes(1);
    await waitFor(() => {
      expect(screen.getByTestId("queue-panel")).toHaveAttribute(
        "data-queue-running",
        "false",
      );
    });
  });
});
