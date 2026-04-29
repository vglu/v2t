export type QueueJobStatus =
  | "pending"
  | "running"
  | "done"
  | "error"
  | "cancelled";

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

export type QueueJob = {
  id: string;
  kind: QueueJobKind;
  /** Path or URL — passed to backend pipeline later */
  source: string;
  displayLabel: string;
  status: QueueJobStatus;
  detail?: string;
  /** Filled when status is `done` — path to the primary .txt transcript */
  transcriptPath?: string | null;
  /** Resolved playlist title from `playlist-resolved` (URL jobs only). */
  playlistTitle?: string;
  /** Resolved playlist entries; populated by `playlist-resolved`, then mutated by `subtask-status` events. */
  subtasks?: SubtaskState[];
};
