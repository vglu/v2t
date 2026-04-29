/**
 * Custom language detector for `i18next-browser-languagedetector`.
 *
 * Bridges the gap between i18next (which has its own LocalStorage cache) and
 * v2t's Tauri-backed `settings.json` (the single source of truth for user
 * preferences). The detector chain is:
 *
 *   1. `customSettings`  — reads `globalThis.__v2tUiLanguage`. App.tsx sets
 *                          this *before* mounting <App/>, after `loadSettings`
 *                          resolves.
 *   2. `navigator`        — falls back to `navigator.language` when (1)
 *                          returns `undefined` (first launch, no settings.json
 *                          yet, or `uiLanguage = "auto"`).
 *
 * `cacheUserLanguage` is a no-op on purpose: persistence is handled by
 * `saveSettings({ uiLanguage })` from React, not by i18next, so the two
 * stores can never disagree.
 */
import type { CustomDetector } from "i18next-browser-languagedetector";

declare global {
  // eslint-disable-next-line no-var
  var __v2tUiLanguage: string | undefined;
}

export const settingsDetector: CustomDetector = {
  name: "customSettings",
  lookup() {
    const lang = globalThis.__v2tUiLanguage;
    if (!lang || lang === "auto") return undefined;
    return lang;
  },
  cacheUserLanguage() {
    // No-op: persisted via Tauri settings.json by the React layer.
  },
};
