import i18next from "i18next";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { OnboardingWizard } from "./components/OnboardingWizard";
import { PreferencesSheet } from "./components/PreferencesSheet";
import { QueuePanel } from "./components/QueuePanel";
import { ReadinessPanel } from "./components/ReadinessPanel";
import { SUPPORTED_LANGUAGES, type SupportedLanguage } from "./i18n";
import {
  checkDependencies,
  defaultDocumentsDir,
  getApiServerInfo,
  getAppVersion,
  loadSettings,
  saveSettings,
  validateGeminiModel,
  type ApiServerInfo,
} from "./lib/invokeSafe";
import type { PrefsDepth, PrefsFocus, PrefsTarget } from "./types/preferences";
import {
  defaultAppSettings,
  type AppSettings,
  type DependencyReport,
  type ModelValidationResult,
  type UiLanguage,
} from "./types/settings";
import "./App.css";

/** Resolve `uiLanguage = "auto"` to a concrete supported locale by reading
 * the OS / browser locale and stripping the region suffix (e.g. `uk-UA` →
 * `uk`). Falls back to English when the OS locale is not in our catalog. */
function resolveAutoLanguage(): SupportedLanguage {
  const raw =
    typeof navigator !== "undefined" && typeof navigator.language === "string"
      ? navigator.language
      : "en";
  const short = (raw.split("-")[0] ?? "en").toLowerCase();
  return (SUPPORTED_LANGUAGES as readonly string[]).includes(short)
    ? (short as SupportedLanguage)
    : "en";
}

/** Compact header switcher options — flag + ISO code keeps width ~80px. */
const HEADER_LANG_OPTIONS: ReadonlyArray<{ value: UiLanguage; label: string }> = [
  { value: "auto", label: "🌐 Auto" },
  { value: "en", label: "🇬🇧 EN" },
  { value: "uk", label: "🇺🇦 UK" },
  { value: "ru", label: "🇷🇺 RU" },
  { value: "de", label: "🇩🇪 DE" },
  { value: "es", label: "🇪🇸 ES" },
  { value: "fr", label: "🇫🇷 FR" },
  { value: "pl", label: "🇵🇱 PL" },
  { value: "pt", label: "🇵🇹 PT" },
];

