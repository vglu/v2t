/** Matches Rust `ProcessQueueItemResult` (serde camelCase). */
export type ProcessQueueItemResult = {
  transcriptPath: string;
  summary: string;
};

export type BrowserTrackInfo = {
  wavPath: string;
  transcriptPath: string;
  skipTranscribe: boolean;
};

/** Matches Rust `ProcessQueueItemOutcome` (serde tag `kind`, camelCase). */
export type ProcessQueueItemOutcome =
  | { kind: "done"; transcriptPath: string; summary: string }
  | {
      kind: "browserPrepared";
      tracks: BrowserTrackInfo[];
      workDir: string;
      deleteAudioAfter: boolean;
      language: string | null;
      whisperModelId: string;
    };

export type QueueJobProgressPayload = {
  jobId: string;
  phase: string;
  message: string;
};
