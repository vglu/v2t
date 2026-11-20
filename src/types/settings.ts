export type TranscriptionMode =
  | "httpApi"
  | "localWhisper"
  | "browserWhisper";

export type VisionMode = "disabled" | "gemini";

/** Browser to pull yt-dlp cookies from. "auto" = OS default; "none" = disabled. */
export type CookiesFromBrowser = "auto" | "chrome" | "brave" | "edge" | "firefox" | "none";

/** Audio format when saving downloaded audio. "original" = no re-encode. */
export type DownloadedAudioFormat = "original" | "mp3" | "m4a";

/** Local Whisper backend: "auto" picks CUDA on NVIDIA, else CPU. */
export type WhisperAcceleration = "auto" | "cuda" | "vulkan" | "cpu";

/** When pyannote diarize fails after ASR. */
export type DiarizeFailPolicy = "keepTranscript" | "failJob";

/** When the Brief stage fails after a successful transcription. */
export type BriefFailPolicy = "keepTranscript" | "failJob";

/** Shape of the post-transcription design document PDF. */
export type BriefDocMode = "fdd" | "brief";

/** Quality profile for Brief / FDD generation.
 * "fast" → briefModel (lite); "accurate" → briefModelAccurate (full Flash, FDD only). */
export type BriefModelProfile = "fast" | "accurate";

/** Returns the effective Gemini model based on profile and doc mode. */
export function effectiveBriefModel(settings: AppSettings): string {
  const useAccurate =
    settings.briefModelProfile === "accurate" && settings.briefDocMode === "fdd";
  if (useAccurate) {
    return settings.briefModelAccurate || "gemini-3.1-flash";
  }
  return settings.briefModel || "gemini-3.1-flash-lite";
}

/** UI language. "auto" lets React derive from navigator.language; the
 * other values are ISO 639-1 codes matching the i18n catalogs in
 * `src/locales/`. Mirror of the Rust `UiLanguage` enum (`settings.rs`). */
export type UiLanguage =
  | "auto"
  | "en"
  | "uk"
  | "ru"
  | "de"
  | "es"
  | "fr"
  | "pl"
  | "pt";

/** Intent preset: Simple / Quality / Power, or Custom after manual edits. */
export type ProfileId = "simple" | "quality" | "power" | "custom";

/** Result of `Win32_VideoController` classification (Windows) or empty (macOS / Linux). */
export type GpuKind = "nvidia" | "amd" | "intel" | "none";
export type GpuInfo = { kind: GpuKind; names: string[] };

export type AppSettings = {
  outputDir: string | null;
  filenameTemplate: string;
  ffmpegPath: string | null;
  ytDlpPath: string | null;
  /** yt-dlp `--js-runtimes` value when non-empty (YouTube EJS). */
  ytDlpJsRuntimes: string | null;
  /** Browser for yt-dlp --cookies-from-browser (helps with age-gated YouTube / TikTok). */
  cookiesFromBrowser: CookiesFromBrowser;
  deleteAudioAfter: boolean;
  /** URL jobs: also save merged best-quality video (.mp4) to the output folder (second yt-dlp pass). */
  keepDownloadedVideo: boolean;
  /** URL jobs + local video: also save the extracted audio file to the output folder. */
  keepDownloadedAudio: boolean;
  /** Also write `{stem}.info.json` next to the transcript (title, URL, dates, …). */
  keepMediaInfo: boolean;
  /** Format for saved audio; "original" keeps bestaudio from yt-dlp / copies local video's audio stream without re-encoding. */
  downloadedAudioFormat: DownloadedAudioFormat;
  apiBaseUrl: string;
  apiModel: string;
  apiKey: string;
  language: string | null;
  recursiveFolderScan: boolean;
  /** false = show first-run setup wizard once; persisted in settings.json */
  onboardingCompleted: boolean;
  /** Intent preset for defaults and Preferences surface. Missing → custom (upgrades). */
  profileId: ProfileId;
  transcriptionMode: TranscriptionMode;
  /** Path to whisper-cli or main (optional). */
  whisperCliPath: string | null;
  /** ggml model directory (optional → app data / models). */
  whisperModelsDir: string | null;
  /** Catalog id: tiny, base, small, … */
  whisperModel: string;
  /** Local Whisper backend (Windows only — macOS uses Metal via Homebrew bottle). */
  whisperAcceleration: WhisperAcceleration;
  /** When a YouTube video has manual subtitles in a priority language, fetch them
   * via yt-dlp and skip the download + Whisper passes entirely. */
  useSubtitlesWhenAvailable: boolean;
  /** Priority order for picking the manual subtitle track. First match wins. */
  subtitlePriorityLangs: string[];
  /** When the subtitle fast-path runs, also save the raw `.srt` next to `.txt`. */
  keepSrt: boolean;
  /** Also save a timed `.vtt` transcript next to the plain `.txt` output. */
  exportWebVtt: boolean;
  /** Label speakers as Person 1 / Person 2 via local tinydiarize (experimental). */
  labelSpeakers: boolean;
  /** Post-ASR pyannote diarize (any language). Requires WebVTT + HF token + Python. */
  diarizeSpeakers: boolean;
  /** Hugging Face token for gated pyannote models (keyring). */
  hfToken: string;
  /** Path to Python with pyannote (venv python.exe). */
  diarizePythonPath: string | null;
  /** Optional path to run_diarize.py. */
  diarizeScriptPath: string | null;
  /** On diarize failure: keep transcript without Person tags, or fail the job. */
  diarizeFailPolicy: DiarizeFailPolicy;
  /** UI language. "auto" derives from navigator.language. */
  uiLanguage: UiLanguage;
  /** Local REST API server config (bind 127.0.0.1 only). */
  apiServer: ApiServerSettings;
  /** Vision / OCR mode. "disabled" = no OCR processing. */
  visionMode: VisionMode;
  /** Gemini model for Vision OCR (e.g. "gemini-2.5-flash"). */
  geminiModel: string;
  /** Google API key for Gemini Vision. */
  geminiApiKey: string;
  /** True = user is on free tier; show rate limit warnings. */
  geminiFreeTier: boolean;

  // ── Download pacing & retry (TASK-Q2-2) ──────────────────────────────────

  /** Minimum gap (ms) between yt-dlp spawns for the same host. */
  downloadMinIntervalMs: number;
  /** Random jitter (ms) added on top of downloadMinIntervalMs. */
  downloadJitterMs: number;
  /** Maximum download attempts per item when the error is retryable. */
  downloadMaxAttempts: number;
  /** Base duration (ms) for exponential backoff between retry attempts. */
  downloadBackoffBaseMs: number;
  /** Upper cap (ms) for both backoff and per-host cooldown. */
  downloadBackoffMaxMs: number;
  /** Per-host cooldown (ms) applied after a 429 response. */
  downloadCooldownMs: number;
  /** Number of concurrent URL downloads. */
  acquireConcurrency: number;
  /** Number of concurrent transcriptions. */
  transcribeConcurrency: number;
  /** Backpressure: max downloaded-but-not-yet-transcribed items. */
  maxPendingArtifacts: number;
  /** Backpressure: max total size (MiB) of pending artifacts. */
  maxPendingArtifactsMb: number;

  // ── Brief / FDD (Design Doc PDF) ──────────────────────────────────────────

  /** When true, run the design-doc stage after a successful transcription. */
  generateBrief: boolean;
  /** Meeting Brief (default) or FDD (functional design doc). */
  briefDocMode: BriefDocMode;
  /** Gemini model for design-doc generation (separate from Vision OCR model). */
  briefModel: string;
  /** User acknowledged that transcripts + screenshots will be sent to a cloud provider. */
  briefProviderPrivacyAcknowledged: boolean;
  /** When the design-doc stage fails: keep transcript or fail the whole job. */
  briefFailPolicy: BriefFailPolicy;
  /** Quality profile: "fast" uses briefModel (lite), "accurate" uses briefModelAccurate for FDD. */
  briefModelProfile: BriefModelProfile;
  /** Gemini model for accurate profile (default: gemini-3.1-flash). */
  briefModelAccurate: string;
};

