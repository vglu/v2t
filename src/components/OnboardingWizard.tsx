import { open } from "@tauri-apps/plugin-dialog";
import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { Trans, useTranslation } from "react-i18next";
import {
  defaultDocumentsDir,
  defaultWhisperModelsDir,
  detectGpu,
  downloadMediaTools,
  downloadWhisperCli,
  downloadWhisperModel,
  installDeno,
  listWhisperModels,
} from "../lib/invokeSafe";
import { isProbablyLinux, isProbablyMac, isProbablyWindows } from "../lib/platform";
import type { AppSettings, CookiesFromBrowser, GpuInfo, TranscriptionMode, WhisperModelMeta } from "../types/settings";

const TOTAL_STEPS = 6;

type ModeChoice = "cloud" | "local" | "browser" | "later";

type Props = {
  open: boolean;
  settings: AppSettings;
  documentsPath: string | null;
  patchSettings: (partial: Partial<AppSettings>) => void;
  persistSettings: (next: AppSettings) => Promise<void>;
  /** After model download — refresh checklist (ggml row). */
  refreshReadiness: () => void;
  onOpenSettings: () => void;
  onFinish: () => void | Promise<void>;
  /** Close overlay only (wizard may show again on next launch). */
  onClose: () => void;
};

function stepTitleKey(step: number, modeChoice: ModeChoice): string {
  switch (step) {
    case 0:
      return "step_title.welcome";
    case 1:
      return "step_title.output";
    case 2:
      return "step_title.tools";
    case 3:
      return "step_title.transcription";
    case 4:
      if (modeChoice === "cloud") return "step_title.cloud";
      if (modeChoice === "local") return "step_title.local";
      if (modeChoice === "browser") return "step_title.browser";
      return "step_title.later";
    case 5:
      return "step_title.run";
    default:
      return "";
  }
}

function pathsProbablyEqual(a: string | null | undefined, b: string | null | undefined): boolean {
  const x = a?.trim();
  const y = b?.trim();
  if (!x || !y) return false;
  return x.replace(/\\/g, "/").toLowerCase() === y.replace(/\\/g, "/").toLowerCase();
}

function WizardSuccessBanner({ children }: { children: ReactNode }) {
  return (
    <div className="onboarding-success-callout" role="status" aria-live="polite">
      <span className="onboarding-check-circle" aria-hidden>
        ✓
      </span>
      <div className="onboarding-success-callout-text">{children}</div>
    </div>
  );
}

function WizardErrorBanner({ children }: { children: ReactNode }) {
  return (
    <div className="onboarding-error-callout" role="alert">
      <span className="onboarding-error-icon" aria-hidden>
        !
      </span>
      <div>{children}</div>
    </div>
  );
}

