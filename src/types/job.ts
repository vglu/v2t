/** Matches Rust `ProcessQueueItemResult` (serde camelCase). */
export type ProcessQueueItemResult = {
  transcriptPath: string;
  summary: string;
};

export type QueueJobProgressPayload = {
  jobId: string;
  phase: string;
  message: string;
};