export type ApiServerSettings = {
  enabled: boolean;
  port: number;
  /** Static bearer token; auto-generated on first enable. */
  bearerToken: string;
};

export const defaultAppSettings: AppSettings = {
  outputDir: null,
  filenameTemplate: "{title}_{date}.{ext}",
  ffmpegPath: null,
  ytDlpPath: null,
  ytDlpJsRuntimes: null,
  cookiesFromBrowser: "auto",
  deleteAudioAfter: true,
  keepDownloadedVideo: false,
  keepDownloadedAudio: false,
  keepMediaInfo: false,
  downloadedAudioFormat: "original",
  apiBaseUrl: "https://api.openai.com/v1",
  apiModel: "whisper-1",
  apiKey: "",
  language: null,
  recursiveFolderScan: false,
  onboardingCompleted: true,
  profileId: "simple",
  transcriptionMode: "httpApi",
  whisperCliPath: null,
  whisperModelsDir: null,
  whisperModel: "base",
  whisperAcceleration: "auto",
  useSubtitlesWhenAvailable: true,
  subtitlePriorityLangs: ["uk", "ru", "en"],
  keepSrt: false,
  exportWebVtt: false,
  labelSpeakers: false,
  diarizeSpeakers: false,
  hfToken: "",
  diarizePythonPath: null,
  diarizeScriptPath: null,
  diarizeFailPolicy: "keepTranscript",
  uiLanguage: "auto",
  apiServer: { enabled: false, port: 8788, bearerToken: "" },
  visionMode: "disabled",
  geminiModel: "gemini-2.5-flash",
  geminiApiKey: "",
  geminiFreeTier: true,
  downloadMinIntervalMs: 20000,
  downloadJitterMs: 5000,
  downloadMaxAttempts: 4,
  downloadBackoffBaseMs: 5000,
  downloadBackoffMaxMs: 120000,
  downloadCooldownMs: 60000,
  acquireConcurrency: 1,
  transcribeConcurrency: 1,
  maxPendingArtifacts: 10,
  maxPendingArtifactsMb: 4096,
  generateBrief: false,
  briefDocMode: "brief",
  briefModel: "gemini-3.1-flash-lite",
  briefProviderPrivacyAcknowledged: false,
  briefFailPolicy: "keepTranscript",
  briefModelProfile: "fast",
  briefModelAccurate: "gemini-3.1-flash",
};

export type DependencyReport = {
  ffmpegFound: boolean;
  ffmpegPath: string | null;
  ytDlpFound: boolean;
  ytDlpPath: string | null;
  whisperCliFound: boolean;
  whisperCliPath: string | null;
  /** Local Whisper: selected ggml file on disk and SHA-1 matches catalog. */
  whisperModelReady: boolean;
};

export type WhisperModelMeta = {
  id: string;
  fileName: string;
  sizeMib: number;
};

export type DownloadedMediaTools = {
  ffmpegPath: string;
  ytDlpPath: string;
};

export type DownloadedWhisperCli = {
  whisperCliPath: string;
};

export type InstalledDeno = {
  jsRuntimes: string;
};

export type ModelValidationResult = {
  isValid: boolean;
  suggestedReplacement: string | null;
  error: string | null;
};

export type CostEstimate = {
  imageCount: number;
  flashCostUsd: number;
  flashLiteCostUsd: number;
  freeTierSeconds: number;
};
