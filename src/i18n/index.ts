/**
 * i18n entry point — initialized exactly once during app boot.
 *
 * Imported (for side effects) by `src/main.tsx` *before* `<App/>` so the
 * very first render already has translations. `App.tsx` later writes to
 * `globalThis.__v2tUiLanguage` after `loadSettings()` resolves and calls
 * `i18next.changeLanguage(...)` to apply the persisted choice.
 *
 * Configured invariants:
 * - `fallbackLng = "en"` — missing keys / unsupported locales degrade to EN.
 * - `supportedLngs` lists every locale the bot will eventually fill in.
 *   Catalogs that haven't shipped yet are still safe — i18next falls back
 *   per-key, not per-locale.
 * - `interpolation.escapeValue = false` — React already escapes; double
 *   escape would break `<Trans/>` HTML composition.
 * - `detection.caches = []` — disables i18next's own LocalStorage cache so
 *   we don't end up with two competing sources of truth.
 */
import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import LanguageDetector from "i18next-browser-languagedetector";

import { resources } from "./resources";
import { settingsDetector } from "./customDetector";

export const SUPPORTED_LANGUAGES = ["en", "uk", "ru", "de", "es", "fr", "pl"] as const;
export type SupportedLanguage = (typeof SUPPORTED_LANGUAGES)[number];
export const I18N_NAMESPACES = [
  "common",
  "onboarding",
  "settings",
  "queue",
  "readiness",
] as const;

const detector = new LanguageDetector();
detector.addDetector(settingsDetector);

void i18n
  .use(detector)
  .use(initReactI18next)
  .init({
    resources,
    fallbackLng: "en",
    supportedLngs: [...SUPPORTED_LANGUAGES],
    ns: [...I18N_NAMESPACES],
    defaultNS: "common",
    interpolation: { escapeValue: false },
    detection: {
      order: ["customSettings", "navigator"],
      caches: [],
    },
    returnNull: false,
    // Don't crash builds when a key is genuinely missing during M1; the
    // CI script `check:i18n` (added in M3) is the strict enforcer.
    saveMissing: false,
  });

export default i18n;