export default function App() {
  const { t } = useTranslation("common");
  const [settings, setSettings] = useState<AppSettings>(defaultAppSettings);
  const [prefsDraft, setPrefsDraft] = useState<AppSettings>(defaultAppSettings);
  const [deps, setDeps] = useState<DependencyReport | null>(null);
  const [settingsHydrated, setSettingsHydrated] = useState(false);
  const [prefsOpen, setPrefsOpen] = useState(false);
  const [prefsDepth, setPrefsDepth] = useState<PrefsDepth>("essentials");
  const [prefsFocus, setPrefsFocus] = useState<PrefsFocus>(null);
  const [wizardOpen, setWizardOpen] = useState(false);
  const [saving, setSaving] = useState(false);
  const [toast, setToast] = useState<string | null>(null);
  const [documentsPath, setDocumentsPath] = useState<string | null>(null);
  const [appVersion, setAppVersion] = useState<string | null>(null);
  const [apiInfo, setApiInfo] = useState<ApiServerInfo | null>(null);
  const settingsRef = useRef(settings);
  settingsRef.current = settings;
  const prefsDirty = useMemo(
    () => JSON.stringify(prefsDraft) !== JSON.stringify(settings),
    [prefsDraft, settings],
  );

  const refreshDeps = useCallback(async (s: AppSettings) => {
    const r = await checkDependencies({
      ffmpegPath: s.ffmpegPath,
      ytDlpPath: s.ytDlpPath,
      whisperCliPath: s.whisperCliPath,
      transcriptionMode: s.transcriptionMode,
      whisperModel: s.whisperModel,
      whisperModelsDir: s.whisperModelsDir,
    });
    setDeps(r);
  }, []);

  useEffect(() => {
    void (async () => {
      const loaded = await loadSettings();
      const docDir = await defaultDocumentsDir();
      if (docDir?.trim()) setDocumentsPath(docDir);
      const next: AppSettings = {
        ...defaultAppSettings,
        ...(loaded ?? {}),
      };
      if (
        !next.outputDir?.trim() &&
        docDir?.trim() &&
        next.onboardingCompleted === false
      ) {
        next.outputDir = docDir;
      }
      setSettings(next);
      setPrefsDraft(next);
      await refreshDeps(next);
      setSettingsHydrated(true);
    })();
  }, [refreshDeps]);

  useEffect(() => {
    if (!settingsHydrated) return;
    if (!settings.onboardingCompleted) {
      setWizardOpen(true);
    }
  }, [settingsHydrated, settings.onboardingCompleted]);

  // Gemini model validation at startup
  useEffect(() => {
    if (!settingsHydrated) return;
    if (settings.visionMode !== "gemini") return;
    if (!settings.geminiApiKey) return;

    validateGeminiModel(settings.geminiApiKey, settings.geminiModel).then(
      (result: ModelValidationResult | null) => {
        if (!result) return;
        if (!result.isValid && result.suggestedReplacement) {
          const oldModel = settings.geminiModel;
          const newModel = result.suggestedReplacement;
          const updated = { ...settingsRef.current, geminiModel: newModel };
          setSettings(updated);
          saveSettings(updated);
          setToast(
            `Gemini model "${oldModel}" is deprecated → switched to "${newModel}". Check Settings → Vision.`,
          );
        }
      },
    );
    // Run once after hydration; model key won't change during session
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [settingsHydrated]);

  // App version (from the bundle) + REST API status for the header badge.
  // Both are best-effort: outside the Tauri runtime they stay null/hidden.
  useEffect(() => {
    void (async () => {
      setAppVersion(await getAppVersion());
      setApiInfo(await getApiServerInfo());
    })();
  }, [settingsHydrated, settings.apiServer]);

  const readinessComplete = useMemo(() => {
    if (!deps) return false;
    const tools = deps.ffmpegFound && deps.ytDlpFound;
    const out = Boolean(settings.outputDir?.trim());
    if (!tools || !out) return false;
    if (settings.transcriptionMode === "httpApi") {
      return Boolean(settings.apiKey?.trim());
    }
    if (settings.transcriptionMode === "browserWhisper") {
      return true;
    }
    return deps.whisperCliFound && deps.whisperModelReady;
  }, [deps, settings.outputDir, settings.apiKey, settings.transcriptionMode]);

  // Bridge `settings.uiLanguage` → i18next. Runs after settings hydrate
  // (so the very first render already uses the persisted language) and on
  // any subsequent change from either switcher.
  useEffect(() => {
    if (!settingsHydrated) return;
    const lang = settings.uiLanguage;
    globalThis.__v2tUiLanguage = lang === "auto" ? undefined : lang;
    const target = lang === "auto" ? resolveAutoLanguage() : lang;
    if (i18next.language !== target) {
      void i18next.changeLanguage(target);
    }
  }, [settingsHydrated, settings.uiLanguage]);

  // Persist immediately when language changes — both switchers (header +
  // Settings) call this. Language is a meta-setting, sticky-on-click feels
  // right; user shouldn't have to press the Settings panel's Save button.
  const handleLanguageChange = useCallback(
    async (next: UiLanguage) => {
      await persistSettings({ ...settingsRef.current, uiLanguage: next });
    },
    [],
  );

  async function handleSave(): Promise<boolean> {
    setSaving(true);
    setToast(null);
    const ok = await saveSettings(prefsDraft);
    setSaving(false);
    setToast(ok ? t("toast.saved") : t("toast.save_failed"));
    if (ok) {
      settingsRef.current = prefsDraft;
      setSettings(prefsDraft);
      await refreshDeps(prefsDraft);
    }
    return ok;
  }

  async function persistSettings(next: AppSettings) {
    settingsRef.current = next;
    setSettings(next);
    setSaving(true);
    setToast(null);
    const ok = await saveSettings(next);
    setSaving(false);
    setToast(ok ? t("toast.saved") : t("toast.save_failed"));
    if (ok) await refreshDeps(next);
  }

  async function finishOnboarding() {
    const next = { ...settings, onboardingCompleted: true };
    setSettings(next);
    setSaving(true);
    setToast(null);
    const ok = await saveSettings(next);
    setSaving(false);
    setToast(ok ? t("toast.saved") : t("toast.save_failed"));
    if (ok) await refreshDeps(next);
    setWizardOpen(false);
  }

  const openPreferences = useCallback((target?: PrefsTarget) => {
    setPrefsDraft(settingsRef.current);
    setPrefsDepth(target?.depth ?? "essentials");
    setPrefsFocus(target?.focus ?? null);
    setPrefsOpen(true);
    setWizardOpen(false);
  }, []);

  const closePreferences = useCallback(() => {
    setPrefsOpen(false);
    setPrefsFocus(null);
  }, []);

  return (
    <div className="app-root">
      <header className="app-header">
        <div className="app-brand">
          <span className="brand-wave" aria-hidden>
            <i /><i /><i /><i /><i />
          </span>
          <h1>Video to Text</h1>
        </div>
        <div className="app-header-actions">
          {appVersion ? (
            <span className="app-version-badge" data-testid="app-version-badge">
              <span className="app-version-badge__ver" data-testid="app-version">
                v{appVersion}
              </span>
              {apiInfo ? (
                <span
                  className={
                    apiInfo.running
                      ? "app-version-badge__api app-version-badge__api--on"
                      : "app-version-badge__api"
                  }
                  data-testid="api-status"
                  title={
                    apiInfo.running
                      ? `${apiInfo.baseUrl}/v1/docs`
                      : "REST API disabled — enable apiServer in settings.json"
                  }
                >
                  {apiInfo.running ? `API :${apiInfo.port}` : "API off"}
                </span>
              ) : null}
            </span>
          ) : null}
          <select
            className="app-lang-select"
            data-testid="header-language-switcher"
            aria-label={t("header.lang_aria")}
            value={settings.uiLanguage}
            onChange={(e) => void handleLanguageChange(e.target.value as UiLanguage)}
          >
            {HEADER_LANG_OPTIONS.map((opt) => (
              <option key={opt.value} value={opt.value}>
                {opt.label}
              </option>
            ))}
          </select>
          <button
            type="button"
            className="ghost"
            data-testid="open-preferences-header"
            onClick={() => openPreferences()}
          >
            {t("preferences.open")}
          </button>
          <button
            type="button"
            className="ghost"
            onClick={() => setWizardOpen(true)}
            title={t("header.setup_guide_title")}
            aria-label={t("header.setup_guide_aria")}
          >
            {t("header.setup_guide_label")}
          </button>
        </div>
      </header>

      <ReadinessPanel
        report={deps}
        documentsPath={documentsPath}
        settings={{
          outputDir: settings.outputDir,
          apiKey: settings.apiKey,
          transcriptionMode: settings.transcriptionMode,
          whisperCliPath: settings.whisperCliPath,
        }}
        onOpenPreferences={openPreferences}
      />

      {toast ? (
        <div className="toast" role="status" aria-live="polite">
          {toast}
        </div>
      ) : null}

      {/* QueuePanel stays mounted under the Preferences sheet so jobs, log,
          and Tauri listeners survive opening Preferences. */}
      <main className="main-workspace">
        <QueuePanel
          settings={settings}
          readinessComplete={readinessComplete}
          onOpenOutputSettings={() =>
            openPreferences({ depth: "essentials", focus: "output-dir" })
          }
        />
      </main>

      <PreferencesSheet
        open={prefsOpen}
        depth={prefsDepth}
        focus={prefsFocus}
        onDepthChange={(d) => {
          setPrefsDepth(d);
          setPrefsFocus(null);
        }}
        onClose={closePreferences}
        onDiscard={() => setPrefsDraft(settings)}
        dirty={prefsDirty}
        settings={prefsDraft}
        onChange={setPrefsDraft}
        onSave={handleSave}
        onPersistSettings={async (s) => {
          setPrefsDraft(s);
          await persistSettings(s);
        }}
        onRefreshReadiness={() => void refreshDeps(prefsDraft)}
        onLanguageChange={(lang) =>
          setPrefsDraft((current) => ({ ...current, uiLanguage: lang }))
        }
        saving={saving}
      />

      <OnboardingWizard
        open={wizardOpen}
        settings={settings}
        documentsPath={documentsPath}
        patchSettings={(partial) => setSettings((s) => ({ ...s, ...partial }))}
        persistSettings={(next) => persistSettings(next)}
        refreshReadiness={() => void refreshDeps(settingsRef.current)}
        onOpenSettings={() => openPreferences()}
        onFinish={() => void finishOnboarding()}
        onClose={() => setWizardOpen(false)}
      />
    </div>
  );
}
