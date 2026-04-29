import i18next from "i18next";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { OnboardingWizard } from "./components/OnboardingWizard";
import { QueuePanel } from "./components/QueuePanel";
import { ReadinessPanel } from "./components/ReadinessPanel";
import { SettingsPanel } from "./components/SettingsPanel";
import { SUPPORTED_LANGUAGES, type SupportedLanguage } from "./i18n";
import {
  checkDependencies,
  defaultDocumentsDir,
  loadSettings,
  saveSettings,
} from "./lib/invokeSafe";
import {
  defaultAppSettings,
  type AppSettings,
  type DependencyReport,
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
  const [settings, setSettings] = useState<AppSettings>(defaultAppSettings);
  const [deps, setDeps] = useState<DependencyReport | null>(null);
  const [settingsHydrated, setSettingsHydrated] = useState(false);
  const [activeTab, setActiveTab] = useState<"queue" | "settings">("queue");
  const [wizardOpen, setWizardOpen] = useState(false);
  const [saving, setSaving] = useState(false);
  const [toast, setToast] = useState<string | null>(null);
  const [documentsPath, setDocumentsPath] = useState<string | null>(null);
  const settingsRef = useRef(settings);
  settingsRef.current = settings;

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

  async function handleSave() {
    setSaving(true);
    setToast(null);
    const ok = await saveSettings(settings);
    setSaving(false);
    setToast(ok ? "Settings saved." : "Could not save settings.");
    if (ok) await refreshDeps(settings);
  }

  async function persistSettings(next: AppSettings) {
    setSettings(next);
    setSaving(true);
    setToast(null);
    const ok = await saveSettings(next);
    setSaving(false);
    setToast(ok ? "Settings saved." : "Could not save settings.");
    if (ok) await refreshDeps(next);
  }

  async function finishOnboarding() {
    const next = { ...settings, onboardingCompleted: true };
    setSettings(next);
    setSaving(true);
    setToast(null);
    const ok = await saveSettings(next);
    setSaving(false);
    setToast(ok ? "Settings saved." : "Could not save settings.");
    if (ok) await refreshDeps(next);
    setWizardOpen(false);
  }

  const openSettingsTab = useCallback(() => {
    setActiveTab("settings");
    setWizardOpen(false);
  }, []);

  return (
    <div className="app-root">
      <header className="app-header">
        <h1>Video to Text</h1>
        <p className="tagline">v2t — portable video / audio → text</p>
        <div className="app-header-actions">
          <select
            className="app-lang-select"
            data-testid="header-language-switcher"
            aria-label="UI language"
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
            onClick={() => setWizardOpen(true)}
            title="Short setup walkthrough"
            aria-label="Open setup guide"
          >
            Setup guide
          </button>
        </div>
      </header>

      <div className="app-tabs" role="tablist" aria-label="Main sections">
        <button
          type="button"
          role="tab"
          className={activeTab === "queue" ? "app-tab app-tab--active" : "app-tab"}
          aria-selected={activeTab === "queue"}
          id="tab-queue"
          onClick={() => setActiveTab("queue")}
        >
          Queue
        </button>
        <button
          type="button"
          role="tab"
          className={
            activeTab === "settings" ? "app-tab app-tab--active" : "app-tab"
          }
          aria-selected={activeTab === "settings"}
          id="tab-settings"
          onClick={() => setActiveTab("settings")}
        >
          Settings
        </button>
      </div>

      <ReadinessPanel
        report={deps}
        documentsPath={documentsPath}
        settings={{
          outputDir: settings.outputDir,
          apiKey: settings.apiKey,
          transcriptionMode: settings.transcriptionMode,
          whisperCliPath: settings.whisperCliPath,
        }}
        onOpenSettings={openSettingsTab}
      />

      {toast ? (
        <div className="toast" role="status" aria-live="polite">
          {toast}
        </div>
      ) : null}

      {/* Both panels stay mounted so QueuePanel's local state (jobs, log,
          subtask progress) and Tauri event listeners survive a tab switch.
          Backend work runs in Rust and would continue regardless, but the
          UI used to lose visibility of it on every navigation. */}
      <main
        className="main-workspace"
        role="tabpanel"
        aria-labelledby="tab-queue"
        hidden={activeTab !== "queue"}
      >
        <QueuePanel settings={settings} readinessComplete={readinessComplete} />
      </main>

      <div
        role="tabpanel"
        aria-labelledby="tab-settings"
        hidden={activeTab !== "settings"}
      >
        <SettingsPanel
          settings={settings}
          onChange={setSettings}
          onSave={() => void handleSave()}
          onPersistSettings={(s) => persistSettings(s)}
          onRefreshReadiness={() => void refreshDeps(settingsRef.current)}
          onLanguageChange={(lang) => void handleLanguageChange(lang)}
          saving={saving}
        />
      </div>

      <OnboardingWizard
        open={wizardOpen}
        settings={settings}
        documentsPath={documentsPath}
        patchSettings={(partial) => setSettings((s) => ({ ...s, ...partial }))}
        persistSettings={(next) => persistSettings(next)}
        refreshReadiness={() => void refreshDeps(settingsRef.current)}
        onOpenSettings={openSettingsTab}
        onFinish={() => void finishOnboarding()}
        onClose={() => setWizardOpen(false)}
      />
    </div>
  );
}
