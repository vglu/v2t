import { open } from "@tauri-apps/plugin-dialog";
import { useEffect, useMemo, useState } from "react";
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
import type {
  AppSettings,
  CookiesFromBrowser,
  GpuInfo,
  TranscriptionMode,
  UiLanguage,
  WhisperAcceleration,
  WhisperModelMeta,
} from "../types/settings";

type Props = {
  settings: AppSettings;
  onChange: (s: AppSettings) => void;
  onSave: () => void;
  /** Save merged settings (e.g. after auto-download paths). */
  onPersistSettings: (s: AppSettings) => Promise<void>;
  /** Re-run dependency check (e.g. after model download). */
  onRefreshReadiness?: () => void;
  /** Persist a UI-language change immediately (also drives i18next.changeLanguage). */
  onLanguageChange: (lang: UiLanguage) => void;
  saving: boolean;
};

/** Full-name labels for the Settings panel switcher. The header switcher uses
 * compact ISO codes — these are the readable-by-natives versions. */
const FULL_LANG_OPTIONS: ReadonlyArray<{ value: UiLanguage; label: string }> = [
  { value: "auto", label: "Auto (system)" },
  { value: "en", label: "English" },
  { value: "uk", label: "Українська" },
  { value: "ru", label: "Русский" },
  { value: "de", label: "Deutsch" },
  { value: "es", label: "Español" },
  { value: "fr", label: "Français" },
  { value: "pl", label: "Polski" },
  { value: "pt", label: "Português" },
];

