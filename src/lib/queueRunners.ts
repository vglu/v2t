import type { AppSettings } from "../types/settings";
import type {
  AcquireOutcome,
  AcquiredArtifact,
  BrowserTrackInfo,
  ProcessQueueItemResult,
} from "../types/job";
import type { QueueItem, StageStatus } from "../types/queue";
import type { TimedTranscript } from "../types/timedTranscript";
import { transcribeBrowserTracks } from "./browserWhisper";

export type QueueRunnersShouldStop = () => boolean;

export type QueueRunnersInvoke = {
  acquireMediaItem: (args: {
    jobId: string;
    jobIndex: number;
    source: string;
    sourceKind: "url" | "file";
    displayLabel: string;
    settings: AppSettings;
  }) => Promise<AcquireOutcome>;
  transcribeArtifact: (args: {
    jobId: string;
    jobIndex: number;
    source: string;
    sourceKind: "url" | "file";
    artifact: AcquiredArtifact;
    settings: AppSettings;
  }) => Promise<ProcessQueueItemResult>;
  browserQueueJobFinish: (args: {
    jobId: string;
    tracks: BrowserTrackInfo[];
    results: TimedTranscript[];
    workDir: string;
    deleteAudioAfter: boolean;
    outputDir: string;
    exportWebVtt: boolean;
    shouldStop: () => boolean;
  }) => Promise<ProcessQueueItemResult>;
  releaseQueueJobSlot: (jobId: string) => Promise<void>;
};

type Deferred = () => void;

/** In-memory backpressure controller shared by both loops. */
export class PendingArtifactsBackpressure {
  private pendingCount = 0;
  private pendingBytes = 0;
  private reservedCount = 0;
  private pendingBytesByItemId = new Map<string, number>();

  private waiters = new Set<Deferred>();

  constructor(
    private readonly maxPendingArtifacts: number,
    private readonly maxPendingArtifactsMb: number,
  ) {}

  static fromSettings(s: AppSettings): PendingArtifactsBackpressure {
    return new PendingArtifactsBackpressure(
      s.maxPendingArtifacts,
      s.maxPendingArtifactsMb,
    );
  }

  private canStart(): boolean {
    const maxBytes = this.maxPendingArtifactsMb * 1024 * 1024;
    const usedCount = this.pendingCount + this.reservedCount;
    const usedBytes = this.pendingBytes;
    return usedCount < this.maxPendingArtifacts && usedBytes < maxBytes;
  }

  isCapacityAvailable(): boolean {
    return this.canStart();
  }

  reserveAcquireStart(): void {
    this.reservedCount += 1;
    this.trigger();
  }

  releaseAcquireReservation(): void {
    this.reservedCount = Math.max(0, this.reservedCount - 1);
    this.trigger();
  }

  addPendingReady(itemId: string, bytesOnDisk: number): void {
    // reservedCount was already incremented.
    this.reservedCount = Math.max(0, this.reservedCount - 1);
    this.pendingCount += 1;
    this.pendingBytes += bytesOnDisk;
    this.pendingBytesByItemId.set(itemId, bytesOnDisk);
    this.trigger();
  }

  addPendingBrowserPrepared(itemId: string): void {
    this.reservedCount = Math.max(0, this.reservedCount - 1);
    this.pendingCount += 1;
    this.pendingBytesByItemId.set(itemId, 0);
    this.trigger();
  }

  markTranscribeDone(itemId: string): void {
    if (!this.pendingBytesByItemId.has(itemId)) return;
    const bytes = this.pendingBytesByItemId.get(itemId) ?? 0;
    this.pendingBytesByItemId.delete(itemId);
    this.pendingCount = Math.max(0, this.pendingCount - 1);
    this.pendingBytes = Math.max(0, this.pendingBytes - bytes);
    this.trigger();
  }

  async waitForCapacityChange(shouldStop: () => boolean): Promise<void> {
    while (!this.canStart()) {
      if (shouldStop()) return;
      await new Promise<void>((resolve) => this.waiters.add(resolve));
    }
  }

  /** Wait for the next internal state change (acquire started/completed,
   * transcribe done, etc.), without enforcing any capacity predicate. */
  async waitForUpdate(shouldStop: () => boolean): Promise<void> {
    if (shouldStop()) return;
    await new Promise<void>((resolve) => this.waiters.add(resolve));
  }

  private trigger(): void {
    if (this.waiters.size === 0) return;
    const all = [...this.waiters];
    this.waiters.clear();
    for (const w of all) w();
  }

  snapshot(): { pendingCount: number; reservedCount: number; pendingBytes: number } {
    return {
      pendingCount: this.pendingCount,
      reservedCount: this.reservedCount,
      pendingBytes: this.pendingBytes,
    };
  }
}

