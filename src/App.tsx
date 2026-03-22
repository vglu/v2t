import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { OnboardingWizard } from "./components/OnboardingWizard";
import { QueuePanel } from "./components/QueuePanel";
import { ReadinessPanel } from "./components/ReadinessPanel";
import { SettingsPanel } from "./components/SettingsPanel";
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
} from "./types/settings";
import "./App.css";

export default function App() {
  const [settings, setSettings] = useState<AppSettings>(defaultAppSettings);
  const [deps, setDeps] = useState<DependencyReport | null>(null);
  const [settingsHydrated, setSettingsHydrated] = useState(false);
  const [showSettings, setShowSettings] = useState(false);
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
    return deps.whisperCliFound && deps.whisperModelReady;
  }, [
    deps,
    settings.outputDir,
    settings.apiKey,
    settings.transcriptionMode,
  ]);

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

  return (
    <div className="app-root">
      <header className="app-header">
        <h1>Video to Text</h1>
        <p className="tagline">v2t — portable video / audio → text</p>
        <div className="app-header-actions">
          <button
            type="button"
            className="ghost"
            onClick={() => setWizardOpen(true)}
            title="Short setup walkthrough"
          >
            Setup guide
          </button>
          <button
            type="button"
            className="ghost"
            onClick={() => setShowSettings((v) => !v)}
            aria-expanded={showSettings ? "true" : "false"}
          >
            {showSettings ? "Close settings" : "Settings"}
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
        onOpenSettings={() => {
          setShowSettings(true);
          setWizardOpen(false);
        }}
      />

      {toast ? (
        <div className="toast" role="status">
          {toast}
        </div>
      ) : null}

      {showSettings ? (
        <SettingsPanel
          settings={settings}
          onChange={setSettings}
          onSave={() => void handleSave()}
          onPersistSettings={(s) => persistSettings(s)}
          onRefreshReadiness={() => void refreshDeps(settingsRef.current)}
          saving={saving}
        />
      ) : null}

      <main className="main-workspace">
        <QueuePanel settings={settings} readinessComplete={readinessComplete} />
      </main>

      <OnboardingWizard
        open={wizardOpen}
        settings={settings}
        documentsPath={documentsPath}
        patchSettings={(partial) => setSettings((s) => ({ ...s, ...partial }))}
        persistSettings={(next) => persistSettings(next)}
        refreshReadiness={() => void refreshDeps(settingsRef.current)}
        onOpenSettings={() => {
          setShowSettings(true);
          setWizardOpen(false);
        }}
        onFinish={() => void finishOnboarding()}
        onClose={() => setWizardOpen(false)}
      />
    </div>
  );
}
