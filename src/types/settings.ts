export type TranscriptionMode = "httpApi" | "localWhisper";

export type AppSettings = {
  outputDir: string | null;
  filenameTemplate: string;
  ffmpegPath: string | null;
  ytDlpPath: string | null;
  deleteAudioAfter: boolean;
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
  filenameTemplate: "{title}_{date}.txt",
  ffmpegPath: null,
  ytDlpPath: null,
  deleteAudioAfter: true,
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
