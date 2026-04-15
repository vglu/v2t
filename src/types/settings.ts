export type TranscriptionMode =
  | "httpApi"
  | "localWhisper"
  | "browserWhisper";

/** Browser to pull yt-dlp cookies from. "auto" = OS default; "none" = disabled. */
export type CookiesFromBrowser = "auto" | "chrome" | "brave" | "edge" | "firefox" | "none";

/** Audio format when saving downloaded audio. "original" = no re-encode. */
export type DownloadedAudioFormat = "original" | "mp3" | "m4a";

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
  /** Format for saved audio; "original" keeps bestaudio from yt-dlp / copies local video's audio stream without re-encoding. */
  downloadedAudioFormat: DownloadedAudioFormat;
  apiBaseUrl: string;
  apiModel: string;
  apiKey: string;
  language: string | null;
  recursiveFolderScan: boolean;
  /** false = show first-run setup wizard once; persisted in settings.json */
  onboardingCompleted: boolean;
  transcriptionMode: TranscriptionMode;
  /** Path to whisper-cli or main (optional). */
  whisperCliPath: string | null;
  /** ggml model directory (optional → app data / models). */
  whisperModelsDir: string | null;
  /** Catalog id: tiny, base, small, … */
  whisperModel: string;
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
  downloadedAudioFormat: "original",
  apiBaseUrl: "https://api.openai.com/v1",
  apiModel: "whisper-1",
  apiKey: "",
  language: null,
  recursiveFolderScan: false,
  onboardingCompleted: true,
  transcriptionMode: "httpApi",
  whisperCliPath: null,
  whisperModelsDir: null,
  whisperModel: "base",
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
