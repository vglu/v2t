import type { ProcessQueueItemResult } from "../types/job";
import type { PrepareAudioResult } from "../types/pipeline";
import type {
  AppSettings,
  DependencyReport,
  DownloadedMediaTools,
  WhisperModelMeta,
} from "../types/settings";

export async function loadSettings(): Promise<AppSettings | null> {
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    return await invoke<AppSettings>("load_settings");
  } catch {
    return null;
  }
}

export async function saveSettings(settings: AppSettings): Promise<boolean> {
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    await invoke("save_settings", { settings });
    return true;
  } catch {
    return false;
  }
}

export async function checkDependencies(
  ffmpegPath: string | null,
  ytDlpPath: string | null,
  whisperCliPath: string | null,
): Promise<DependencyReport | null> {
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    return await invoke<DependencyReport>("check_dependencies", {
      ffmpegPathOverride: ffmpegPath,
      ytDlpPathOverride: ytDlpPath,
      whisperCliPathOverride: whisperCliPath,
    });
  } catch {
    return null;
  }
}

export async function scanMediaFolder(
  path: string,
  recursive: boolean,
): Promise<string[] | null> {
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    return await invoke<string[]>("scan_media_folder", { path, recursive });
  } catch {
    return null;
  }
}

export async function prepareMediaAudio(
  source: string,
  sourceKind: "url" | "file",
  ffmpegPath: string | null,
  ytDlpPath: string | null,
): Promise<PrepareAudioResult> {
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    return await invoke<PrepareAudioResult>("prepare_media_audio", {
      source,
      sourceKind,
      ffmpegPathOverride: ffmpegPath,
      ytDlpPathOverride: ytDlpPath,
    });
  } catch (e) {
    const msg =
      typeof e === "string"
        ? e
        : e instanceof Error
          ? e.message
          : "invoke failed (run inside Tauri app)";
    throw new Error(msg);
  }
}

export async function cancelQueueJob(jobId: string | null): Promise<void> {
  if (!jobId) return;
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    await invoke("cancel_queue_job", { jobId });
  } catch {
    /* web / tests */
  }
}

export async function listWhisperModels(): Promise<WhisperModelMeta[] | null> {
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    return await invoke<WhisperModelMeta[]>("list_whisper_models");
  } catch {
    return null;
  }
}

export async function defaultDocumentsDir(): Promise<string | null> {
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    return await invoke<string>("default_documents_dir");
  } catch {
    return null;
  }
}

export async function downloadMediaTools(): Promise<DownloadedMediaTools> {
  const { invoke } = await import("@tauri-apps/api/core");
  return await invoke<DownloadedMediaTools>("download_media_tools");
}

export async function defaultWhisperModelsDir(): Promise<string | null> {
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    return await invoke<string>("default_whisper_models_dir");
  } catch {
    return null;
  }
}

export async function downloadWhisperModel(
  modelId: string,
  modelsDir: string | null,
): Promise<void> {
  const { invoke } = await import("@tauri-apps/api/core");
  await invoke("download_whisper_model", {
    modelId,
    modelsDir,
  });
}

export async function processQueueItem(args: {
  jobId: string;
  jobIndex: number;
  source: string;
  sourceKind: "url" | "file";
  displayLabel: string;
  settings: AppSettings;
}): Promise<ProcessQueueItemResult> {
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    const { settings, jobId, jobIndex, source, sourceKind, displayLabel } =
      args;
    return await invoke<ProcessQueueItemResult>("process_queue_item", {
      jobId,
      jobIndex,
      source,
      sourceKind,
      displayLabel,
      settings,
      ffmpegPathOverride: settings.ffmpegPath,
      ytDlpPathOverride: settings.ytDlpPath,
    });
  } catch (e) {
    const msg =
      typeof e === "string"
        ? e
        : e instanceof Error
          ? e.message
          : "invoke failed (run inside Tauri app)";
    throw new Error(msg);
  }
}
