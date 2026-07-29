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

/** Matches Rust `AcquiredArtifact` (serde camelCase). */
export type AcquiredArtifact = {
  workDir: string;
  wavPaths: string[];
  /** Absolute paths to the source media files produced by yt-dlp / ffmpeg. */
  sourceMediaFiles: string[];
  sourceMediaFilesAreAudio: boolean;
  /** Remote enrichment fetched during Acquire; used by the info-sidecar writer. */
  remoteMediaInfo?: unknown | null;
  /** UTC date string (YYYY-MM-DD) used in output file names. */
  date: string;
  displayLabel: string;
  /** Number of wav tracks produced by Acquire. */
  nTracks: number;
  /** Approximate size of all wav files, for backpressure accounting (bytes). */
  bytesOnDisk: number;
};

/** Matches Rust `AcquireOutcome` (serde tag `kind`, camelCase). */
export type AcquireOutcome =
  | ({ kind: "ready" } & AcquiredArtifact)
  | {
      kind: "completed";
      transcriptPath: string;
      summary: string;
    }
  | {
      kind: "browserPrepared";
      tracks: BrowserTrackInfo[];
      workDir: string;
      deleteAudioAfter: boolean;
      language: string | null;
      whisperModelId: string;
      exportWebVtt: boolean;
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
      exportWebVtt: boolean;
    };

export type QueueJobProgressPayload = {
  jobId: string;
  phase: string;
  message: string;
};