export function SettingsPanel({
  settings,
  onChange,
  onSave,
  onPersistSettings,
  onRefreshReadiness,
  onLanguageChange,
  saving,
}: Props) {
  const { t } = useTranslation("settings");
  const [whisperModels, setWhisperModels] = useState<WhisperModelMeta[]>([]);
  const [defaultModelsPath, setDefaultModelsPath] = useState<string | null>(null);
  const [modelDlMsg, setModelDlMsg] = useState<string | null>(null);
  const [modelDlBusy, setModelDlBusy] = useState(false);
  const [modelDlProgress, setModelDlProgress] = useState<{
    received: number;
    total: number | null;
  } | null>(null);

  const [toolDlMsg, setToolDlMsg] = useState<string | null>(null);
  const [toolDlBusy, setToolDlBusy] = useState(false);
  const [toolDlProgress, setToolDlProgress] = useState<{
    received: number;
    total: number | null;
  } | null>(null);

  const [whisperCliDlBusy, setWhisperCliDlBusy] = useState(false);
  const [whisperCliDlMsg, setWhisperCliDlMsg] = useState<string | null>(null);
  const [whisperCliDlError, setWhisperCliDlError] = useState<string | null>(null);
  const [whisperCliInstallSuccess, setWhisperCliInstallSuccess] = useState(false);
  const [whisperCliDlProgress, setWhisperCliDlProgress] = useState<{
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

  const isWin = useMemo(() => isProbablyWindows(), []);
  const isMac = useMemo(() => isProbablyMac(), []);
  const isLinux = useMemo(() => isProbablyLinux(), []);
  const showManagedToolDownloads = isWin || isMac;

  const [gpuInfo, setGpuInfo] = useState<GpuInfo | null>(null);
  useEffect(() => {
    if (!isWin) return;
    void detectGpu().then(setGpuInfo);
  }, [isWin]);

  useEffect(() => {
    void listWhisperModels().then((m) => {
      if (m) setWhisperModels(m);
    });
    void defaultWhisperModelsDir().then(setDefaultModelsPath);
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
      .catch(() => {
        /* web / tests */
      });
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
            setWhisperCliDlMsg(line);
            setWhisperCliDlProgress(prog);
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

  async function pickOutputDir() {
    const dir = await open({ directory: true, multiple: false });
    if (typeof dir === "string" && dir.length > 0) {
      onChange({ ...settings, outputDir: dir });
    }
  }

  async function useDocumentsAsOutput() {
    const doc = await defaultDocumentsDir();
    if (!doc?.trim()) return;
    await onPersistSettings({ ...settings, outputDir: doc });
  }

  async function pickWhisperModelsDir() {
    const dir = await open({ directory: true, multiple: false });
    if (typeof dir === "string" && dir.length > 0) {
      onChange({ ...settings, whisperModelsDir: dir });
    }
  }

  async function pickWhisperCliExecutable() {
    const f = await open({ multiple: false });
    if (typeof f === "string" && f.length > 0) {
      onChange({ ...settings, whisperCliPath: f });
      onRefreshReadiness?.();
    }
  }

  async function onDownloadModel() {
    setModelDlBusy(true);
    setModelDlMsg(null);
    setModelDlProgress(null);
    try {
      await downloadWhisperModel(settings.whisperModel, settings.whisperModelsDir);
      setModelDlMsg(t("local_whisper.model_dl_msg_done"));
      onRefreshReadiness?.();
    } catch (e) {
      const msg =
        typeof e === "string"
          ? e
          : e instanceof Error
            ? e.message
            : "Download failed";
      setModelDlMsg(msg);
    } finally {
      setModelDlBusy(false);
      setModelDlProgress(null);
    }
  }

  async function onDownloadWhisperCliSetup() {
    setWhisperCliDlBusy(true);
    setWhisperCliDlMsg(null);
    setWhisperCliDlError(null);
    setWhisperCliInstallSuccess(false);
    setWhisperCliDlProgress(null);
    try {
      const p = await downloadWhisperCli(isWin ? settings.whisperAcceleration : undefined);
      await onPersistSettings({ ...settings, whisperCliPath: p.whisperCliPath });
      setWhisperCliInstallSuccess(true);
      onRefreshReadiness?.();
    } catch (e) {
      const msg =
        typeof e === "string"
          ? e
          : e instanceof Error
            ? e.message
            : "Setup failed";
      setWhisperCliDlError(msg);
    } finally {
      setWhisperCliDlBusy(false);
      setWhisperCliDlProgress(null);
    }
  }

  async function onDownloadMediaTools() {
    setToolDlBusy(true);
    setToolDlMsg(null);
    setToolDlProgress(null);
    try {
      const p = await downloadMediaTools();
      await onPersistSettings({
        ...settings,
        ffmpegPath: p.ffmpegPath,
        ytDlpPath: p.ytDlpPath,
      });
      setToolDlMsg(t("media_tools.tools_msg_done"));
    } catch (e) {
      const msg =
        typeof e === "string"
          ? e
          : e instanceof Error
            ? e.message
            : "Download failed";
      setToolDlMsg(msg);
    } finally {
      setToolDlBusy(false);
      setToolDlProgress(null);
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
      await onPersistSettings({ ...settings, ytDlpJsRuntimes: res.jsRuntimes });
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

  const useLocal = settings.transcriptionMode === "localWhisper";
  const useBrowser = settings.transcriptionMode === "browserWhisper";

  return (
    <section
      className="settings-panel settings-panel--embedded"
      aria-label={t("panel_aria")}
    >
      <h2>{t("title")}</h2>

      <p className="settings-section-title">{t("section.language")}</p>
      <label className="field">
        <span>{t("language.field_label")}</span>
        <select
          aria-label={t("language.select_aria")}
          data-testid="settings-language-switcher"
          value={settings.uiLanguage}
          onChange={(e) => onLanguageChange(e.target.value as UiLanguage)}
        >
          {FULL_LANG_OPTIONS.map((opt) => (
            <option key={opt.value} value={opt.value}>
              {opt.label}
            </option>
          ))}
        </select>
        <p className="hint">
          <Trans i18nKey="language.hint" t={t} components={{ strong: <strong />, code: <code /> }} />
        </p>
      </label>

      <p className="settings-section-title">{t("section.output")}</p>
      <label className="field">
        <span>{t("output.folder_label")}</span>
        <div className="row-gap">
          <input
            type="text"
            readOnly
            value={settings.outputDir ?? ""}
            placeholder={t("output.folder_placeholder")}
          />
          <button type="button" onClick={() => void pickOutputDir()}>
            {t("output.browse")}
          </button>
          <button type="button" onClick={() => void useDocumentsAsOutput()}>
            {t("output.use_documents")}
          </button>
        </div>
      </label>
      <p className="hint">
        <Trans i18nKey="output.hint" t={t} components={{ strong: <strong /> }} />
      </p>

      <p className="settings-section-title">{t("section.transcription")}</p>
      <div
        className="settings-highlight"
        data-testid="transcription-mode-card"
        role="group"
        aria-label={t("transcription.transcription_aria")}
      >
        <label className="field mb-sm">
          <span>{t("transcription.mode_label")}</span>
          <select
            aria-label={t("transcription.mode_aria")}
            value={settings.transcriptionMode}
            onChange={(e) =>
              onChange({
                ...settings,
                transcriptionMode: e.target.value as TranscriptionMode,
              })
            }
          >
            <option value="httpApi">{t("transcription.mode_options.http_api")}</option>
            <option value="localWhisper">{t("transcription.mode_options.local")}</option>
            <option value="browserWhisper">{t("transcription.mode_options.browser")}</option>
          </select>
        </label>
        <p className="hint settings-mode-summary">
          {useLocal
            ? t("transcription.mode_summary.local")
            : useBrowser
              ? t("transcription.mode_summary.browser")
              : t("transcription.mode_summary.cloud")}
        </p>
        {useBrowser ? (
          <p className="hint hint--warn" role="note">
            {t("transcription.browser_warn")}
          </p>
        ) : null}

        {useLocal ? (
          <div className="local-whisper-block" data-testid="local-whisper-block">
            <div className="settings-step-card">
              <p className="settings-step-title">{t("local_whisper.step1_title")}</p>
              <p className="hint settings-step-body">
                <Trans
                  i18nKey="local_whisper.step1_hint"
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
              {isLinux ? (
                <div
                  className="onboarding-info-callout settings-linux-whisper-hint"
                  data-testid="linux-whisper-instructions"
                >
                  <p className="hint settings-step-body">
                    <Trans
                      i18nKey="local_whisper.linux_intro"
                      t={t}
                      components={{ strong: <strong /> }}
                    />
                  </p>
                  <ul className="hint settings-step-body">
                    <li>
                      <Trans
                        i18nKey="local_whisper.linux_li_ubuntu"
                        t={t}
                        components={{
                          strong: <strong />,
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
                        i18nKey="local_whisper.linux_li_fedora"
                        t={t}
                        components={{ strong: <strong />, code: <code /> }}
                      />
                    </li>
                    <li>
                      <Trans
                        i18nKey="local_whisper.linux_li_arch"
                        t={t}
                        components={{ strong: <strong />, code: <code /> }}
                      />
                    </li>
                  </ul>
                </div>
              ) : null}
              <div className="row-gap mb-sm">
                {showManagedToolDownloads ? (
                  <button
                    type="button"
                    disabled={whisperCliDlBusy || saving}
                    onClick={() => void onDownloadWhisperCliSetup()}
                  >
                    {whisperCliDlBusy
                      ? t("local_whisper.btn_working")
                      : isWin
                        ? t("local_whisper.btn_download_win")
                        : t("local_whisper.btn_find_mac")}
                  </button>
                ) : null}
                <button type="button" onClick={() => void pickWhisperCliExecutable()}>
                  {t("local_whisper.btn_pick_file")}
                </button>
              </div>
              {whisperCliDlProgress &&
              whisperCliDlProgress.total != null &&
              whisperCliDlProgress.total > 0 ? (
                <div className="download-progress-wrap">
                  <progress
                    value={whisperCliDlProgress.received}
                    max={whisperCliDlProgress.total}
                  />
                </div>
              ) : null}
              {whisperCliDlMsg && whisperCliDlBusy ? (
                <p className="hint mt-xs">{whisperCliDlMsg}</p>
              ) : null}
              {whisperCliInstallSuccess ? (
                <div className="onboarding-success-callout mt-sm" role="status">
                  <span className="onboarding-check-circle" aria-hidden>
                    ✓
                  </span>
                  <div className="onboarding-success-callout-text">
                    <Trans
                      i18nKey="local_whisper.install_success"
                      t={t}
                      components={{ strong: <strong /> }}
                    />
                  </div>
                </div>
              ) : null}
              {whisperCliDlError ? (
                <div className="onboarding-error-callout mt-sm" role="alert">
                  <span className="onboarding-error-icon" aria-hidden>
                    !
                  </span>
                  <div>{whisperCliDlError}</div>
                </div>
              ) : null}
              <label className="field">
                <span>{t("local_whisper.path_field_label")}</span>
                <div className="row-gap">
                  <input
                    type="text"
                    value={settings.whisperCliPath ?? ""}
                    onChange={(e) =>
                      onChange({
                        ...settings,
                        whisperCliPath: e.target.value.trim() || null,
                      })
                    }
                    placeholder={t("local_whisper.path_placeholder")}
                  />
                </div>
              </label>
            </div>

            {isWin ? (
              <div className="settings-step-card" data-testid="whisper-acceleration-card">
                <p className="settings-step-title">{t("local_whisper.step15_title")}</p>
                <p className="hint settings-step-body">
                  <Trans
                    i18nKey="local_whisper.step15_hint"
                    t={t}
                    components={{ strong: <strong /> }}
                  />
                </p>
                {gpuInfo ? (
                  <p className="hint settings-step-body" data-testid="gpu-detect-hint">
                    {t("local_whisper.gpu_detected_prefix")}
                    {gpuInfo.kind === "none"
                      ? t("local_whisper.gpu_none")
                      : `${gpuInfo.kind.toUpperCase()} (${gpuInfo.names.join(", ") || t("local_whisper.gpu_unknown_adapter")})`}
                    {gpuInfo.kind === "nvidia"
                      ? t("local_whisper.gpu_auto_nvidia")
                      : gpuInfo.kind === "none"
                        ? t("local_whisper.gpu_auto_none")
                        : t("local_whisper.gpu_auto_other")}
                  </p>
                ) : null}
                <div
                  className="onboarding-radio-group"
                  role="radiogroup"
                  aria-label={t("local_whisper.accel_aria")}
                >
                  {(
                    [
                      ["auto", t("local_whisper.accel.auto")],
                      ["cuda", t("local_whisper.accel.cuda")],
                      ["vulkan", t("local_whisper.accel.vulkan")],
                      ["cpu", t("local_whisper.accel.cpu")],
                    ] as ReadonlyArray<[WhisperAcceleration, string]>
                  ).map(([val, label]) => (
                    <label className="onboarding-radio" key={val}>
                      <input
                        type="radio"
                        name="whisper-acceleration"
                        checked={settings.whisperAcceleration === val}
                        onChange={() => onChange({ ...settings, whisperAcceleration: val })}
                      />
                      <span>{label}</span>
                    </label>
                  ))}
                </div>
                <div className="row-gap mt-xs">
                  <button
                    type="button"
                    disabled={whisperCliDlBusy || saving}
                    onClick={() => void onDownloadWhisperCliSetup()}
                  >
                    {whisperCliDlBusy ? t("local_whisper.btn_working") : t("local_whisper.btn_redownload")}
                  </button>
                </div>
              </div>
            ) : null}

            <div className="settings-step-card">
              <p className="settings-step-title">{t("local_whisper.step2_title")}</p>
              <p className="hint settings-step-body">{t("local_whisper.step2_hint")}</p>
              <label className="field">
                <span>{t("local_whisper.models_dir_label")}</span>
                <div className="row-gap">
                  <input
                    type="text"
                    readOnly
                    value={settings.whisperModelsDir ?? ""}
                    placeholder={defaultModelsPath ?? t("local_whisper.models_dir_placeholder_default")}
                  />
                  <button type="button" onClick={() => void pickWhisperModelsDir()}>
                    {t("output.browse")}
                  </button>
                </div>
              </label>

              <label className="field">
                <span>{t("local_whisper.model_label")}</span>
                <select
                  aria-label={t("local_whisper.model_aria")}
                  value={settings.whisperModel}
                  onChange={(e) =>
                    onChange({ ...settings, whisperModel: e.target.value })
                  }
                >
                  {whisperModels.map((m) => (
                    <option key={m.id} value={m.id}>
                      {t("local_whisper.model_option", {
                        id: m.id,
                        sizeMib: m.sizeMib,
                        fileName: m.fileName,
                      })}
                    </option>
                  ))}
                </select>
              </label>

              <div className="row-gap mt-xs">
                <button
                  type="button"
                  disabled={modelDlBusy}
                  onClick={() => void onDownloadModel()}
                >
                  {modelDlBusy ? t("local_whisper.btn_downloading") : t("local_whisper.btn_download_model")}
                </button>
              </div>

              {modelDlProgress && modelDlProgress.total != null && modelDlProgress.total > 0 ? (
                <div className="download-progress-wrap">
                  <progress
                    value={modelDlProgress.received}
                    max={modelDlProgress.total}
                  />
                </div>
              ) : null}
              {modelDlMsg ? (
                <p className="hint" data-testid="model-download-msg">
                  {modelDlMsg}
                </p>
              ) : null}
            </div>
          </div>
        ) : useBrowser ? (
          <div className="browser-whisper-block" data-testid="browser-whisper-block">
            <div className="settings-step-card">
              <p className="settings-step-title">{t("browser_whisper.model_size_title")}</p>
              <p className="hint settings-step-body">
                <Trans
                  i18nKey="browser_whisper.model_size_hint"
                  t={t}
                  components={{ code: <code /> }}
                />
              </p>
              <label className="field">
                <span>{t("browser_whisper.model_label")}</span>
                <select
                  aria-label={t("browser_whisper.model_aria")}
                  value={settings.whisperModel}
                  onChange={(e) =>
                    onChange({ ...settings, whisperModel: e.target.value })
                  }
                >
                  {whisperModels.map((m) => (
                    <option key={m.id} value={m.id}>
                      {t("browser_whisper.model_option", { id: m.id, sizeMib: m.sizeMib })}
                    </option>
                  ))}
                </select>
              </label>
            </div>
          </div>
        ) : (
          <>
            <label className="field">
              <span>{t("cloud.api_url_label")}</span>
              <input
                type="url"
                value={settings.apiBaseUrl}
                onChange={(e) =>
                  onChange({ ...settings, apiBaseUrl: e.target.value })
                }
              />
            </label>
            <label className="field">
              <span>{t("cloud.api_model_label")}</span>
              <input
                type="text"
                value={settings.apiModel}
                onChange={(e) => onChange({ ...settings, apiModel: e.target.value })}
              />
            </label>
            <label className="field">
              <span>{t("cloud.api_key_label")}</span>
              <input
                type="password"
                autoComplete="off"
                value={settings.apiKey}
                onChange={(e) => onChange({ ...settings, apiKey: e.target.value })}
              />
            </label>
            <details className="help-details">
              <summary>{t("cloud.where_key_summary")}</summary>
              <div className="help-details-body">
                <p>
                  <Trans
                    i18nKey="cloud.where_key_body_provider"
                    t={t}
                    components={{ code: <code /> }}
                  />
                </p>
                <p>
                  <Trans
                    i18nKey="cloud.where_key_body_openai"
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
                <p className="hint help-details-warning">{t("cloud.where_key_warning")}</p>
              </div>
            </details>
            <p className="hint" data-testid="cloud-credential-store-hint">
              {t("cloud.credential_store_hint")}
            </p>
          </>
        )}
      </div>

      <p className="settings-section-title">{t("section.media_tools")}</p>
      {showManagedToolDownloads ? (
        <>
          {isWin ? (
            <p className="hint">
              <Trans
                i18nKey="media_tools.win_hint"
                t={t}
                components={{
                  strong: <strong />,
                  code: <code />,
                  a: (
                    <a
                      href="https://github.com/BtbN/FFmpeg-Builds"
                      target="_blank"
                      rel="noopener noreferrer"
                    />
                  ),
                }}
              />
            </p>
          ) : null}
          {isMac ? (
            <p className="hint">
              <Trans
                i18nKey="media_tools.mac_hint"
                t={t}
                components={{
                  strong: <strong />,
                  code: <code />,
                  a: (
                    <a
                      href="https://github.com/eugeneware/ffmpeg-static"
                      target="_blank"
                      rel="noopener noreferrer"
                    />
                  ),
                }}
              />
            </p>
          ) : null}
          <div className="row-gap mb-sm">
            <button
              type="button"
              disabled={toolDlBusy}
              onClick={() => void onDownloadMediaTools()}
            >
              {toolDlBusy ? t("media_tools.btn_downloading") : t("media_tools.btn_download")}
            </button>
          </div>
          {toolDlProgress && toolDlProgress.total != null && toolDlProgress.total > 0 ? (
            <div className="download-progress-wrap">
              <progress
                value={toolDlProgress.received}
                max={toolDlProgress.total}
              />
            </div>
          ) : null}
          {toolDlMsg ? <p className="hint">{toolDlMsg}</p> : null}
        </>
      ) : (
        <p className="hint">
          <Trans
            i18nKey="media_tools.linux_hint"
            t={t}
            components={{ strong: <strong />, code: <code /> }}
          />
        </p>
      )}

      <details className="help-details">
        <summary>{t("media_tools.self_install_summary")}</summary>
        <div className="help-details-body">
          <p>
            <Trans
              i18nKey="media_tools.self_install_intro"
              t={t}
              components={{ strong: <strong />, code: <code /> }}
            />
          </p>
          <label className="field">
            <span>{t("media_tools.ffmpeg_path_label")}</span>
            <input
              type="text"
              value={settings.ffmpegPath ?? ""}
              onChange={(e) =>
                onChange({
                  ...settings,
                  ffmpegPath: e.target.value.trim() || null,
                })
              }
              placeholder={t("media_tools.auto_detect_placeholder")}
            />
          </label>
          <label className="field">
            <span>{t("media_tools.ytdlp_path_label")}</span>
            <input
              type="text"
              value={settings.ytDlpPath ?? ""}
              onChange={(e) =>
                onChange({
                  ...settings,
                  ytDlpPath: e.target.value.trim() || null,
                })
              }
              placeholder={t("media_tools.auto_detect_placeholder")}
            />
          </label>
          <label className="field">
            <span>{t("media_tools.js_runtimes_label")}</span>
            <input
              type="text"
              value={settings.ytDlpJsRuntimes ?? ""}
              onChange={(e) =>
                onChange({
                  ...settings,
                  ytDlpJsRuntimes: e.target.value.trim() || null,
                })
              }
              placeholder={t("media_tools.js_runtimes_placeholder")}
              aria-label={t("media_tools.js_runtimes_aria")}
            />
            <div
              className="field-lang-examples"
              role="group"
              aria-label={t("media_tools.js_runtimes_aria")}
            >
              <span className="field-lang-examples-label">{t("media_tools.common_label")}</span>
              <button
                type="button"
                className="lang-code-chip"
                onClick={() => onChange({ ...settings, ytDlpJsRuntimes: "deno" })}
              >
                deno
              </button>
              <button
                type="button"
                className="lang-code-chip"
                onClick={() => onChange({ ...settings, ytDlpJsRuntimes: "nodejs" })}
              >
                nodejs
              </button>
              <button
                type="button"
                className="lang-code-chip"
                onClick={() => onChange({ ...settings, ytDlpJsRuntimes: "node" })}
              >
                node
              </button>
            </div>
          </label>
          <p className="hint">
            <Trans
              i18nKey="media_tools.js_runtimes_hint"
              t={t}
              components={{ code: <code /> }}
            />
          </p>
          {showManagedToolDownloads ? (
            <div className="onboarding-block">
              <button
                type="button"
                className="primary"
                disabled={denoDlBusy}
                onClick={() => void onInstallDeno()}
              >
                {denoDlBusy ? t("media_tools.btn_install_deno_busy") : t("media_tools.btn_install_deno")}
              </button>
              {denoDlProgress && denoDlProgress.total != null && denoDlProgress.total > 0 ? (
                <div className="download-progress-wrap">
                  <progress value={denoDlProgress.received} max={denoDlProgress.total} />
                </div>
              ) : null}
              {denoDlMsg && denoDlBusy ? <p className="hint">{denoDlMsg}</p> : null}
              {denoInstallSuccess ? (
                <p className="hint" style={{ color: "var(--ok)" }}>
                  <Trans
                    i18nKey="media_tools.deno_success"
                    t={t}
                    components={{ strong: <strong />, code: <code /> }}
                  />
                </p>
              ) : null}
              {denoDlError ? <p className="hint" style={{ color: "var(--err)" }}>{denoDlError}</p> : null}
            </div>
          ) : null}
          <label className="field">
            <span>{t("media_tools.cookies_label")}</span>
            <select
              aria-label={t("media_tools.cookies_aria")}
              value={settings.cookiesFromBrowser}
              onChange={(e) =>
                onChange({ ...settings, cookiesFromBrowser: e.target.value as CookiesFromBrowser })
              }
            >
              <option value="auto">{t("media_tools.cookies_options.auto")}</option>
              <option value="chrome">{t("media_tools.cookies_options.chrome")}</option>
              <option value="brave">{t("media_tools.cookies_options.brave")}</option>
              <option value="edge">{t("media_tools.cookies_options.edge")}</option>
              <option value="firefox">{t("media_tools.cookies_options.firefox")}</option>
              <option value="none">{t("media_tools.cookies_options.none")}</option>
            </select>
          </label>
          <p className="hint">
            <Trans
              i18nKey="media_tools.cookies_hint"
              t={t}
              components={{
                strong: <strong />,
                code: <code />,
                a: (
                  <a
                    href="https://github.com/yt-dlp/yt-dlp/issues/10927"
                    target="_blank"
                    rel="noopener noreferrer"
                  />
                ),
              }}
            />
          </p>        </div>
      </details>

      <p className="settings-section-title">{t("section.other")}</p>
      <label className="field">
        <span>{t("filename_template_label")}</span>
        <input
          type="text"
          value={settings.filenameTemplate}
          onChange={(e) =>
            onChange({ ...settings, filenameTemplate: e.target.value })
          }
          placeholder={t("filename_template_placeholder")}
        />
      </label>

      <label className="field checkbox">
        <input
          type="checkbox"
          checked={settings.deleteAudioAfter}
          onChange={(e) =>
            onChange({ ...settings, deleteAudioAfter: e.target.checked })
          }
        />
        <span>{t("delete_audio_after")}</span>
      </label>

      <label className="field checkbox">
        <input
          type="checkbox"
          checked={settings.keepDownloadedVideo}
          onChange={(e) =>
            onChange({ ...settings, keepDownloadedVideo: e.target.checked })
          }
        />
        <span>{t("keep_video")}</span>
      </label>

      <label className="field checkbox">
        <input
          type="checkbox"
          checked={settings.keepDownloadedAudio}
          onChange={(e) =>
            onChange({ ...settings, keepDownloadedAudio: e.target.checked })
          }
        />
        <span>{t("keep_audio")}</span>
      </label>

      {settings.keepDownloadedAudio && (
        <label className="field">
          <span>{t("audio_format.label")}</span>
          <select
            value={settings.downloadedAudioFormat}
            onChange={(e) =>
              onChange({
                ...settings,
                downloadedAudioFormat: e.target.value as AppSettings["downloadedAudioFormat"],
              })
            }
          >
            <option value="original">{t("audio_format.original")}</option>
            <option value="m4a">{t("audio_format.m4a")}</option>
            <option value="mp3">{t("audio_format.mp3")}</option>
          </select>
        </label>
      )}

      <label className="field checkbox">
        <input
          type="checkbox"
          checked={settings.recursiveFolderScan}
          onChange={(e) =>
            onChange({ ...settings, recursiveFolderScan: e.target.checked })
          }
        />
        <span>{t("recursive_scan")}</span>
      </label>

      <label className="field">
        <span>{t("language_iso.label")}</span>
        <input
          type="text"
          value={settings.language ?? ""}
          onChange={(e) =>
            onChange({
              ...settings,
              language: e.target.value.trim() || null,
            })
          }
          placeholder={t("language_iso.placeholder")}
          aria-describedby="language-examples-hint"
        />
        <div
          className="field-lang-examples"
          role="group"
          aria-label={t("language_iso.examples_label")}
        >
          <span className="field-lang-examples-label">{t("language_iso.examples_label")}</span>
          <button
            type="button"
            className="lang-code-chip"
            onClick={() => onChange({ ...settings, language: "ru" })}
          >
            {t("language_iso.ru_chip")}{" "}
            <span className="lang-code-chip-desc">{t("language_iso.ru_desc")}</span>
          </button>
          <button
            type="button"
            className="lang-code-chip"
            onClick={() => onChange({ ...settings, language: "uk" })}
          >
            {t("language_iso.uk_chip")}{" "}
            <span className="lang-code-chip-desc">{t("language_iso.uk_desc")}</span>
          </button>
          <button
            type="button"
            className="lang-code-chip"
            onClick={() => onChange({ ...settings, language: "en" })}
          >
            {t("language_iso.en_chip")}{" "}
            <span className="lang-code-chip-desc">{t("language_iso.en_desc")}</span>
          </button>
        </div>
        <p className="hint" id="language-examples-hint">
          <Trans i18nKey="language_iso.hint" t={t} components={{ code: <code /> }} />
        </p>
      </label>

      <p className="settings-section-title">{t("section.subtitles_fast_path")}</p>
      <p className="hint">
        <Trans i18nKey="subtitles.intro" t={t} components={{ strong: <strong /> }} />
      </p>
      <label className="field checkbox">
        <input
          type="checkbox"
          data-testid="use-subtitles-toggle"
          checked={settings.useSubtitlesWhenAvailable}
          onChange={(e) =>
            onChange({ ...settings, useSubtitlesWhenAvailable: e.target.checked })
          }
        />
        <span>{t("subtitles.toggle")}</span>
      </label>
      {settings.useSubtitlesWhenAvailable ? (
        <>
          <label className="field">
            <span>{t("subtitles.priority_label")}</span>
            <input
              type="text"
              data-testid="subtitle-priority-langs"
              value={settings.subtitlePriorityLangs.join(", ")}
              onChange={(e) =>
                onChange({
                  ...settings,
                  subtitlePriorityLangs: e.target.value
                    .split(",")
                    .map((s) => s.trim())
                    .filter((s) => s.length > 0),
                })
              }
              placeholder={t("subtitles.priority_placeholder")}
            />
            <p className="hint">
              <Trans i18nKey="subtitles.priority_hint" t={t} components={{ code: <code /> }} />
            </p>
          </label>
          <label className="field checkbox">
            <input
              type="checkbox"
              checked={settings.keepSrt}
              onChange={(e) => onChange({ ...settings, keepSrt: e.target.checked })}
            />
            <span>{t("subtitles.keep_srt")}</span>
          </label>
        </>
      ) : null}

      <button type="button" className="primary" disabled={saving} onClick={onSave}>
        {saving ? t("btn_saving") : t("btn_save")}
      </button>
    </section>
  );
}
