export type QueueJobKind = "file" | "url" | "folder";

/** Snapshot of the latest `queue-job-progress` event from the backend. */
export type JobProgressSnapshot = {
  phase: string;
  message: string;
  subtaskIndex?: number;
  subtaskTotal?: number;
  /** 0..=100, in 5%-buckets when coming from yt-dlp. */
  subtaskPercent?: number;
};

export type StageStatus =
  | "pending"
  | "waiting"
  | "running"
  | "done"
  | "error"
  | "unsupported"
  | "cancelled";

/** Same labels as Rust `DownloadFailureKind::label()`. */
export type DownloadFailureKind =
  | "rateLimited"
  | "serviceUnavailable"
  | "forbidden"
  | "unsupported"
  | "unavailable"
  | "extractorStale"
  | "cookieStore"
  | "stalled"
  | "other";

export type StageState = {
  status: StageStatus;
  /** Human-readable status detail (error text, cancellation reason, etc.). */
  detail?: string;
  /** Number of times this stage was started by UI (initial + retries). */
  attempts: number;
  /** Epoch ms; for status="waiting" — when can be retried again. */
  nextAttemptAt?: number;
  /** Classification of the latest acquire-stage error (for retry UX). */
  failureKind?: DownloadFailureKind;
};

/** One video inside a YouTube playlist, after pre-resolve. */
export type SubtaskStatus = "pending" | "running" | "done" | "skipped" | "error";

export type SubtaskState = {
  id: string;
  /** 1-based playlist index (matches `subtask_index` in `subtask-status` events). */
  index: number;
  title: string;
  originalUrl: string;
  status: SubtaskStatus;
  /** Reason for `skipped` (e.g. "already done") or `error` (the failure message). */
  reason?: string;
};

export type AcquiredArtifact = {
  workDir: string;
  wavPaths: string[];
  sourceMediaFiles: string[];
  sourceMediaFilesAreAudio: boolean;
  remoteMediaInfo?: unknown | null;
  date: string;
  displayLabel: string;
  nTracks: number;
  bytesOnDisk: number;
};

export type BrowserPreparedPayload = {
  tracks: Array<{
    wavPath: string;
    transcriptPath: string;
    skipTranscribe: boolean;
  }>;
  workDir: string;
  deleteAudioAfter: boolean;
  language: string | null;
  whisperModelId: string;
  exportWebVtt: boolean;
};

export type QueueItem = {
  id: string;
  kind: QueueJobKind;
  /** Path or URL — passed to backend pipeline later */
  source: string;
  displayLabel: string;
  /** Spoken-language override for this item. null = automatic detection. */
  language: string | null;
  acquire: StageState;
  transcribe: StageState;
  artifact?: AcquiredArtifact;
  /** Filled when transcribe stage is `done` — path to primary .txt transcript */
  transcriptPath?: string | null;
  /** When acquire stage ends with BrowserWhisper mode, we keep WASM input here. */
  browserPrepared?: BrowserPreparedPayload;
  /** Resolved playlist title from `playlist-resolved` (URL jobs only). */
  playlistTitle?: string;
  /** Resolved playlist entries; populated by `playlist-resolved`, then mutated by `subtask-status` events. */
  subtasks?: SubtaskState[];
};

/** Derived queue-level status for backward-compatible table rendering. */
export type QueueItemStatus =
  | "pending"
  | "waiting"
  | "running"
  | "done"
  | "error"
  | "unsupported"
  | "cancelled";

export function itemStatus(item: QueueItem): QueueItemStatus {
  if (
    item.acquire.status === "error" ||
    item.transcribe.status === "error"
  ) {
    return "error";
  }
  if (item.acquire.status === "unsupported") return "unsupported";
  if (item.transcribe.status === "done") return "done";
  if (item.acquire.status === "cancelled") return "cancelled";
  if (item.transcribe.status === "cancelled") return "cancelled";
  if (item.acquire.status === "waiting") return "waiting";
  // Downloaded, waiting for a transcription slot (or queue start).
  if (item.acquire.status === "done" && item.transcribe.status === "pending") {
    return "waiting";
  }
  if (item.acquire.status === "running" || item.transcribe.status === "running")
    return "running";
  if (item.acquire.status === "pending" || item.transcribe.status === "pending")
    return "pending";
  // Fallback: any unexpected value.
  return "pending";
}