export function OnboardingWizard({
  open: wizardOpen,
  settings,
  documentsPath,
  patchSettings,
  persistSettings,
  refreshReadiness,
  onOpenSettings,
  onFinish,
  onClose,
}: Props) {
  const { t } = useTranslation("onboarding");
  const [step, setStep] = useState(0);
  const [busy, setBusy] = useState(false);
  const [modeChoice, setModeChoice] = useState<ModeChoice>("cloud");
  const [cloudError, setCloudError] = useState<string | null>(null);
  const [outputError, setOutputError] = useState<string | null>(null);

  const [toolDlBusy, setToolDlBusy] = useState(false);
  const [toolDlMsg, setToolDlMsg] = useState<string | null>(null);
  const [toolDlError, setToolDlError] = useState<string | null>(null);
  const [toolsInstallSuccess, setToolsInstallSuccess] = useState(false);
  const [toolDlProgress, setToolDlProgress] = useState<{
    received: number;
    total: number | null;
  } | null>(null);

  const [whisperModels, setWhisperModels] = useState<WhisperModelMeta[]>([]);
  const [defaultModelsPath, setDefaultModelsPath] = useState<string | null>(null);
  const [modelDlBusy, setModelDlBusy] = useState(false);
  const [modelDlMsg, setModelDlMsg] = useState<string | null>(null);
  const [modelDlError, setModelDlError] = useState<string | null>(null);
  const [modelInstallSuccess, setModelInstallSuccess] = useState(false);
  const [modelDlProgress, setModelDlProgress] = useState<{
    received: number;
    total: number | null;
  } | null>(null);

  const [whisperCliBusy, setWhisperCliBusy] = useState(false);
  const [whisperCliError, setWhisperCliError] = useState<string | null>(null);
  const [whisperCliSuccess, setWhisperCliSuccess] = useState(false);
  const [whisperCliLineMsg, setWhisperCliLineMsg] = useState<string | null>(null);
  const [whisperCliProgress, setWhisperCliProgress] = useState<{
    received: number;
    total: number | null;
  } | null>(null);

  const [denoDlBusy, setDenoDlBusy] = useState(false);
  const [denoDlMsg, setDenoDlMsg] = useState<string | null>(null);
  const [denoDlError, setDenoDlError] = useState<string | null>(null);
  const [denoInstallSuccess, setDenoInstallSuccess] = useState(false);
  const [denoDlProgress, setDenoDlProgress] = useState<{
    received: number;
    total: number | null;
  } | null>(null);

  const prevOpen = useRef(false);
  const isWin = useMemo(() => isProbablyWindows(), []);
  const isMac = useMemo(() => isProbablyMac(), []);
  const isLinux = useMemo(() => isProbablyLinux(), []);
  const showManagedToolDownloads = isWin || isMac;

  const [gpuInfo, setGpuInfo] = useState<GpuInfo | null>(null);
  useEffect(() => {
    if (!wizardOpen || !isWin) return;
    void detectGpu().then(setGpuInfo);
  }, [wizardOpen, isWin]);

  useEffect(() => {
    if (wizardOpen && !prevOpen.current) {
      setStep(0);
      setModeChoice(
        settings.transcriptionMode === "localWhisper"
          ? "local"
          : settings.transcriptionMode === "browserWhisper"
            ? "browser"
            : "cloud",
      );
      setCloudError(null);
      setOutputError(null);
      setToolDlMsg(null);
      setToolDlError(null);
      setToolsInstallSuccess(false);
      setModelDlMsg(null);
      setModelDlError(null);
      setModelInstallSuccess(false);
      setWhisperCliError(null);
      setWhisperCliSuccess(false);
      setWhisperCliLineMsg(null);
      setDenoDlMsg(null);
      setDenoDlError(null);
      setDenoInstallSuccess(false);
    }
    prevOpen.current = wizardOpen;
  }, [wizardOpen, settings.transcriptionMode]);

  useEffect(() => {
    if (!wizardOpen) return;
    void listWhisperModels().then((m) => {
      if (m?.length) setWhisperModels(m);
    });
    void defaultWhisperModelsDir().then(setDefaultModelsPath);
  }, [wizardOpen]);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    const ac = new AbortController();
    void import("@tauri-apps/api/event")
      .then(({ listen }) =>
        listen<{
          tool: string;
          phase: string;
          message: string;
          bytesReceived: number;
          totalBytes: number | null;
        }>("tool-download-progress", (ev) => {
          if (ac.signal.aborted) return;
          const prog = {
            received: ev.payload.bytesReceived,
            total: ev.payload.totalBytes,
          };
          const line = `[${ev.payload.tool}] ${ev.payload.message}`;
          if (ev.payload.tool === "whisper-cli") {
            setWhisperCliLineMsg(line);
            setWhisperCliProgress(prog);
          } else if (ev.payload.tool === "deno") {
            setDenoDlMsg(line);
            setDenoDlProgress(prog);
          } else {
            setToolDlMsg(line);
            setToolDlProgress(prog);
          }
        }),
      )
      .then((fn) => {
        if (!ac.signal.aborted) unlisten = fn;
      })
      .catch(() => {});
    return () => {
      ac.abort();
      unlisten?.();
    };
  }, []);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    const ac = new AbortController();
    void import("@tauri-apps/api/event")
      .then(({ listen }) =>
        listen<{
          modelId: string;
          phase: string;
          message: string;
          bytesReceived: number;
          totalBytes: number | null;
        }>("model-download-progress", (ev) => {
          if (ac.signal.aborted) return;
          setModelDlMsg(`[${ev.payload.phase}] ${ev.payload.message}`);
          setModelDlProgress({
            received: ev.payload.bytesReceived,
            total: ev.payload.totalBytes,
          });
        }),
      )
      .then((fn) => {
        if (!ac.signal.aborted) unlisten = fn;
      })
      .catch(() => {});
    return () => {
      ac.abort();
      unlisten?.();
    };
  }, []);

  if (!wizardOpen) return null;

  const last = step >= TOTAL_STEPS - 1;

  async function handleFinish() {
    setBusy(true);
    try {
      await onFinish();
    } finally {
      setBusy(false);
    }
  }

  async function useDocumentsAsOutput() {
    const doc = await defaultDocumentsDir();
    if (!doc?.trim()) return;
    const next = { ...settings, outputDir: doc };
    await persistSettings(next);
  }

  async function pickOutputFolder() {
    const dir = await open({ directory: true, multiple: false });
    if (typeof dir === "string" && dir.length > 0) {
      patchSettings({ outputDir: dir });
    }
  }

  async function onDownloadMediaTools() {
    setToolDlBusy(true);
    setToolDlMsg(null);
    setToolDlError(null);
    setToolsInstallSuccess(false);
    setToolDlProgress(null);
    try {
      const p = await downloadMediaTools();
      const next = { ...settings, ffmpegPath: p.ffmpegPath, ytDlpPath: p.ytDlpPath };
      await persistSettings(next);
      setToolsInstallSuccess(true);
      setToolDlMsg(null);
    } catch (e) {
      const msg =
        typeof e === "string"
          ? e
          : e instanceof Error
            ? e.message
            : "Download failed";
      setToolDlError(msg);
    } finally {
      setToolDlBusy(false);
      setToolDlProgress(null);
    }
  }

  async function onDownloadModel() {
    setModelDlBusy(true);
    setModelDlMsg(null);
    setModelDlError(null);
    setModelInstallSuccess(false);
    setModelDlProgress(null);
    try {
      await downloadWhisperModel(settings.whisperModel, settings.whisperModelsDir);
      setModelInstallSuccess(true);
      setModelDlMsg(null);
      refreshReadiness();
    } catch (e) {
      const msg =
        typeof e === "string"
          ? e
          : e instanceof Error
            ? e.message
            : "Download failed";
      setModelDlError(msg);
    } finally {
      setModelDlBusy(false);
      setModelDlProgress(null);
    }
  }

  async function pickWhisperModelsDir() {
    const dir = await open({ directory: true, multiple: false });
    if (typeof dir === "string" && dir.length > 0) {
      patchSettings({ whisperModelsDir: dir });
    }
  }

  async function pickWhisperCliExecutable() {
    const f = await open({ multiple: false });
    if (typeof f === "string" && f.length > 0) {
      patchSettings({ whisperCliPath: f });
      refreshReadiness();
    }
  }

  async function onInstallDeno() {
    setDenoDlBusy(true);
    setDenoDlMsg(null);
    setDenoDlError(null);
    setDenoInstallSuccess(false);
    setDenoDlProgress(null);
    try {
      const res = await installDeno();
      const next = { ...settings, ytDlpJsRuntimes: res.jsRuntimes };
      await persistSettings(next);
      patchSettings({ ytDlpJsRuntimes: res.jsRuntimes });
      setDenoInstallSuccess(true);
    } catch (e) {
      const msg =
        typeof e === "string"
          ? e
          : e instanceof Error
            ? e.message
            : "Deno install failed";
      setDenoDlError(msg);
    } finally {
      setDenoDlBusy(false);
      setDenoDlProgress(null);
    }
  }

  async function onSetupWhisperCli() {
    setWhisperCliBusy(true);
    setWhisperCliError(null);
    setWhisperCliSuccess(false);
    setWhisperCliLineMsg(null);
    setWhisperCliProgress(null);
    try {
      const p = await downloadWhisperCli(isWin ? settings.whisperAcceleration : undefined);
      await persistSettings({ ...settings, whisperCliPath: p.whisperCliPath });
      patchSettings({ whisperCliPath: p.whisperCliPath });
      setWhisperCliSuccess(true);
      refreshReadiness();
    } catch (e) {
      const msg =
        typeof e === "string"
          ? e
          : e instanceof Error
            ? e.message
            : "Setup failed";
      setWhisperCliError(msg);
    } finally {
      setWhisperCliBusy(false);
      setWhisperCliProgress(null);
    }
  }

  async function goNext() {
    if (step === 1) {
      if (!settings.outputDir?.trim()) {
        setOutputError(t("output.error_pick_folder"));
        return;
      }
      setOutputError(null);
      setBusy(true);
      try {
        await persistSettings(settings);
      } finally {
        setBusy(false);
      }
      setStep(2);
      return;
    }

    if (step === 3) {
      const tm: TranscriptionMode =
        modeChoice === "local"
          ? "localWhisper"
          : modeChoice === "browser"
            ? "browserWhisper"
            : modeChoice === "cloud"
              ? "httpApi"
              : settings.transcriptionMode;
      const next =
        modeChoice === "later" ? { ...settings } : { ...settings, transcriptionMode: tm };
      setBusy(true);
      try {
        await persistSettings(next);
      } finally {
        setBusy(false);
      }
      setStep(4);
      return;
    }

    if (step === 4 && modeChoice === "cloud") {
      if (!settings.apiKey?.trim()) {
        setCloudError(t("cloud_step.error_no_key"));
        return;
      }
      setCloudError(null);
      setBusy(true);
      try {
        await persistSettings({ ...settings, transcriptionMode: "httpApi" });
      } finally {
        setBusy(false);
      }
      setStep(5);
      return;
    }

    if (step === 4 && modeChoice === "local") {
      setBusy(true);
      try {
        await persistSettings({ ...settings, transcriptionMode: "localWhisper" });
      } finally {
        setBusy(false);
      }
      setStep(5);
      return;
    }

    if (step === 4 && modeChoice === "browser") {
      setBusy(true);
      try {
        await persistSettings({ ...settings, transcriptionMode: "browserWhisper" });
      } finally {
        setBusy(false);
      }
      setStep(5);
      return;
    }

    if (step === 4 && modeChoice === "later") {
      setStep(5);
      return;
    }

    setStep((s) => s + 1);
  }

  function goBack() {
    setCloudError(null);
    setOutputError(null);
    setStep((s) => Math.max(0, s - 1));
  }

  const titleKey = stepTitleKey(step, modeChoice);
  // `t` types are inferred from the typed CustomTypeOptions augmentation; the
  // dynamic key needs a cast to satisfy the literal-union signature.
  const title = titleKey ? (t as (k: string) => string)(titleKey) : "";

  const body = (() => {
    switch (step) {
      case 0:
        return (
          <>
            <p>
              <Trans i18nKey="welcome.intro" t={t} components={{ strong: <strong /> }} />
            </p>
            <p className="onboarding-tip">{t("checklist_tip")}</p>
          </>
        );
      case 1:
        return (
          <>
            <p>
              <Trans i18nKey="output.intro" t={t} components={{ strong: <strong /> }} />
            </p>
            <label className="field onboarding-field">
              <span>{t("output.current_folder_label")}</span>
              <input
                type="text"
                readOnly
                value={settings.outputDir ?? ""}
                placeholder={t("output.not_set_placeholder")}
              />
            </label>
            {outputError ? <p className="onboarding-error">{outputError}</p> : null}
            <div className="onboarding-row">
              <button type="button" disabled={busy} onClick={() => void useDocumentsAsOutput()}>
                {t("output.use_documents")}
              </button>
              <button type="button" disabled={busy} onClick={() => void pickOutputFolder()}>
                {t("output.browse")}
              </button>
            </div>
            {pathsProbablyEqual(settings.outputDir, documentsPath) ? (
              <p className="onboarding-tip onboarding-success-hint">
                {t("output.tip_documents_set")}
              </p>
            ) : settings.outputDir?.trim() ? (
              <p className="onboarding-tip">{t("output.tip_custom_set")}</p>
            ) : (
              <p className="onboarding-tip">{t("output.tip_use_documents")}</p>
            )}
          </>
        );
      case 2:
        return (
          <>
            <p>
              <Trans i18nKey="tools.intro" t={t} components={{ strong: <strong /> }} />
            </p>
            {showManagedToolDownloads ? (
              <div className="onboarding-block">
                <button
                  type="button"
                  className="primary"
                  disabled={toolDlBusy || busy}
                  onClick={() => void onDownloadMediaTools()}
                >
                  {toolDlBusy ? t("tools.btn_installing") : t("tools.btn_install")}
                </button>
                {toolDlProgress && toolDlProgress.total != null && toolDlProgress.total > 0 ? (
                  <div className="download-progress-wrap onboarding-progress">
                    <progress value={toolDlProgress.received} max={toolDlProgress.total} />
                  </div>
                ) : null}
                {toolDlMsg && toolDlBusy ? (
                  <p className="hint onboarding-hint">{toolDlMsg}</p>
                ) : null}
                {toolsInstallSuccess ? (
                  <WizardSuccessBanner>
                    <Trans i18nKey="tools.install_success" t={t} components={{ strong: <strong /> }} />
                  </WizardSuccessBanner>
                ) : null}
                {toolDlError ? <WizardErrorBanner>{toolDlError}</WizardErrorBanner> : null}
                <p className="onboarding-tip">
                  <Trans i18nKey="tools.tip_manual" t={t} components={{ strong: <strong /> }} />
                </p>
              </div>
            ) : (
              <p className="onboarding-tip">
                <Trans
                  i18nKey="tools.linux_hint"
                  t={t}
                  components={{ strong: <strong />, code: <code /> }}
                />
              </p>
            )}
            <div className="onboarding-block">
              <label className="field onboarding-field">
                <span>{t("tools.js_runtimes_label")}</span>
                <input
                  type="text"
                  value={settings.ytDlpJsRuntimes ?? ""}
                  onChange={(e) => patchSettings({ ytDlpJsRuntimes: e.target.value.trim() || null })}
                  placeholder={t("tools.js_runtimes_placeholder")}
                  aria-label={t("tools.js_runtimes_aria")}
                />
                <div
                  className="field-lang-examples"
                  role="group"
                  aria-label={t("tools.js_runtimes_aria")}
                >
                  <span className="field-lang-examples-label">{t("tools.common_label")}</span>
                  <button
                    type="button"
                    className="lang-code-chip"
                    onClick={() => patchSettings({ ytDlpJsRuntimes: "deno" })}
                  >
                    deno
                  </button>
                  <button
                    type="button"
                    className="lang-code-chip"
                    onClick={() => patchSettings({ ytDlpJsRuntimes: "nodejs" })}
                  >
                    nodejs
                  </button>
                  <button
                    type="button"
                    className="lang-code-chip"
                    onClick={() => patchSettings({ ytDlpJsRuntimes: "node" })}
                  >
                    node
                  </button>
                </div>
              </label>
              <p className="onboarding-tip">
                <Trans i18nKey="tools.js_runtimes_tip" t={t} components={{ code: <code /> }} />
              </p>
              {showManagedToolDownloads ? (
                <>
                  <button
                    type="button"
                    className="primary"
                    disabled={denoDlBusy || busy}
                    onClick={() => void onInstallDeno()}
                  >
                    {denoDlBusy ? t("tools.btn_install_deno_busy") : t("tools.btn_install_deno")}
                  </button>
                  {denoDlProgress && denoDlProgress.total != null && denoDlProgress.total > 0 ? (
                    <div className="download-progress-wrap onboarding-progress">
                      <progress value={denoDlProgress.received} max={denoDlProgress.total} />
                    </div>
                  ) : null}
                  {denoDlMsg && denoDlBusy ? (
                    <p className="hint onboarding-hint">{denoDlMsg}</p>
                  ) : null}
                  {denoInstallSuccess ? (
                    <WizardSuccessBanner>
                      <Trans
                        i18nKey="tools.deno_success"
                        t={t}
                        components={{ strong: <strong />, code: <code /> }}
                      />
                    </WizardSuccessBanner>
                  ) : null}
                  {denoDlError ? <WizardErrorBanner>{denoDlError}</WizardErrorBanner> : null}
                </>
              ) : null}
            </div>
            <div className="onboarding-block">
              <label className="field onboarding-field">
                <span>{t("tools.cookies_label")}</span>
                <select
                  aria-label={t("tools.cookies_aria")}
                  value={settings.cookiesFromBrowser}
                  onChange={(e) => patchSettings({ cookiesFromBrowser: e.target.value as CookiesFromBrowser })}
                >
                  <option value="auto">{t("tools.cookies_options.auto")}</option>
                  <option value="chrome">{t("tools.cookies_options.chrome")}</option>
                  <option value="brave">{t("tools.cookies_options.brave")}</option>
                  <option value="edge">{t("tools.cookies_options.edge")}</option>
                  <option value="firefox">{t("tools.cookies_options.firefox")}</option>
                  <option value="none">{t("tools.cookies_options.none")}</option>
                </select>
              </label>
              <p className="onboarding-tip">
                <Trans i18nKey="tools.cookies_tip" t={t} components={{ strong: <strong /> }} />
              </p>
            </div>
          </>
        );
      case 3:
        return (
          <>
            <p>{t("mode.intro")}</p>
            <div className="onboarding-radio-group" role="radiogroup" aria-label={t("mode.group_aria")}>
              <label className="onboarding-radio">
                <input
                  type="radio"
                  name="wiz-transcription"
                  checked={modeChoice === "cloud"}
                  onChange={() => setModeChoice("cloud")}
                />
                <span>
                  <Trans
                    i18nKey="mode.cloud"
                    t={t}
                    components={{ strong: <strong />, code: <code /> }}
                  />
                </span>
              </label>
              <label className="onboarding-radio">
                <input
                  type="radio"
                  name="wiz-transcription"
                  checked={modeChoice === "local"}
                  onChange={() => setModeChoice("local")}
                />
                <span>
                  <Trans
                    i18nKey="mode.local"
                    t={t}
                    components={{ strong: <strong />, code: <code /> }}
                  />
                </span>
              </label>
              <label className="onboarding-radio">
                <input
                  type="radio"
                  name="wiz-transcription"
                  checked={modeChoice === "browser"}
                  onChange={() => setModeChoice("browser")}
                />
                <span>
                  <Trans
                    i18nKey="mode.browser"
                    t={t}
                    components={{ strong: <strong />, code: <code /> }}
                  />
                </span>
              </label>
              <label className="onboarding-radio">
                <input
                  type="radio"
                  name="wiz-transcription"
                  checked={modeChoice === "later"}
                  onChange={() => setModeChoice("later")}
                />
                <span>
                  <Trans i18nKey="mode.later" t={t} components={{ strong: <strong /> }} />
                </span>
              </label>
            </div>
          </>
        );
      case 4:
        if (modeChoice === "browser") {
          return (
            <>
              <p>
                <Trans i18nKey="browser_step.intro" t={t} components={{ strong: <strong /> }} />
              </p>
              <label className="field onboarding-field">
                <span>{t("browser_step.model_size_label")}</span>
                <select
                  aria-label={t("browser_step.model_aria")}
                  value={settings.whisperModel}
                  onChange={(e) => patchSettings({ whisperModel: e.target.value })}
                >
                  {whisperModels.map((m) => (
                    <option key={m.id} value={m.id}>
                      {t("browser_step.model_option", { id: m.id, sizeMib: m.sizeMib })}
                    </option>
                  ))}
                </select>
              </label>
              <p className="onboarding-tip">
                <Trans i18nKey="browser_step.tip" t={t} components={{ code: <code /> }} />
              </p>
            </>
          );
        }
        if (modeChoice === "cloud") {
          return (
            <>
              <p>{t("cloud_step.intro")}</p>
              <label className="field onboarding-field">
                <span>{t("cloud_step.api_url_label")}</span>
                <input
                  type="url"
                  value={settings.apiBaseUrl}
                  onChange={(e) => patchSettings({ apiBaseUrl: e.target.value })}
                  autoComplete="off"
                />
              </label>
              <label className="field onboarding-field">
                <span>{t("cloud_step.api_model_label")}</span>
                <input
                  type="text"
                  value={settings.apiModel}
                  onChange={(e) => patchSettings({ apiModel: e.target.value })}
                  autoComplete="off"
                />
              </label>
              <label className="field onboarding-field">
                <span>{t("cloud_step.api_key_label")}</span>
                <input
                  type="password"
                  autoComplete="off"
                  value={settings.apiKey}
                  onChange={(e) => patchSettings({ apiKey: e.target.value })}
                />
              </label>
              {cloudError ? <p className="onboarding-error">{cloudError}</p> : null}
              <p className="onboarding-tip">
                <Trans
                  i18nKey="cloud_step.tip_openai"
                  t={t}
                  components={{
                    strong: <strong />,
                    code: <code />,
                    a: (
                      <a
                        href="https://platform.openai.com/api-keys"
                        target="_blank"
                        rel="noopener noreferrer"
                      />
                    ),
                  }}
                />
              </p>
            </>
          );
        }
        if (modeChoice === "local") {
          return (
            <>
              <p className="onboarding-tip onboarding-info-callout">
                <Trans
                  i18nKey="local_step.intro"
                  t={t}
                  components={{
                    strong: <strong />,
                    code: <code />,
                    a: (
                      <a
                        href="https://github.com/ggml-org/whisper.cpp/releases"
                        target="_blank"
                        rel="noopener noreferrer"
                      />
                    ),
                  }}
                />
              </p>

              <div className="onboarding-local-step">
                <p className="onboarding-local-step-title">{t("local_step.step_a_title")}</p>
                <p className="onboarding-tip">
                  <Trans
                    i18nKey="local_step.step_a_tip"
                    t={t}
                    components={{ strong: <strong />, code: <code /> }}
                  />
                </p>
                {isWin && gpuInfo?.kind === "nvidia" ? (
                  <div
                    className="onboarding-tip onboarding-info-callout"
                    data-testid="onboarding-cuda-hint"
                  >
                    <p>
                      <Trans
                        i18nKey="local_step.cuda_intro"
                        t={t}
                        values={{ name: gpuInfo.names[0] ?? t("local_step.cuda_default_name") }}
                        components={{ strong: <strong /> }}
                      />
                    </p>
                    <label className="onboarding-radio">
                      <input
                        type="checkbox"
                        checked={settings.whisperAcceleration !== "cpu"}
                        onChange={(e) =>
                          patchSettings({ whisperAcceleration: e.target.checked ? "auto" : "cpu" })
                        }
                      />
                      <span>{t("local_step.cuda_enable")}</span>
                    </label>
                  </div>
                ) : null}
                {isLinux ? (
                  <div className="onboarding-tip onboarding-info-callout onboarding-linux-whisper-block">
                    <p>
                      <Trans i18nKey="local_step.linux_intro" t={t} components={{ strong: <strong /> }} />
                    </p>
                    <ul>
                      <li>
                        <Trans
                          i18nKey="local_step.linux_li_ubuntu"
                          t={t}
                          components={{
                            code: <code />,
                            a: (
                              <a
                                href="https://github.com/ggml-org/whisper.cpp"
                                target="_blank"
                                rel="noopener noreferrer"
                              />
                            ),
                          }}
                        />
                      </li>
                      <li>
                        <Trans
                          i18nKey="local_step.linux_li_fedora"
                          t={t}
                          components={{ code: <code /> }}
                        />
                      </li>
                      <li>
                        <Trans
                          i18nKey="local_step.linux_li_arch"
                          t={t}
                          components={{ code: <code /> }}
                        />
                      </li>
                    </ul>
                  </div>
                ) : null}
                <div className="row-gap">
                  {showManagedToolDownloads ? (
                    <button
                      type="button"
                      disabled={whisperCliBusy || busy}
                      onClick={() => void onSetupWhisperCli()}
                    >
                      {whisperCliBusy
                        ? t("local_step.btn_working")
                        : isWin
                          ? t("local_step.btn_download_win")
                          : t("local_step.btn_find_mac")}
                    </button>
                  ) : null}
                  <button type="button" disabled={busy} onClick={() => void pickWhisperCliExecutable()}>
                    {t("local_step.btn_pick")}
                  </button>
                </div>
                {whisperCliProgress &&
                whisperCliProgress.total != null &&
                whisperCliProgress.total > 0 ? (
                  <div className="download-progress-wrap onboarding-progress">
                    <progress value={whisperCliProgress.received} max={whisperCliProgress.total} />
                  </div>
                ) : null}
                {whisperCliLineMsg && whisperCliBusy ? (
                  <p className="hint onboarding-hint">{whisperCliLineMsg}</p>
                ) : null}
                {whisperCliSuccess ? (
                  <WizardSuccessBanner>
                    <Trans
                      i18nKey="local_step.cli_success"
                      t={t}
                      components={{ strong: <strong /> }}
                    />
                  </WizardSuccessBanner>
                ) : null}
                {whisperCliError ? <WizardErrorBanner>{whisperCliError}</WizardErrorBanner> : null}
                <label className="field onboarding-field">
                  <span>{t("local_step.cli_path_label")}</span>
                  <div className="row-gap">
                    <input
                      type="text"
                      value={settings.whisperCliPath ?? ""}
                      onChange={(e) =>
                        patchSettings({ whisperCliPath: e.target.value.trim() || null })
                      }
                      placeholder={t("local_step.cli_path_placeholder")}
                      autoComplete="off"
                    />
                  </div>
                </label>
              </div>

              <div className="onboarding-local-step">
                <p className="onboarding-local-step-title">{t("local_step.step_b_title")}</p>
                <p className="onboarding-tip">
                  <Trans
                    i18nKey="local_step.step_b_tip"
                    t={t}
                    components={{ strong: <strong /> }}
                  />
                </p>
                <label className="field onboarding-field">
                  <span>{t("local_step.models_dir_label")}</span>
                  <div className="row-gap">
                    <input
                      type="text"
                      readOnly
                      value={settings.whisperModelsDir ?? ""}
                      placeholder={defaultModelsPath ?? t("local_step.models_dir_default_placeholder")}
                    />
                    <button type="button" disabled={busy} onClick={() => void pickWhisperModelsDir()}>
                      {t("local_step.browse")}
                    </button>
                  </div>
                </label>
                <label className="field onboarding-field">
                  <span>{t("local_step.model_label")}</span>
                  <select
                    aria-label={t("local_step.model_aria")}
                    value={settings.whisperModel}
                    onChange={(e) => {
                      patchSettings({ whisperModel: e.target.value });
                      setModelInstallSuccess(false);
                      setModelDlError(null);
                    }}
                  >
                    {whisperModels.map((m) => (
                      <option key={m.id} value={m.id}>
                        {t("local_step.model_option", {
                          id: m.id,
                          sizeMib: m.sizeMib,
                          fileName: m.fileName,
                        })}
                      </option>
                    ))}
                  </select>
                </label>
                <button type="button" disabled={modelDlBusy || busy} onClick={() => void onDownloadModel()}>
                  {modelDlBusy ? t("local_step.btn_downloading") : t("local_step.btn_download_model")}
                </button>
                {modelDlProgress && modelDlProgress.total != null && modelDlProgress.total > 0 ? (
                  <div className="download-progress-wrap onboarding-progress">
                    <progress value={modelDlProgress.received} max={modelDlProgress.total} />
                  </div>
                ) : null}
                {modelDlMsg && modelDlBusy ? (
                  <p className="hint onboarding-hint">{modelDlMsg}</p>
                ) : null}
                {modelInstallSuccess ? (
                  <WizardSuccessBanner>
                    <Trans
                      i18nKey="local_step.model_success"
                      t={t}
                      components={{ strong: <strong /> }}
                    />
                  </WizardSuccessBanner>
                ) : null}
                {modelDlError ? <WizardErrorBanner>{modelDlError}</WizardErrorBanner> : null}
              </div>
            </>
          );
        }
        return (
          <>
            <p>
              <Trans i18nKey="later_step.intro" t={t} components={{ strong: <strong /> }} />
            </p>
            <button type="button" className="ghost" disabled={busy} onClick={onOpenSettings}>
              {t("later_step.btn_open_settings")}
            </button>
          </>
        );
      case 5:
        return (
          <>
            <p>
              <Trans i18nKey="run_step.intro" t={t} components={{ strong: <strong /> }} />
            </p>
            <p className="onboarding-tip">{t("run_step.tip")}</p>
          </>
        );
      default:
        return null;
    }
  })();

  const nextDisabled =
    busy ||
    (step === 2 && toolDlBusy) ||
    (step === 4 && modeChoice === "local" && (modelDlBusy || whisperCliBusy));

  return (
    <div className="onboarding-backdrop" role="presentation">
      <div
        className="onboarding-modal onboarding-modal-wide"
        role="dialog"
        aria-modal="true"
        aria-labelledby="onboarding-title"
        data-testid="onboarding-wizard"
      >
        <p className="onboarding-step-label">
          {t("step_label", { current: step + 1, total: TOTAL_STEPS })}
        </p>
        <h2 id="onboarding-title" className="onboarding-modal-title">
          {title}
        </h2>
        <div className="onboarding-body">{body}</div>
        <div className="onboarding-actions">
          {step > 0 ? (
            <button
              type="button"
              disabled={busy || toolDlBusy || modelDlBusy || whisperCliBusy}
              onClick={goBack}
            >
              {t("btn.back")}
            </button>
          ) : (
            <span />
          )}
          <div className="onboarding-actions-right">
            <button type="button" className="ghost" disabled={busy} onClick={() => void handleFinish()}>
              {t("btn.skip")}
            </button>
            {step === 5 ? (
              <button type="button" disabled={busy} onClick={onOpenSettings}>
                {t("btn.open_settings")}
              </button>
            ) : null}
            {last ? (
              <button type="button" className="primary" disabled={busy} onClick={() => void handleFinish()}>
                {busy ? t("btn.saving") : t("btn.done")}
              </button>
            ) : (
              <button
                type="button"
                className="primary"
                disabled={nextDisabled}
                onClick={() => void goNext()}
              >
                {t("btn.next")}
              </button>
            )}
          </div>
        </div>
        <button type="button" className="onboarding-close" aria-label={t("btn.close_aria")} disabled={busy} onClick={onClose}>
          ×
        </button>
      </div>
    </div>
  );
}
