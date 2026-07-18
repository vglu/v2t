import type { BrowserTrackInfo } from "../types/job";
import type {
  TimedSegment,
  TimedTranscript,
} from "../types/timedTranscript";

/** Must match package.json @xenova/transformers (used for CDN wasm / ort paths). */
const TRANSFORMERS_NPM_VERSION = "2.17.2";

const CATALOG_TO_XENOVA: Record<string, string> = {
  tiny: "Xenova/whisper-tiny",
  base: "Xenova/whisper-base",
  small: "Xenova/whisper-small",
  medium: "Xenova/whisper-medium",
  "large-v3": "Xenova/whisper-large-v3",
  "large-v3-turbo": "Xenova/whisper-large-v3-turbo",
};

/** Soft caps for optional word-piece → cue grouping (factual times only). */
const WORD_PIECE_MAX_CHARS = 50;
const WORD_PIECE_MAX_DURATION_S = 10;
const WORD_PIECE_GAP_BREAK_S = 0.5;
/** Median trimmed length at or below this → treat chunks as word pieces. */
const WORD_PIECE_MEDIAN_CHAR_THRESHOLD = 12;

function mapCatalogToXenova(catalogId: string | undefined | null): string {
  const k = (catalogId ?? "base").trim().toLowerCase();
  return CATALOG_TO_XENOVA[k] ?? "Xenova/whisper-tiny";
}

function extractAsrText(result: unknown): string {
  if (result == null) return "";
  if (typeof result === "string") return result;
  if (typeof result === "object" && "text" in result) {
    const t = (result as { text: unknown }).text;
    return typeof t === "string" ? t : "";
  }
  return String(result);
}

function browserWebVttError(detail: string): Error {
  return new Error(
    `Browser Whisper WebVTT export failed: ${detail}. Disable "Export WebVTT" or use a timestamp-capable mode.`,
  );
}

function secondsToMilliseconds(seconds: number): number {
  return Math.round(seconds * 1000);
}

type ParsedChunk = {
  startSeconds: number;
  endSeconds: number;
  text: string;
  speaker?: string;
};

function optionalSpeaker(value: unknown): string | undefined {
  if (typeof value !== "string") return undefined;
  const trimmed = value.trim();
  return trimmed.length > 0 ? trimmed : undefined;
}

function looksLikeWordPieces(chunks: ParsedChunk[]): boolean {
  if (chunks.length < 2) return false;
  const lengths = chunks.map((c) => c.text.length).sort((a, b) => a - b);
  const mid = Math.floor(lengths.length / 2);
  const median =
    lengths.length % 2 === 0
      ? (lengths[mid - 1]! + lengths[mid]!) / 2
      : lengths[mid]!;
  return median <= WORD_PIECE_MEDIAN_CHAR_THRESHOLD;
}

function toTimedSegment(chunk: ParsedChunk): TimedSegment {
  const startMs = secondsToMilliseconds(chunk.startSeconds);
  const endMs = secondsToMilliseconds(chunk.endSeconds);
  if (endMs <= startMs) {
    throw browserWebVttError(
      "chunk is too short for integer millisecond timestamps",
    );
  }
  const segment: TimedSegment = {
    startMs,
    endMs,
    text: chunk.text,
  };
  if (chunk.speaker !== undefined) {
    segment.speaker = chunk.speaker;
  }
  return segment;
}

/**
 * Merge short word-like chunks into cues using only source start/end times.
 * Does not invent timestamps or speaker labels.
 */