function isCancelledError(err: unknown): boolean {
  if (!(err instanceof Error)) return false;
  const s = err.message.toLowerCase();
  return s.includes("job cancelled") || s === "cancelled" || s.includes("cancelled");
}

function jobIndexByItem(items: QueueItem[], itemId: string): number {
  const idx = items.findIndex((it) => it.id === itemId);
  return idx >= 0 ? idx + 1 : 1;
}

export async function runAcquireLoop(args: {
  items: QueueItem[];
  settings: AppSettings;
  invoke: QueueRunnersInvoke;
  shouldStop: QueueRunnersShouldStop;
  backpressure: PendingArtifactsBackpressure;
  onUpdate: () => void;
}) {
  const { items, settings, invoke, shouldStop, backpressure, onUpdate } = args;

  type InFlight = { itemId: string; promise: Promise<void> };
  const inFlight = new Set<InFlight>();

  const markAcquireStatus = (item: QueueItem, status: StageStatus, detail?: string) => {
    item.acquire.status = status;
    item.acquire.detail = detail;
    onUpdate();
  };

  while (!shouldStop()) {
    // Start new acquires until we hit concurrency or capacity.
    while (inFlight.size < settings.acquireConcurrency && !shouldStop()) {
      const next = items.find(
        (it) => it.acquire.status === "pending" || it.acquire.status === "waiting",
      );
      if (!next) break;

      if (!backpressure.isCapacityAvailable()) {
        // Backpressure: don't start more downloads; show waiting state.
        markAcquireStatus(next, "waiting", "transcription is behind");
        await backpressure.waitForCapacityChange(shouldStop);
        if (shouldStop()) break;
        // Capacity has freed; resume.
        markAcquireStatus(next, "pending");
        continue;
      }

      // Start acquire for this item.
      markAcquireStatus(next, "running");
      next.acquire.attempts += 1;
      next.acquire.failureKind = undefined;

      const jobId = next.id;
      const jobIndex = jobIndexByItem(items, jobId);

      backpressure.reserveAcquireStart();

      const promise = (async () => {
        try {
          const outcome = await invoke.acquireMediaItem({
            jobId,
            jobIndex,
            source: next.source,
            sourceKind: next.kind === "url" ? "url" : "file",
            displayLabel: next.displayLabel,
            settings: { ...settings, language: next.language },
          });

          if (shouldStop()) {
            markAcquireStatus(next, "cancelled", "Cancelled");
            backpressure.releaseAcquireReservation();
            return;
          }

          next.acquire.status = "done";
          next.acquire.detail = undefined;
          next.acquire.failureKind = undefined;

          switch (outcome.kind) {
            case "ready": {
              next.artifact = {
                workDir: outcome.workDir,
                wavPaths: outcome.wavPaths,
                sourceMediaFiles: outcome.sourceMediaFiles,
                sourceMediaFilesAreAudio: outcome.sourceMediaFilesAreAudio,
                remoteMediaInfo: outcome.remoteMediaInfo ?? null,
                date: outcome.date,
                displayLabel: outcome.displayLabel,
                nTracks: outcome.nTracks,
                bytesOnDisk: outcome.bytesOnDisk,
              };
              next.transcribe.status = "pending";
              next.transcribe.detail = undefined;
              backpressure.addPendingReady(next.id, outcome.bytesOnDisk);
              onUpdate();
              break;
            }
            case "completed": {
              next.transcriptPath = outcome.transcriptPath;
              next.transcribe.status = "done";
              next.transcribe.detail = outcome.summary;
              // Fast-path does not produce pending artifacts.
              backpressure.releaseAcquireReservation();
              onUpdate();
              break;
            }
            case "browserPrepared": {
              next.browserPrepared = {
                tracks: outcome.tracks,
                workDir: outcome.workDir,
                deleteAudioAfter: outcome.deleteAudioAfter,
                language: outcome.language,
                whisperModelId: outcome.whisperModelId,
                exportWebVtt: outcome.exportWebVtt,
              };
              next.transcriptPath = null;
              next.transcribe.status = "pending";
              next.transcribe.detail = undefined;
              backpressure.addPendingBrowserPrepared(next.id);
              onUpdate();
              break;
            }
          }
        } catch (e) {
          if (shouldStop() || isCancelledError(e)) {
            markAcquireStatus(next, "cancelled", "Cancelled");
          } else {
            markAcquireStatus(next, "error", e instanceof Error ? e.message : String(e));
          }
          // Any error releases reserved capacity because it doesn't create a pending artifact.
          backpressure.releaseAcquireReservation();
          onUpdate();
        }
      })();

      inFlight.add({ itemId: jobId, promise });
    }

    if (inFlight.size === 0) break;

    await Promise.race(
      [...inFlight].map((inf) =>
        inf.promise.then(() => inf.itemId).catch(() => inf.itemId),
      ),
    );

    // Remove completed promises (based on stage status).
    for (const inf of [...inFlight]) {
      const it = items.find((x) => x.id === inf.itemId);
      if (!it || it.acquire.status !== "running") {
        inFlight.delete(inf);
      }
    }
  }
}

