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

vi.mock("../lib/invokeSafe", () => ({
  scanMediaFolder: vi.fn().mockResolvedValue(null),
  cancelQueueJob: vi.fn().mockResolvedValue(undefined),
  processQueueItem: vi.fn().mockResolvedValue({
    transcriptPath: "/out/transcript.txt",
    summary: "Saved: /out/transcript.txt",
  }),
}));

import { cancelQueueJob, processQueueItem } from "../lib/invokeSafe";
import { defaultAppSettings } from "../types/settings";

const settingsFixture = {
  ...defaultAppSettings,
  outputDir: "C:\\temp\\v2t-out",
  recursiveFolderScan: false,
};

describe("QueuePanel", () => {
  beforeEach(() => {
    vi.mocked(processQueueItem).mockClear();
    vi.mocked(processQueueItem).mockResolvedValue({
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
    expect(screen.getByTestId("queue-empty-hint")).toHaveClass("queue-empty-hint-ok");
    expect(screen.getByText(/Ready when you are/i)).toBeInTheDocument();
  });

  it("shows setup warning when readiness is incomplete", () => {
    render(
      <QueuePanel settings={settingsFixture} readinessComplete={false} />,
    );
    expect(screen.getByTestId("queue-empty-hint")).toHaveClass(
      "queue-empty-hint-warn",
    );
    expect(screen.getByText(/Finish setup first/i)).toBeInTheDocument();
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
    expect(screen.getByTestId("queue-log").textContent).toMatch(/Queue idle/);
  });

  it("stop cancels jobs not yet started after the first finishes", async () => {
    const user = userEvent.setup();
    let resolveFirst!: (v: {
      transcriptPath: string;
      summary: string;
    }) => void;
    const firstP = new Promise<{ transcriptPath: string; summary: string }>(
      (r) => {
        resolveFirst = r;
      },
    );
    let callCount = 0;
    vi.mocked(processQueueItem).mockImplementation(() => {
      callCount += 1;
      if (callCount === 1) return firstP;
      return Promise.resolve({
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
    expect(screen.getByTestId("queue-log").textContent).toMatch(/Queue idle/);
  });
});
