/**
 * Constants — Ollama URL, model, paths, brand glossary, target locales.
 *
 * Adapted from NumbersM W1B for v2t. Source of truth for the bot's input
 * is `src/locales/en/*.json` (filled by M3a). Bot synthesizes an in-memory
 * audit shape from those files plus any pre-existing per-locale partial
 * catalogs in `src/locales/{lang}/*.json` (resume-friendly).
 *
 * All overridable via env (see comments). Defaults are tuned for the
 * RTX 3060 Ti dev machine.
 */
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));

/** Repo root — `<root>/scripts/translation-bot/src/config.ts` → 3 levels up. */
export const REPO_ROOT = path.resolve(__dirname, '..', '..', '..');
export const BOT_ROOT = path.resolve(__dirname, '..');

/** EN catalog directory — bot reads every `*.json` here as source-of-truth keys. */
export const LOCALE_SOURCE_DIR = path.join(REPO_ROOT, 'src', 'locales', 'en');
/** Per-locale partial catalogs (if a key is already translated there, bot skips it). */
export const LOCALE_TARGET_DIR = path.join(REPO_ROOT, 'src', 'locales');

/** Bot-internal directories — gitignored. Created at first run. */
export const STATE_DIR = path.join(BOT_ROOT, 'state');
export const STATE_FILE = path.join(STATE_DIR, 'bot-state.json');
export const DRAFTS_DIR = path.join(BOT_ROOT, 'output', 'drafts');
export const LOGS_DIR = path.join(BOT_ROOT, 'logs');

/** Ollama bare-metal endpoint (NOT the docker port). */
export const OLLAMA_URL = process.env.OLLAMA_URL ?? 'http://localhost:11434';

/** Default model — Qwen2.5-Coder:latest (7.6B Q4_K_M, ~5GB VRAM). */
export const OLLAMA_MODEL = process.env.OLLAMA_MODEL ?? 'Qwen2.5-Coder:latest';

/** Inference parameters — tuned for translation, not generation. */
export const INFERENCE_OPTIONS = {
  /** Low temp = consistent translations across runs. */
  temperature: 0.3,
  /** Slightly relaxed top_p — single-string outputs need some variance. */
  top_p: 0.9,
  /** Cap output length — long onboarding paragraphs may need ~400 tokens. */
  num_predict: 400,
} as const;

/** Throttle between calls to keep GPU steady (ms). */
export const THROTTLE_MS = Number(process.env.THROTTLE_MS ?? '200');

/** Flush state to disk every N completed strings (crash-loss bound). */
export const STATE_FLUSH_EVERY = 5;

/** Auto-pause threshold — if rolling avg > this, pause 30s before continuing. */
export const SLOW_AVG_THRESHOLD_MS = 15_000;
export const AUTO_PAUSE_MS = 30_000;

/** Retry policy for transient failures. */
export const RETRY_DELAYS_MS = [2_000, 5_000, 10_000];

/** Per-call timeout (ms) — Qwen2.5-Coder:latest p99 ~25s; long onboarding paragraphs need headroom. */
export const CALL_TIMEOUT_MS = 90_000;

/**
 * Locales we translate to. EN is source. v2t targets UA-first (PO native),
 * RU + Western European mid-priority, PT for Iberian / Brazilian audience.
 */
export const TARGET_LOCALES = ['uk', 'ru', 'de', 'es', 'fr', 'pl', 'pt'] as const;
export type TargetLocale = (typeof TARGET_LOCALES)[number];

/**
 * Brand / technical glossary — never translated by the model. If a term
 * appears in the EN source, it MUST appear verbatim in the translation;
 * otherwise `GLOSSARY_LOST` warning fires.
 *
 * Match is case-insensitive but preservation is exact. Includes proper
 * names (yt-dlp, ffmpeg, Whisper), file extensions (mp3, m4a, srt, …),
 * acceleration names (CUDA, Vulkan, cuBLAS), platform names, and brands.
 */
export const BRAND_GLOSSARY = [
  'v2t',
  'Video to Text',
  'yt-dlp',
  'yt-dlp.exe',
  'yt-dlp_macos',
  'ffmpeg',
  'ffmpeg.exe',
  'ffprobe',
  'Whisper',
  'whisper.cpp',
  'whisper-cli',
  'whisper-cli.exe',
  'whisper-bin-x64.zip',
  'whisper-cpp',
  'CUDA',
  'cuBLAS',
  'Vulkan',
  'GGML',
  'Tauri',
  'WASM',
  'Transformers.js',
  'Deno',
  'Node',
  'Node.js',
  'MiB',
  'MB',
  'KB',
  'mp3',
  'm4a',
  'mp4',
  'wav',
  'srt',
  'vtt',
  'webm',
  'opus',
  'aac',
  'flac',
  'ogg',
  'txt',
  'json',
  'bin',
  'URL',
  'HTTP',
  'API',
  'OAuth',
  'JWT',
  'SHA-1',
  'AAC',
  'EJS',
  'DPAPI',
  'AUR',
  'Chrome',
  'Brave',
  'Edge',
  'Firefox',
  'Windows',
  'macOS',
  'Linux',
  'Ubuntu',
  'Debian',
  'Fedora',
  'Arch',
  'Homebrew',
  'OpenAI',
  'YouTube',
  'TikTok',
  'Anthropic',
  'GitHub',
  'GPL',
  'MIT',
  'BtbN/FFmpeg-Builds',
  'ggml-org/whisper.cpp',
  'ffmpeg-static',
] as const;

export const LOCALE_LABELS: Record<TargetLocale, string> = {
  uk: 'Ukrainian (українська)',
  ru: 'Russian (русский)',
  de: 'German (Deutsch)',
  es: 'Spanish (español)',
  fr: 'French (français)',
  pl: 'Polish (polski)',
  pt: 'Portuguese (português)',
};

/** Friendly description of an i18n namespace prefix (used in prompt context). */
export const NAMESPACE_HINTS: Record<string, string> = {
  common: 'app header, main tabs (Queue / Settings), generic toasts and buttons',
  onboarding: 'first-run setup wizard for ffmpeg / yt-dlp / Whisper, 6 steps',
  settings: 'preferences panel — output dir, transcription mode, Whisper models, subtitles fast-path, media tools',
  queue: 'job queue table, drop zone, per-job progress, subtask list, log toolbar',
  readiness: 'top-bar dependency checklist (ffmpeg / yt-dlp / Whisper model status)',
};