function groupWordPiecesIntoCues(chunks: ParsedChunk[]): TimedSegment[] {
  const cues: TimedSegment[] = [];
  let groupStart = chunks[0]!.startSeconds;
  let groupEnd = chunks[0]!.endSeconds;
  let parts = [chunks[0]!.text];
  let groupSpeaker = chunks[0]!.speaker;

  const flush = () => {
    const text = parts.join(" ").replace(/\s+/g, " ").trim();
    const segment = toTimedSegment({
      startSeconds: groupStart,
      endSeconds: groupEnd,
      text,
      speaker: groupSpeaker,
    });
    cues.push(segment);
  };

  for (let i = 1; i < chunks.length; i++) {
    const chunk = chunks[i]!;
    const gap = chunk.startSeconds - groupEnd;
    const joined = [...parts, chunk.text].join(" ").replace(/\s+/g, " ").trim();
    const duration = chunk.endSeconds - groupStart;
    const speakerMismatch =
      chunk.speaker !== undefined &&
      groupSpeaker !== undefined &&
      chunk.speaker !== groupSpeaker;

    if (
      speakerMismatch ||
      gap > WORD_PIECE_GAP_BREAK_S ||
      joined.length > WORD_PIECE_MAX_CHARS ||
      duration > WORD_PIECE_MAX_DURATION_S
    ) {
      flush();
      groupStart = chunk.startSeconds;
      groupEnd = chunk.endSeconds;
      parts = [chunk.text];
      groupSpeaker = chunk.speaker;
      continue;
    }

    parts.push(chunk.text);
    groupEnd = chunk.endSeconds;
    if (groupSpeaker === undefined && chunk.speaker !== undefined) {
      groupSpeaker = chunk.speaker;
    }
  }

  flush();
  return cues;
}

/** Convert the documented Transformers.js ASR result into the Tauri wire shape. */
export function extractAsrTranscript(
  result: unknown,
  exportWebVtt: boolean,
): TimedTranscript {
  if (!exportWebVtt) {
    return { text: extractAsrText(result).trim(), segments: [] };
  }
  if (result === null || typeof result !== "object") {
    throw browserWebVttError("the model returned an invalid result object");
  }

  const candidate = result as { text?: unknown; chunks?: unknown };
  if (typeof candidate.text !== "string") {
    throw browserWebVttError("the model result has no string text field");
  }
  const text = candidate.text.trim();
  if (candidate.chunks === undefined && text.length === 0) {
    return { text, segments: [] };
  }
  if (!Array.isArray(candidate.chunks)) {
    throw browserWebVttError("no factual segment timestamps were returned");
  }

  let previousStartSeconds = -1;
  const parsed: ParsedChunk[] = candidate.chunks.map((chunk, index) => {
    if (chunk === null || typeof chunk !== "object") {
      throw browserWebVttError(`chunk ${index} is not an object`);
    }
    const value = chunk as {
      text?: unknown;
      timestamp?: unknown;
      speaker?: unknown;
    };
    if (typeof value.text !== "string" || value.text.trim().length === 0) {
      throw browserWebVttError(`chunk ${index} must have nonempty text`);
    }
    if (!Array.isArray(value.timestamp) || value.timestamp.length !== 2) {
      throw browserWebVttError(
        `chunk ${index} must have a numeric [start, end] timestamp`,
      );
    }

    const [startRaw, endRaw] = value.timestamp;
    if (startRaw == null || endRaw == null) {
      throw browserWebVttError(
        `chunk ${index} has an open-ended or missing timestamp incompatible with WebVTT export`,
      );
    }
    if (typeof startRaw !== "number" || typeof endRaw !== "number") {
      throw browserWebVttError(
        `chunk ${index} must have a numeric [start, end] timestamp`,
      );
    }

    const startSeconds = startRaw;
    const endSeconds = endRaw;
    if (!Number.isFinite(startSeconds) || !Number.isFinite(endSeconds)) {
      throw browserWebVttError(`chunk ${index} timestamps must be finite`);
    }
    if (startSeconds < 0 || endSeconds < 0) {
      throw browserWebVttError(
        `chunk ${index} timestamps must be non-negative`,
      );
    }
    if (endSeconds <= startSeconds) {
      throw browserWebVttError(
        `chunk ${index} timestamp end must be greater than start`,
      );
    }
    if (startSeconds < previousStartSeconds) {
      throw browserWebVttError("chunks must be in chronological order");
    }
    previousStartSeconds = startSeconds;

    const startMs = secondsToMilliseconds(startSeconds);
    const endMs = secondsToMilliseconds(endSeconds);
    if (endMs <= startMs) {
      throw browserWebVttError(
        `chunk ${index} is too short for integer millisecond timestamps`,
      );
    }

    return {
      startSeconds,
      endSeconds,
      text: value.text.trim(),
      speaker: optionalSpeaker(value.speaker),
    };
  });

  if (text.length > 0 && parsed.length === 0) {
    throw browserWebVttError("no factual segment timestamps were returned");
  }

  const segments = looksLikeWordPieces(parsed)
    ? groupWordPiecesIntoCues(parsed)
    : parsed.map(toTimedSegment);

  return { text, segments };
}

