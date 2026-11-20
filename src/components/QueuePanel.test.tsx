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
  estimateOcrBatch: vi.fn().mockResolvedValue(null),
  scanMediaFolder: vi.fn().mockResolvedValue(null),
  cancelQueueJob: vi.fn().mockResolvedValue(undefined),
  sessionLogAppendUi: vi.fn().mockResolvedValue(undefined),
  openSessionLog: vi.fn().mockResolvedValue(true),
  releaseQueueJobSlot: vi.fn().mockResolvedValue(undefined),
  browserQueueJobFinish: vi.fn(),
  acquireMediaItem: vi.fn(),
  transcribeArtifact: vi.fn(),
  discardArtifact: vi.fn().mockResolvedValue(undefined),
  processQueueItem: vi.fn(), // unused in the refactored QueuePanel (kept for backwards compat in mocks)
  saveFailedLinks: vi.fn().mockResolvedValue(0),
  runBriefForItem: vi.fn().mockResolvedValue("/tmp/brief.pdf"),
  resolveCastboxChannel: vi.fn(),
}));

import { transcribeBrowserTracks } from "../lib/browserWhisper";
import {
  acquireMediaItem,
  browserQueueJobFinish,
  cancelQueueJob,
  discardArtifact,
  releaseQueueJobSlot,
  transcribeArtifact,
} from "../lib/invokeSafe";

import { defaultAppSettings } from "../types/settings";
import type { TimedTranscript } from "../types/timedTranscript";

const settingsFixture = {
  ...defaultAppSettings,
  outputDir: "C:\\temp\\v2t-out",
  recursiveFolderScan: false,
};

function makeTimedTranscript(text: string): TimedTranscript {
  return {
    text,
    segments: [{ startMs: 0, endMs: 1000, text }],
  };
}

