import type { AppSettings, ProfileId } from "../types/settings";

export type { ProfileId };

export const NAMED_PROFILES: Exclude<ProfileId, "custom">[] = [
  "simple",
  "quality",
  "power",
];

/** Fields owned by presets (keys, paths, output folder, tokens are never touched). */
export type PresetSlice = Pick<
  AppSettings,
  | "exportWebVtt"
  | "labelSpeakers"
  | "useSubtitlesWhenAvailable"
  | "keepSrt"
  | "keepDownloadedVideo"
  | "keepDownloadedAudio"
  | "deleteAudioAfter"
  | "downloadedAudioFormat"
  | "whisperModel"
  | "apiModel"
  | "apiBaseUrl"
  | "recursiveFolderScan"
  | "cookiesFromBrowser"
  | "whisperAcceleration"
  | "visionMode"
  | "geminiFreeTier"
>;

function sliceFrom(s: AppSettings): PresetSlice {
  return {
    exportWebVtt: s.exportWebVtt,
    labelSpeakers: s.labelSpeakers,
    useSubtitlesWhenAvailable: s.useSubtitlesWhenAvailable,
    keepSrt: s.keepSrt,
    keepDownloadedVideo: s.keepDownloadedVideo,
    keepDownloadedAudio: s.keepDownloadedAudio,
    deleteAudioAfter: s.deleteAudioAfter,
    downloadedAudioFormat: s.downloadedAudioFormat,
    whisperModel: s.whisperModel,
    apiModel: s.apiModel,
    apiBaseUrl: s.apiBaseUrl,
    recursiveFolderScan: s.recursiveFolderScan,
    cookiesFromBrowser: s.cookiesFromBrowser,
    whisperAcceleration: s.whisperAcceleration,
    visionMode: s.visionMode,
    geminiFreeTier: s.geminiFreeTier,
  };
}

const PRESETS: Record<Exclude<ProfileId, "custom">, PresetSlice> = {
  simple: {
    exportWebVtt: false,
    labelSpeakers: false,
    useSubtitlesWhenAvailable: true,
    keepSrt: false,
    keepDownloadedVideo: false,
    keepDownloadedAudio: false,
    deleteAudioAfter: true,
    downloadedAudioFormat: "original",
    whisperModel: "base",
    apiModel: "whisper-1",
    apiBaseUrl: "https://api.openai.com/v1",
    recursiveFolderScan: false,
    cookiesFromBrowser: "auto",
    whisperAcceleration: "auto",
    visionMode: "disabled",
    geminiFreeTier: true,
  },
  quality: {
    exportWebVtt: true,
    labelSpeakers: false,
    useSubtitlesWhenAvailable: false,
    keepSrt: false,
    keepDownloadedVideo: false,
    keepDownloadedAudio: true,
    deleteAudioAfter: false,
    downloadedAudioFormat: "m4a",
    whisperModel: "large-v3",
    apiModel: "whisper-1",
    apiBaseUrl: "https://api.openai.com/v1",
    recursiveFolderScan: false,
    cookiesFromBrowser: "auto",
    whisperAcceleration: "auto",
    visionMode: "disabled",
    geminiFreeTier: true,
  },
  power: {
    exportWebVtt: true,
    labelSpeakers: false,
    useSubtitlesWhenAvailable: true,
    keepSrt: true,
    keepDownloadedVideo: true,
    keepDownloadedAudio: true,
    deleteAudioAfter: false,
    downloadedAudioFormat: "m4a",
    whisperModel: "large-v3",
    apiModel: "whisper-1",
    apiBaseUrl: "https://api.openai.com/v1",
    recursiveFolderScan: true,
    cookiesFromBrowser: "auto",
    whisperAcceleration: "auto",
    visionMode: "disabled",
    geminiFreeTier: true,
  },
};

export function presetSlice(id: Exclude<ProfileId, "custom">): PresetSlice {
  return { ...PRESETS[id] };
}

export function matchesPreset(
  settings: AppSettings,
  id: Exclude<ProfileId, "custom">,
): boolean {
  const a = sliceFrom(settings);
  const b = PRESETS[id];
  return (Object.keys(b) as (keyof PresetSlice)[]).every((k) => a[k] === b[k]);
}

/** Apply a named preset. Preserves keys, paths, output dir, mode, language, REST token. */
export function applyPreset(
  settings: AppSettings,
  id: Exclude<ProfileId, "custom">,
): AppSettings {
  const next: AppSettings = {
    ...settings,
    ...PRESETS[id],
    profileId: id,
  };
  // Power may enable REST later via UI; never force-enable here (security).
  if (id === "simple") {
    next.apiServer = { ...settings.apiServer, enabled: false };
    next.labelSpeakers = false;
  }
  return next;
}

/** If profileId is a named preset but fields diverged → `custom`. */
export function reconcileProfileId(settings: AppSettings): AppSettings {
  const id = settings.profileId;
  if (id === "custom") return settings;
  if (id === "simple" || id === "quality" || id === "power") {
    if (matchesPreset(settings, id)) return settings;
    return { ...settings, profileId: "custom" };
  }
  return { ...settings, profileId: "custom" };
}

export function isSimpleSurface(profileId: ProfileId): boolean {
  return profileId === "simple";
}

export function parseProfileId(raw: unknown): ProfileId {
  if (raw === "simple" || raw === "quality" || raw === "power" || raw === "custom") {
    return raw;
  }
  return "custom";
}