function configureTransformersForTauriWebview(
  env: Awaited<typeof import("@xenova/transformers")>["env"],
): void {
  // Vite может подмешать полифиллы fs/path → transformers.js считает среду Node и
  // выставляет локальный wasmPaths (неверный в webview). Явно грузим ort-wasm с CDN.
  env.useFS = false;
  env.useFSCache = false;
  env.allowLocalModels = false;
  env.allowRemoteModels = true;

  const wasm = env.backends?.onnx?.wasm;
  if (wasm) {
    wasm.wasmPaths = `https://cdn.jsdelivr.net/npm/@xenova/transformers@${TRANSFORMERS_NPM_VERSION}/dist/`;
    // WebView2: проще без SIMD / потоков (меньше сюрпризов при первом запуске).
    if ("simd" in wasm) {
      (wasm as { simd: boolean }).simd = false;
    }
    if ("numThreads" in wasm) {
      (wasm as { numThreads: number }).numThreads = 1;
    }
  }
}

export async function transcribeBrowserTracks(args: {
  whisperModelId: string;
  tracks: BrowserTrackInfo[];
  language: string | null;
  exportWebVtt: boolean;
  shouldStop: () => boolean;
  onProgress?: (message: string) => void;
}): Promise<TimedTranscript[]> {
  let mod: typeof import("@xenova/transformers");
  try {
    mod = await import("@xenova/transformers");
  } catch (e) {
    const inner = e instanceof Error ? e.message : String(e);
    throw new Error(`Transformers.js не загрузился: ${inner}`);
  }
  if (args.shouldStop()) {
    throw new Error("Job cancelled");
  }

  const { pipeline, env } = mod;
  configureTransformersForTauriWebview(env);

  const modelId = mapCatalogToXenova(args.whisperModelId);
  args.onProgress?.(`Loading in-app model ${modelId}…`);

  let transcriber: Awaited<ReturnType<typeof pipeline>>;
  try {
    transcriber = await pipeline("automatic-speech-recognition", modelId);
  } catch (e) {
    const inner = e instanceof Error ? e.message : String(e);
    throw new Error(`Не удалось открыть модель ${modelId}: ${inner}`);
  }
  if (args.shouldStop()) {
    throw new Error("Job cancelled");
  }

  const { convertFileSrc } = await import("@tauri-apps/api/core");
  const results: TimedTranscript[] = [];

  for (let i = 0; i < args.tracks.length; i++) {
    if (args.shouldStop()) {
      throw new Error("Job cancelled");
    }
    const t = args.tracks[i]!;
    if (t.skipTranscribe) {
      results.push({ text: "", segments: [] });
      continue;
    }
    args.onProgress?.(`Transcribing track ${i + 1}/${args.tracks.length} (WASM)…`);
    const url = convertFileSrc(t.wavPath);
    try {
      const options = {
        chunk_length_s: 30,
        language: args.language ?? undefined,
        task: "transcribe",
        ...(args.exportWebVtt ? { return_timestamps: true } : {}),
      };
      if (args.shouldStop()) {
        throw new Error("Job cancelled");
      }
      const raw = await transcriber(url, options);
      if (args.shouldStop()) {
        throw new Error("Job cancelled");
      }
      results.push(extractAsrTranscript(raw, args.exportWebVtt));
    } catch (e) {
      const inner = e instanceof Error ? e.message : String(e);
      throw new Error(
        `Транскрипция трека ${i + 1} не удалась (${t.wavPath}): ${inner}`,
      );
    }
  }

  return results;
}
