/** Preference sheet depth (IA: Essentials / Engine / Advanced). */
export type PrefsDepth = "essentials" | "engine" | "advanced";

/** Optional scroll/highlight target inside the open depth. */
export type PrefsFocus =
  | "output-dir"
  | "whisper-model"
  | "api-credentials"
  | "ffmpeg"
  | "yt-dlp"
  | "api-server"
  | null;

export type PrefsTarget = {
  depth: PrefsDepth;
  focus?: PrefsFocus;
};
