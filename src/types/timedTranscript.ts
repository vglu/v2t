/** Matches Rust `TimedSegment` on the Tauri camelCase wire contract. */
export type TimedSegment = {
  startMs: number;
  endMs: number;
  text: string;
  /** Present when the source (e.g. subtitles) supplied a speaker label. */
  speaker?: string;
};

/** Matches Rust `TimedTranscript` on the Tauri camelCase wire contract. */
export type TimedTranscript = {
  text: string;
  segments: TimedSegment[];
};