export async function runTranscribeLoop(args: {
  items: QueueItem[];
  settings: AppSettings;
  invoke: QueueRunnersInvoke;
  shouldStop: QueueRunnersShouldStop;
  backpressure: PendingArtifactsBackpressure;
  onUpdate: () => void;
}) {
  const { items, settings, invoke, shouldStop, backpressure, onUpdate } = args;
  const outDir = settings.outputDir?.trim();
  if (!outDir) throw new Error("Output folder is not set");

  type InFlight = { itemId: string; promise: Promise<void> };
  const inFlight = new Set<InFlight>();

  const markTranscribeStatus = (item: QueueItem, status: StageStatus, detail?: string) => {
    item.transcribe.status = status;
    item.transcribe.detail = detail;
    onUpdate();
  };

  while (!shouldStop()) {
    while (inFlight.size < settings.transcribeConcurrency && !shouldStop()) {
      const next = items.find(
        (it) => it.acquire.status === "done" && it.transcribe.status === "pending",
      );
      if (!next) break;

      markTranscribeStatus(next, "running");
      next.transcribe.attempts += 1;

      const jobId = next.id;
      const jobIndex = jobIndexByItem(items, jobId);

      const promise = (async () => {
        try {
          if (next.browserPrepared) {
            const bp = next.browserPrepared;
            let released = false;
            const releaseOnce = async () => {
              if (released) return;
              released = true;
              await invoke.releaseQueueJobSlot(jobId);
            };

            let results: TimedTranscript[];
            try {
              results = await transcribeBrowserTracks({
                whisperModelId: bp.whisperModelId,
                tracks: bp.tracks,
                language: bp.language,
                exportWebVtt: bp.exportWebVtt,
                shouldStop,
                onProgress: () => {},
              });
            } catch (e) {
              await releaseOnce();
              throw e;
            }

            if (shouldStop()) {
              await releaseOnce();
              markTranscribeStatus(next, "cancelled", "Cancelled");
              onUpdate();
              return;
            }

            try {
              const finishRes = await invoke.browserQueueJobFinish({
                jobId,
                tracks: bp.tracks,
                results,
                workDir: bp.workDir,
                deleteAudioAfter: bp.deleteAudioAfter,
                outputDir: outDir,
                exportWebVtt: bp.exportWebVtt,
                shouldStop,
              });
              next.transcriptPath = finishRes.transcriptPath;
              next.transcribe.status = "done";
              next.transcribe.detail = finishRes.summary;
              backpressure.markTranscribeDone(jobId);
              onUpdate();
            } catch (e) {
              await releaseOnce();
              throw e;
            }
          } else if (next.artifact) {
            const res = await invoke.transcribeArtifact({
              jobId,
              jobIndex,
              source: next.source,
              sourceKind: next.kind === "url" ? "url" : "file",
              artifact: next.artifact,
              settings: { ...settings, language: next.language },
            });

            next.transcriptPath = res.transcriptPath;
            next.transcribe.status = "done";
            next.transcribe.detail = res.summary;
            backpressure.markTranscribeDone(jobId);
            onUpdate();
          } else {
            throw new Error("Missing acquired artifact / browserPrepared payload");
          }
        } catch (e) {
          const msg = e instanceof Error ? e.message : String(e);
          if (isCancelledError(e)) {
            markTranscribeStatus(next, "cancelled", "Cancelled");
          } else {
            markTranscribeStatus(next, "error", msg);
          }
          onUpdate();
        }
      })();

      inFlight.add({ itemId: jobId, promise });
    }

    if (inFlight.size === 0) {
      // No transcribe work yet: wait while acquire is still progressing.
      const acquireStillRunning = items.some(
        (it) =>
          it.acquire.status === "pending" ||
          it.acquire.status === "waiting" ||
          it.acquire.status === "running",
      );
      if (!acquireStillRunning) break;
      await backpressure.waitForUpdate(shouldStop);
      continue;
    }

    await Promise.race(
      [...inFlight].map((inf) =>
        inf.promise.then(() => inf.itemId).catch(() => inf.itemId),
      ),
    );

    for (const inf of [...inFlight]) {
      const it = items.find((x) => x.id === inf.itemId);
      if (!it || it.transcribe.status !== "running") {
        inFlight.delete(inf);
      }
    }
  }
}

