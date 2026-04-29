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
};