describe("QueuePanel", () => {
  beforeEach(() => {
    vi.clearAllMocks();

    vi.mocked(acquireMediaItem).mockResolvedValue({
      kind: "completed",
      transcriptPath: "/out/transcript.txt",
      summary: "Saved: /out/transcript.txt",
    });

    vi.mocked(transcribeArtifact).mockResolvedValue({
      transcriptPath: "/out/transcript.txt",
      summary: "Saved: /out/transcript.txt",
    } as any);

    vi.mocked(browserQueueJobFinish).mockResolvedValue({
      transcriptPath: "/out/transcript.txt",
      summary: "Saved: /out/transcript.txt",
    });

    vi.mocked(releaseQueueJobSlot).mockResolvedValue(undefined);
    vi.mocked(transcribeBrowserTracks).mockResolvedValue([makeTimedTranscript("hello")]);
    vi.mocked(discardArtifact).mockResolvedValue(undefined);
  });

  it("hides stop buttons while idle", () => {
    render(<QueuePanel settings={settingsFixture} readinessComplete={true} />);
    expect(screen.queryByTestId("stop-all")).not.toBeInTheDocument();
    expect(screen.queryByTestId("stop-downloads")).not.toBeInTheDocument();
    expect(screen.queryByTestId("stop-transcription")).not.toBeInTheDocument();
  });

  it("shows source, shared batch profile, and queue when ready", () => {
    render(<QueuePanel settings={settingsFixture} readinessComplete={true} />);
    expect(screen.getByRole("heading", { name: "Transcribe media" })).toBeVisible();
    expect(screen.getByTestId("batch-profile-bar")).toHaveTextContent("Defaults");
    expect(screen.getByRole("heading", { name: /Current batch/ })).toBeVisible();
  });

  it("toggles VTT / Video / Audio chips and persists settings", async () => {
    const user = userEvent.setup();
    const onPersistSettings = vi.fn().mockResolvedValue(undefined);

    render(
      <QueuePanel
        settings={{
          ...settingsFixture,
          exportWebVtt: false,
          keepDownloadedVideo: false,
          keepDownloadedAudio: false,
          keepMediaInfo: false,
        }}
        readinessComplete={true}
        onPersistSettings={onPersistSettings}
      />,
    );

    expect(screen.getByTestId("output-chip-txt")).toBeDisabled();
    expect(screen.getByTestId("output-chip-txt")).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByTestId("output-chip-vtt")).toHaveAttribute("aria-pressed", "false");

    await user.click(screen.getByTestId("output-chip-vtt"));
    expect(onPersistSettings).toHaveBeenCalledWith(
      expect.objectContaining({ exportWebVtt: true }),
    );

    await user.click(screen.getByTestId("output-chip-video"));
    expect(onPersistSettings).toHaveBeenCalledWith(
      expect.objectContaining({ keepDownloadedVideo: true }),
    );

    await user.click(screen.getByTestId("output-chip-audio"));
    expect(onPersistSettings).toHaveBeenCalledWith(
      expect.objectContaining({ keepDownloadedAudio: true }),
    );

    await user.click(screen.getByTestId("output-chip-info"));
    expect(onPersistSettings).toHaveBeenCalledWith(
      expect.objectContaining({ keepMediaInfo: true }),
    );
  });

  it("keeps start disabled when setup is incomplete", () => {
    render(<QueuePanel settings={settingsFixture} readinessComplete={false} />);
    expect(screen.getByTestId("start-queue")).toBeDisabled();
  });

  it("uses the default language and allows a per-item override", async () => {
    const user = userEvent.setup();

    render(<QueuePanel settings={{ ...settingsFixture, language: "de" }} readinessComplete={true} />);

    expect(screen.getByTestId("batch-language-select")).toHaveValue("de");

    await user.type(screen.getByTestId("url-input"), "https://example.com/video");
    await user.click(screen.getByTestId("add-urls"));

    const itemLanguage = document.querySelector(".queue-language-select") as HTMLSelectElement;
    expect(itemLanguage).toHaveValue("de");

    await user.selectOptions(itemLanguage, "uk");
    await user.click(screen.getByTestId("start-queue"));

    await waitFor(() => {
      expect(acquireMediaItem).toHaveBeenCalled();
      const call = vi.mocked(acquireMediaItem).mock.calls[0]?.[0];
      expect(call).toBeTruthy();
      expect(call?.settings).toEqual(expect.objectContaining({ language: "uk" }));
    });
  });

  it("adds URLs and completes acquire/transcribe stages", async () => {
    const user = userEvent.setup();
    render(<QueuePanel settings={settingsFixture} readinessComplete={true} />);

    await user.type(screen.getByTestId("url-input"), "https://example.com/video");
    await user.click(screen.getByTestId("add-urls"));

    expect(screen.getAllByTestId("queue-row")).toHaveLength(1);
    await user.click(screen.getByTestId("start-queue"));

    await waitFor(() => {
      const el = document.querySelector('[data-testid^="job-status-"]');
      expect(el?.textContent).toBe("done");
    });

    await waitFor(() => {
      expect(screen.getByTestId("queue-panel")).toHaveAttribute(
        "data-queue-running",
        "false",
      );
    });
  });

  it("stop downloads cancels only acquire stage (transcription continues)", async () => {
    const user = userEvent.setup();

    let resolveSecondAcquire!: (v: any) => void;
    const secondAcquireDeferred = new Promise<any>((r) => {
      resolveSecondAcquire = r;
    });

    // Control the first item's WASM completion.
    let resolveWasm!: (v: TimedTranscript[]) => void;
    const wasmControl = new Promise<TimedTranscript[]>((resolve) => {
      resolveWasm = resolve;
    });

    const firstBrowserPrepared = {
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
    };

    vi.mocked(acquireMediaItem)
      .mockResolvedValueOnce(firstBrowserPrepared as any)
      .mockImplementationOnce(() => secondAcquireDeferred);

    vi.mocked(transcribeBrowserTracks).mockReturnValueOnce(wasmControl as any);

    render(<QueuePanel settings={settingsFixture} readinessComplete={true} />);

    await user.type(screen.getByTestId("url-input"), "https://example.com/a\nhttps://example.com/b");
    await user.click(screen.getByTestId("add-urls"));
    expect(screen.getAllByTestId("queue-row")).toHaveLength(2);
    await user.click(screen.getByTestId("start-queue"));

    await waitFor(() => expect(transcribeBrowserTracks).toHaveBeenCalledTimes(1));
    await user.click(screen.getByTestId("stop-downloads"));

    const acquireCancels = vi.mocked(cancelQueueJob).mock.calls
      .map((c: any[]) => c[0])
      .filter((v: unknown) => typeof v === "string" && v.endsWith(":acquire"));
    expect(acquireCancels.length).toBeGreaterThanOrEqual(1);

    const transcribeCancels = vi.mocked(cancelQueueJob).mock.calls
      .map((c: any[]) => c[0])
      .filter((v: unknown) => typeof v === "string" && v.endsWith(":transcribe"));
    expect(transcribeCancels.length).toBe(0);

    // Let the second acquire finish after cancellation.
    resolveSecondAcquire(firstBrowserPrepared);
    // Let the first transcription finish.
    resolveWasm([makeTimedTranscript("hello")]);

    await waitFor(() => {
      expect(browserQueueJobFinish).toHaveBeenCalled();
    });

    await waitFor(() => {
      const statusEls = document.querySelectorAll('[data-testid^="job-status-"]');
      const texts = [...statusEls].map((el) => el.textContent);
      expect(texts).toEqual(expect.arrayContaining(["done", "cancelled"]));
    });
  });

  it("stop transcription cancels only transcribe stage (skips browser finish)", async () => {
    const user = userEvent.setup();

    const browserPrepared = {
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
    };

    let resolveWasm!: (v: TimedTranscript[]) => void;
    const wasmDeferred = new Promise<TimedTranscript[]>((resolve) => {
      resolveWasm = resolve;
    });

    vi.mocked(acquireMediaItem).mockResolvedValue(browserPrepared as any);
    vi.mocked(transcribeBrowserTracks).mockReturnValueOnce(wasmDeferred as any);

    render(<QueuePanel settings={settingsFixture} readinessComplete={true} />);

    await user.type(screen.getByTestId("url-input"), "https://example.com/browser");
    await user.click(screen.getByTestId("add-urls"));
    await user.click(screen.getByTestId("start-queue"));

    await waitFor(() => expect(transcribeBrowserTracks).toHaveBeenCalledTimes(1));
    await user.click(screen.getByTestId("stop-transcription"));

    const transcribeCancels = vi.mocked(cancelQueueJob).mock.calls
      .map((c: any[]) => c[0])
      .filter((v: unknown) => typeof v === "string" && v.endsWith(":transcribe"));
    expect(transcribeCancels.length).toBeGreaterThanOrEqual(1);

    const acquireCancels = vi.mocked(cancelQueueJob).mock.calls
      .map((c: any[]) => c[0])
      .filter((v: unknown) => typeof v === "string" && v.endsWith(":acquire"));
    expect(acquireCancels.length).toBe(0);

    resolveWasm([makeTimedTranscript("hello")]);

    await waitFor(() => {
      const el = document.querySelector('[data-testid^="job-status-"]');
      expect(el?.textContent).toBe("cancelled");
    });

    expect(browserQueueJobFinish).not.toHaveBeenCalled();
    expect(releaseQueueJobSlot).toHaveBeenCalledTimes(1);
  });

  it("applies backpressure: second acquire waits for pending transcription", async () => {
    const user = userEvent.setup();

    const settings = {
      ...settingsFixture,
      maxPendingArtifacts: 1,
      maxPendingArtifactsMb: 4096,
    };

    let resolveWasm!: (v: TimedTranscript[]) => void;
    const wasmDeferred = new Promise<TimedTranscript[]>((resolve) => {
      resolveWasm = resolve;
    });

    const firstBrowserPrepared = {
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
    };

    let acquireCallCount = 0;
    vi.mocked(acquireMediaItem).mockImplementation(() => {
      acquireCallCount += 1;
      if (acquireCallCount === 1) return Promise.resolve(firstBrowserPrepared as any);
      return Promise.resolve(firstBrowserPrepared as any);
    });

    vi.mocked(transcribeBrowserTracks).mockReturnValueOnce(wasmDeferred as any);

    render(<QueuePanel settings={settings} readinessComplete={true} />);

    await user.type(
      screen.getByTestId("url-input"),
      "https://example.com/one\nhttps://example.com/two",
    );
    await user.click(screen.getByTestId("add-urls"));
    await user.click(screen.getByTestId("start-queue"));

    await waitFor(() => expect(transcribeBrowserTracks).toHaveBeenCalledTimes(1));
    expect(acquireCallCount).toBe(1);

    // Let the first transcription finish, freeing backpressure.
    resolveWasm([makeTimedTranscript("hello")]);

    await waitFor(() => {
      expect(browserQueueJobFinish).toHaveBeenCalled();
    });

    await waitFor(() => {
      expect(acquireCallCount).toBe(2);
    });
  });

  it("finishes browserPrepared with prepared exportWebVtt = true even if current settings differ", async () => {
    const user = userEvent.setup();

    vi.mocked(acquireMediaItem).mockResolvedValue({
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
    } as any);

    vi.mocked(transcribeBrowserTracks).mockResolvedValue([
      makeTimedTranscript("hello"),
    ]);

    render(
      <QueuePanel
        settings={{ ...settingsFixture, exportWebVtt: false }}
        readinessComplete={true}
      />,
    );

    await user.type(screen.getByTestId("url-input"), "https://example.com/browser");
    await user.click(screen.getByTestId("add-urls"));
    await user.click(screen.getByTestId("start-queue"));

    await waitFor(() => expect(browserQueueJobFinish).toHaveBeenCalledTimes(1));
    expect(transcribeBrowserTracks).toHaveBeenCalledWith(
      expect.objectContaining({ exportWebVtt: true }),
    );
    expect(browserQueueJobFinish).toHaveBeenCalledWith(
      expect.objectContaining({
        exportWebVtt: true,
      }),
    );
  });

  it("keeps browserPrepared exportWebVtt=false and passes it through", async () => {
    const user = userEvent.setup();

    vi.mocked(acquireMediaItem).mockResolvedValue({
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
    } as any);

    vi.mocked(transcribeBrowserTracks).mockResolvedValue([makeTimedTranscript("legacy")]);

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
        exportWebVtt: false,
      }),
    );
  });

  it("stop transcription after WASM resolve skips finish, releases slot once, UI cancelled", async () => {
    const user = userEvent.setup();

    let resolveWasm!: (value: TimedTranscript[]) => void;
    const wasmDeferred = new Promise<TimedTranscript[]>((resolve) => {
      resolveWasm = resolve;
    });

    vi.mocked(acquireMediaItem).mockResolvedValue({
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
    } as any);

    vi.mocked(transcribeBrowserTracks).mockReturnValueOnce(wasmDeferred as any);

    render(<QueuePanel settings={settingsFixture} readinessComplete={true} />);

    await user.type(screen.getByTestId("url-input"), "https://example.com/stop-before-finish");
    await user.click(screen.getByTestId("add-urls"));
    await user.click(screen.getByTestId("start-queue"));

    await waitFor(() => expect(transcribeBrowserTracks).toHaveBeenCalledTimes(1));
    await user.click(screen.getByTestId("stop-transcription"));
    expect(cancelQueueJob).toHaveBeenCalled();

    resolveWasm([makeTimedTranscript("hello")]);

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

  it("stop transcription during browser finish gate releases slot once, UI cancelled", async () => {
    const user = userEvent.setup();
    let resolveFinishImport!: () => void;
    const finishImportGate = new Promise<void>((resolve) => {
      resolveFinishImport = resolve;
    });

    let finishInvokeCount = 0;

    vi.mocked(acquireMediaItem).mockResolvedValue({
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
    } as any);

    vi.mocked(transcribeBrowserTracks).mockResolvedValue([makeTimedTranscript("hello")]);

    vi.mocked(browserQueueJobFinish).mockImplementation(async (args: any) => {
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

    render(<QueuePanel settings={settingsFixture} readinessComplete={true} />);

    await user.type(
      screen.getByTestId("url-input"),
      "https://example.com/stop-finish-toctou",
    );
    await user.click(screen.getByTestId("add-urls"));
    await user.click(screen.getByTestId("start-queue"));

    await waitFor(() => expect(browserQueueJobFinish).toHaveBeenCalledTimes(1));
    await user.click(screen.getByTestId("stop-transcription"));
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

describe("Brief chip", () => {
  it("Brief chip is disabled when geminiApiKey is empty", async () => {
    const settings = { ...settingsFixture, geminiApiKey: "", generateBrief: false };
    render(
      <QueuePanel
        settings={settings}
        readinessComplete={true}
        onPersistSettings={vi.fn()}
        onOpenOutputSettings={undefined}
      />,
    );
    const briefChip = screen.getByTestId("output-chip-brief");
    expect(briefChip).toBeDisabled();
  });

  it("FDD chip label is used when briefDocMode is fdd", async () => {
    const settings = {
      ...settingsFixture,
      geminiApiKey: "test-key",
      generateBrief: false,
      briefDocMode: "fdd" as const,
    };
    render(
      <QueuePanel
        settings={settings}
        readinessComplete={true}
        onPersistSettings={vi.fn()}
        onOpenOutputSettings={undefined}
      />,
    );
    expect(screen.getByTestId("output-chip-brief")).toHaveTextContent("FDD");
  });

  it("Brief chip is enabled when geminiApiKey is set", async () => {
    const settings = {
      ...settingsFixture,
      geminiApiKey: "test-key",
      generateBrief: false,
      briefDocMode: "brief" as const,
    };
    render(
      <QueuePanel
        settings={settings}
        readinessComplete={true}
        onPersistSettings={vi.fn()}
        onOpenOutputSettings={undefined}
      />,
    );
    const briefChip = screen.getByTestId("output-chip-brief");
    expect(briefChip).not.toBeDisabled();
    expect(briefChip).toHaveTextContent("Brief");
  });

  it("clicking Brief chip when privacy not acknowledged shows privacy warning", async () => {
    const user = userEvent.setup();
    const onPersist = vi.fn();
    const settings = {
      ...settingsFixture,
      geminiApiKey: "test-key",
      generateBrief: false,
      briefProviderPrivacyAcknowledged: false,
    };
    render(
      <QueuePanel
        settings={settings}
        readinessComplete={true}
        onPersistSettings={onPersist}
        onOpenOutputSettings={undefined}
      />,
    );
    await user.click(screen.getByTestId("output-chip-brief"));
    // Privacy warning should appear, but onPersist should NOT have been called yet
    expect(screen.getByRole("alert")).toBeInTheDocument();
    expect(onPersist).not.toHaveBeenCalled();
  });

  it("confirming privacy warning enables Brief and VTT", async () => {
    const user = userEvent.setup();
    const onPersist = vi.fn();
    const settings = {
      ...settingsFixture,
      geminiApiKey: "test-key",
      generateBrief: false,
      exportWebVtt: false,
      briefProviderPrivacyAcknowledged: false,
    };
    render(
      <QueuePanel
        settings={settings}
        readinessComplete={true}
        onPersistSettings={onPersist}
        onOpenOutputSettings={undefined}
      />,
    );
    await user.click(screen.getByTestId("output-chip-brief"));
    // Click confirm button
    const confirmBtn = screen.getByRole("button", { name: /i understand/i });
    await user.click(confirmBtn);
    expect(onPersist).toHaveBeenCalledWith(
      expect.objectContaining({
        generateBrief: true,
        exportWebVtt: true,
        briefProviderPrivacyAcknowledged: true,
      }),
    );
  });
});

