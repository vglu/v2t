import { open } from "@tauri-apps/plugin-dialog";
import { useEffect, useMemo, useState } from "react";
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
      setModelDlMsg("Model ready (verified SHA-1).");
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
      setToolDlMsg("ffmpeg and yt-dlp saved to app data and paths updated.");
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
      aria-label="Settings"
    >
      <h2>Settings</h2>

      <p className="settings-section-title">Language</p>
      <label className="field">
        <span>UI language</span>
        <select
          aria-label="UI language (full)"
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
          <strong>Auto</strong> follows your OS locale (<code>navigator.language</code>); other
          options force a specific language. Saved instantly — no need to press Save below. The
          same switcher (compact, flag-only) lives in the top-right header.
        </p>
      </label>

      <p className="settings-section-title">Output</p>
      <label className="field">
        <span>Output folder (transcripts .txt)</span>
        <div className="row-gap">
          <input
            type="text"
            readOnly
            value={settings.outputDir ?? ""}
            placeholder="Not set"
          />
          <button type="button" onClick={() => void pickOutputDir()}>
            Browse…
          </button>
          <button type="button" onClick={() => void useDocumentsAsOutput()}>
            Use Documents
          </button>
        </div>
      </label>
      <p className="hint">
        <strong>Use Documents</strong> sets your OS “Documents” folder and saves settings. You can change it anytime with Browse.
      </p>

      <p className="settings-section-title">Transcription &amp; models</p>
      <div
        className="settings-highlight"
        data-testid="transcription-mode-card"
        role="group"
        aria-label="Transcription settings"
      >
        <label className="field mb-sm">
          <span>Transcription mode</span>
          <select
            aria-label="Transcription mode"
            value={settings.transcriptionMode}
            onChange={(e) =>
              onChange({
                ...settings,
                transcriptionMode: e.target.value as TranscriptionMode,
              })
            }
          >
            <option value="httpApi">Cloud — HTTP API (OpenAI-compatible)</option>
            <option value="localWhisper">Offline — Local Whisper (whisper.cpp)</option>
            <option value="browserWhisper">In-app — Whisper (WASM / Transformers.js)</option>
          </select>
        </label>
        <p className="hint settings-mode-summary">
          {useLocal
            ? "No API key. You need the whisper-cli executable and one ggml .bin model on disk."
            : useBrowser
              ? "No API key or whisper-cli. Transcription runs in the app (WASM); pick model size — first run may download weights."
              : "Uses your provider’s API key. whisper-cli and local models are not used."}
        </p>
        {useBrowser ? (
          <p className="hint hint--warn" role="note">
            Experimental: needs internet for the first model download; long audio may be slow or run out of memory. If
            this fails, switch to Cloud API or Local Whisper.
          </p>
        ) : null}

        {useLocal ? (
          <div className="local-whisper-block" data-testid="local-whisper-block">
            <div className="settings-step-card">
              <p className="settings-step-title">1 · whisper-cli (engine)</p>
              <p className="hint settings-step-body">
                <strong>Windows:</strong> official <code>whisper-bin-x64.zip</code> from{" "}
                <a href="https://github.com/ggml-org/whisper.cpp/releases" target="_blank" rel="noopener noreferrer">
                  ggml-org/whisper.cpp
                </a>{" "}
                (MIT) — use the button below to download into app data (includes DLLs).{" "}
                <strong>macOS:</strong> no CLI zip in those releases; the button runs{" "}
                <code>which whisper-cli</code> / <code>whisper</code> / <code>main</code> and scans Homebrew paths, or
                use <strong>Pick file…</strong> after <code>brew install whisper-cpp</code>.
              </p>
              {isLinux ? (
                <div
                  className="onboarding-info-callout settings-linux-whisper-hint"
                  data-testid="linux-whisper-instructions"
                >
                  <p className="hint settings-step-body">
                    <strong>Linux:</strong> this app does not download whisper-cli here. Install a package or build
                    from source, then set the path below or use <strong>Pick file…</strong>.
                  </p>
                  <ul className="hint settings-step-body">
                    <li>
                      <strong>Ubuntu / Debian:</strong> <code>sudo apt install whisper-cpp</code> (if available in your
                      release) or build from{" "}
                      <a href="https://github.com/ggml-org/whisper.cpp" target="_blank" rel="noopener noreferrer">
                        ggml-org/whisper.cpp
                      </a>
                    </li>
                    <li>
                      <strong>Fedora:</strong> <code>sudo dnf install whisper-cpp</code> (package name may vary)
                    </li>
                    <li>
                      <strong>Arch:</strong> AUR e.g. <code>yay -S whisper-cpp</code>
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
                      ? "Working…"
                      : isWin
                        ? "Download whisper-cli for me (Windows)"
                        : "Find whisper-cli (macOS)"}
                  </button>
                ) : null}
                <button type="button" onClick={() => void pickWhisperCliExecutable()}>
                  Pick file…
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
                    <strong>whisper-cli path saved.</strong> The checklist should update after refresh.
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
                <span>Path to executable (optional if next to app)</span>
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
                    placeholder="whisper-cli.exe / whisper-cli — or use Pick file…"
                  />
                </div>
              </label>
            </div>

            {isWin ? (
              <div className="settings-step-card" data-testid="whisper-acceleration-card">
                <p className="settings-step-title">1.5 · Whisper acceleration (Windows)</p>
                <p className="hint settings-step-body">
                  Picks which whisper.cpp build to download. <strong>CUDA</strong> needs an NVIDIA GPU
                  (10-20× faster on RTX-class hardware). <strong>Vulkan</strong> works on most NVIDIA /
                  AMD / Intel GPUs (8-15×). <strong>CPU</strong> is the safe baseline. Changing this
                  does not auto-redownload; click <strong>Re-download whisper-cli</strong> below to
                  fetch the matching bundle.
                </p>
                {gpuInfo ? (
                  <p className="hint settings-step-body" data-testid="gpu-detect-hint">
                    Detected: {gpuInfo.kind === "none"
                      ? "no discrete GPU recognized"
                      : `${gpuInfo.kind.toUpperCase()} (${gpuInfo.names.join(", ") || "unknown adapter"})`}
                    {gpuInfo.kind === "nvidia"
                      ? " — Auto will pick CUDA."
                      : gpuInfo.kind === "none"
                        ? " — Auto stays on CPU."
                        : " — Auto stays on CPU; pick Vulkan manually if drivers are current."}
                  </p>
                ) : null}
                <div
                  className="onboarding-radio-group"
                  role="radiogroup"
                  aria-label="Whisper acceleration"
                >
                  {(
                    [
                      ["auto", "Auto (recommended)"],
                      ["cuda", "CUDA — NVIDIA only"],
                      ["vulkan", "Vulkan — NVIDIA / AMD / Intel"],
                      ["cpu", "CPU — safe baseline"],
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
                    {whisperCliDlBusy ? "Working…" : "Re-download whisper-cli for selected backend"}
                  </button>
                </div>
              </div>
            ) : null}

            <div className="settings-step-card">
              <p className="settings-step-title">2 · GGML model (.bin)</p>
              <p className="hint settings-step-body">
                Choose a model size, then download. The checklist turns green when the file exists and the
                SHA-1 matches the catalog (same check as after download).
              </p>
              <label className="field">
                <span>Folder for .bin files</span>
                <div className="row-gap">
                  <input
                    type="text"
                    readOnly
                    value={settings.whisperModelsDir ?? ""}
                    placeholder={defaultModelsPath ?? "Default: app data / models"}
                  />
                  <button type="button" onClick={() => void pickWhisperModelsDir()}>
                    Browse…
                  </button>
                </div>
              </label>

              <label className="field">
                <span>Model</span>
                <select
                  aria-label="Whisper GGML model"
                  value={settings.whisperModel}
                  onChange={(e) =>
                    onChange({ ...settings, whisperModel: e.target.value })
                  }
                >
                  {whisperModels.map((m) => (
                    <option key={m.id} value={m.id}>
                      {m.id} — ~{m.sizeMib} MiB ({m.fileName})
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
                  {modelDlBusy ? "Downloading…" : "Download / verify model"}
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
              <p className="settings-step-title">Model size (browser)</p>
              <p className="hint settings-step-body">
                Same labels as the local catalog; the app maps them to Transformers.js checkpoints. No ggml{" "}
                <code>.bin</code> download here — weights load on first transcription (needs network once per model).
              </p>
              <label className="field">
                <span>Model</span>
                <select
                  aria-label="In-app Whisper model size"
                  value={settings.whisperModel}
                  onChange={(e) =>
                    onChange({ ...settings, whisperModel: e.target.value })
                  }
                >
                  {whisperModels.map((m) => (
                    <option key={m.id} value={m.id}>
                      {m.id} — ~{m.sizeMib} MiB (browser, approximate)
                    </option>
                  ))}
                </select>
              </label>
            </div>
          </div>
        ) : (
          <>
            <label className="field">
              <span>API base URL</span>
              <input
                type="url"
                value={settings.apiBaseUrl}
                onChange={(e) =>
                  onChange({ ...settings, apiBaseUrl: e.target.value })
                }
              />
            </label>
            <label className="field">
              <span>API model name</span>
              <input
                type="text"
                value={settings.apiModel}
                onChange={(e) => onChange({ ...settings, apiModel: e.target.value })}
              />
            </label>
            <label className="field">
              <span>API key</span>
              <input
                type="password"
                autoComplete="off"
                value={settings.apiKey}
                onChange={(e) => onChange({ ...settings, apiKey: e.target.value })}
              />
            </label>
            <details className="help-details">
              <summary>Where do I get an API key?</summary>
              <div className="help-details-body">
                <p>
                  Use a provider with OpenAI-style{" "}
                  <code>POST …/audio/transcriptions</code> and JSON with a <code>text</code> field.
                </p>
                <p>
                  <strong>OpenAI:</strong>{" "}
                  <a
                    href="https://platform.openai.com/api-keys"
                    target="_blank"
                    rel="noopener noreferrer"
                  >
                    API keys
                  </a>
                  , base <code>https://api.openai.com/v1</code>, model e.g. <code>whisper-1</code>.
                </p>
                <p className="hint help-details-warning">
                  Treat the key like a password: do not share it or paste it into public chats or git.
                </p>
              </div>
            </details>
            <p className="hint">
              API key is saved in the OS credential store (Windows Credential Manager, macOS Keychain,
              Secret Service on Linux). The settings file keeps other options only (no key in plain text).
            </p>
          </>
        )}
      </div>

      <p className="settings-section-title">Media tools (ffmpeg, yt-dlp)</p>
      {showManagedToolDownloads ? (
        <>
          {isWin ? (
            <p className="hint">
              <strong>Download for me (Windows):</strong> fetches <code>yt-dlp.exe</code> from GitHub
              releases and a <strong>GPL</strong> FFmpeg zip from{" "}
              <a
                href="https://github.com/BtbN/FFmpeg-Builds"
                target="_blank"
                rel="noopener noreferrer"
              >
                BtbN/FFmpeg-Builds
              </a>
              , extracts <code>ffmpeg.exe</code> into your app data folder, then saves paths in settings.
            </p>
          ) : null}
          {isMac ? (
            <p className="hint">
              <strong>Download for me (macOS):</strong> fetches <code>yt-dlp_macos</code> from GitHub
              releases and a static <code>ffmpeg</code> for your CPU (
              <a
                href="https://github.com/eugeneware/ffmpeg-static"
                target="_blank"
                rel="noopener noreferrer"
              >
                ffmpeg-static
              </a>
              ), saves both under app data and updates paths. If macOS blocks them, allow in{" "}
              <strong>Privacy &amp; Security</strong> or clear quarantine (
              <code>xattr -dr com.apple.quarantine …</code>).
            </p>
          ) : null}
          <div className="row-gap mb-sm">
            <button
              type="button"
              disabled={toolDlBusy}
              onClick={() => void onDownloadMediaTools()}
            >
              {toolDlBusy ? "Downloading…" : "Download ffmpeg & yt-dlp for me"}
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
          One-click download is only on <strong>Windows</strong> and <strong>macOS</strong>. On Linux
          install <code>ffmpeg</code> and <code>yt-dlp</code> (e.g. package manager) and paste full paths
          below.
        </p>
      )}

      <details className="help-details">
        <summary>I’ll install ffmpeg / yt-dlp myself</summary>
        <div className="help-details-body">
          <p>
            Put <strong>ffmpeg</strong> and <strong>yt-dlp</strong> next to <code>v2t.exe</code> (or in
            a <code>bin</code> subfolder), or enter full paths here.
          </p>
          <label className="field">
            <span>ffmpeg path (optional)</span>
            <input
              type="text"
              value={settings.ffmpegPath ?? ""}
              onChange={(e) =>
                onChange({
                  ...settings,
                  ffmpegPath: e.target.value.trim() || null,
                })
              }
              placeholder="Auto-detect if next to app"
            />
          </label>
          <label className="field">
            <span>yt-dlp path (optional)</span>
            <input
              type="text"
              value={settings.ytDlpPath ?? ""}
              onChange={(e) =>
                onChange({
                  ...settings,
                  ytDlpPath: e.target.value.trim() || null,
                })
              }
              placeholder="Auto-detect if next to app"
            />
          </label>
          <label className="field">
            <span>yt-dlp JS runtimes (optional)</span>
            <input
              type="text"
              value={settings.ytDlpJsRuntimes ?? ""}
              onChange={(e) =>
                onChange({
                  ...settings,
                  ytDlpJsRuntimes: e.target.value.trim() || null,
                })
              }
              placeholder="e.g. deno — see yt-dlp wiki (EJS)"
              aria-label="yt-dlp JavaScript runtimes for EJS"
            />
            <div
              className="field-lang-examples"
              role="group"
              aria-label="Insert JS runtime values"
            >
              <span className="field-lang-examples-label">Common:</span>
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
            If YouTube fails with "no supported JavaScript runtime", install Deno or Node and set this
            to the runtime name yt-dlp expects (often <code>deno</code>).
          </p>
          {showManagedToolDownloads ? (
            <div className="onboarding-block">
              <button
                type="button"
                className="primary"
                disabled={denoDlBusy}
                onClick={() => void onInstallDeno()}
              >
                {denoDlBusy ? "Installing…" : "Download & install Deno for me"}
              </button>
              {denoDlProgress && denoDlProgress.total != null && denoDlProgress.total > 0 ? (
                <div className="download-progress-wrap">
                  <progress value={denoDlProgress.received} max={denoDlProgress.total} />
                </div>
              ) : null}
              {denoDlMsg && denoDlBusy ? <p className="hint">{denoDlMsg}</p> : null}
              {denoInstallSuccess ? (
                <p className="hint" style={{ color: "var(--ok)" }}>
                  <strong>Done.</strong> Deno installed; JS runtimes set to <code>deno</code>.
                </p>
              ) : null}
              {denoDlError ? <p className="hint" style={{ color: "var(--err)" }}>{denoDlError}</p> : null}
            </div>
          ) : null}
          <label className="field">
            <span>Cookies source for yt-dlp (YouTube / TikTok age-gate)</span>
            <select
              aria-label="Browser to read cookies from"
              value={settings.cookiesFromBrowser}
              onChange={(e) =>
                onChange({ ...settings, cookiesFromBrowser: e.target.value as CookiesFromBrowser })
              }
            >
              <option value="auto">Auto (Edge on Windows, Chrome on macOS, Firefox on Linux)</option>
              <option value="chrome">Chrome</option>
              <option value="brave">Brave</option>
              <option value="edge">Edge</option>
              <option value="firefox">Firefox</option>
              <option value="none">Disabled — do not use browser cookies</option>
            </select>
          </label>
          <p className="hint">
            Passes <code>--cookies-from-browser</code> to yt-dlp. The browser must be installed and you
            must be logged in to YouTube / TikTok in it.{" "}
            <strong>Chrome, Brave and Edge have two known issues on Windows:</strong>{" "}
            (1) their cookie database is locked while the browser is running — close it first;{" "}
            (2) since Chrome 127+ cookies are encrypted with app-bound encryption (DPAPI) that yt-dlp
            cannot decrypt even when the browser is closed{" "}
            (<a href="https://github.com/yt-dlp/yt-dlp/issues/10927" target="_blank" rel="noopener noreferrer">issue #10927</a>).{" "}
            <strong>Firefox is the most reliable option</strong> — log in there and select Firefox.
          </p>        </div>
      </details>

      <p className="settings-section-title">Other</p>
      <label className="field">
        <span>Filename template</span>
        <input
          type="text"
          value={settings.filenameTemplate}
          onChange={(e) =>
            onChange({ ...settings, filenameTemplate: e.target.value })
          }
          placeholder="{title}_{date}_{index}_t{track}.txt"
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
        <span>Delete temp audio after success</span>
      </label>

      <label className="field checkbox">
        <input
          type="checkbox"
          checked={settings.keepDownloadedVideo}
          onChange={(e) =>
            onChange({ ...settings, keepDownloadedVideo: e.target.checked })
          }
        />
        <span>
          Save downloaded video to output folder (URL jobs — second yt-dlp pass, best mp4)
        </span>
      </label>

      <label className="field checkbox">
        <input
          type="checkbox"
          checked={settings.keepDownloadedAudio}
          onChange={(e) =>
            onChange({ ...settings, keepDownloadedAudio: e.target.checked })
          }
        />
        <span>
          Save downloaded audio to output folder (URL jobs + local video — extracted via ffmpeg)
        </span>
      </label>

      {settings.keepDownloadedAudio && (
        <label className="field">
          <span>Audio format</span>
          <select
            value={settings.downloadedAudioFormat}
            onChange={(e) =>
              onChange({
                ...settings,
                downloadedAudioFormat: e.target.value as AppSettings["downloadedAudioFormat"],
              })
            }
          >
            <option value="original">Original (bestaudio / stream copy, no re-encode)</option>
            <option value="m4a">m4a (AAC)</option>
            <option value="mp3">mp3</option>
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
        <span>Recursive folder scan (include subfolders)</span>
      </label>

      <label className="field">
        <span>Language (optional, ISO code)</span>
        <input
          type="text"
          value={settings.language ?? ""}
          onChange={(e) =>
            onChange({
              ...settings,
              language: e.target.value.trim() || null,
            })
          }
          placeholder="auto"
          aria-describedby="language-examples-hint"
        />
        <div
          className="field-lang-examples"
          role="group"
          aria-label="Insert example language codes"
        >
          <span className="field-lang-examples-label">Examples:</span>
          <button
            type="button"
            className="lang-code-chip"
            onClick={() => onChange({ ...settings, language: "ru" })}
          >
            ru <span className="lang-code-chip-desc">(Russian)</span>
          </button>
          <button
            type="button"
            className="lang-code-chip"
            onClick={() => onChange({ ...settings, language: "uk" })}
          >
            uk <span className="lang-code-chip-desc">(Ukrainian)</span>
          </button>
          <button
            type="button"
            className="lang-code-chip"
            onClick={() => onChange({ ...settings, language: "en" })}
          >
            en <span className="lang-code-chip-desc">(English)</span>
          </button>
        </div>
        <p className="hint" id="language-examples-hint">
          Or any other ISO 639-1 code (e.g. <code>de</code>, <code>fr</code>, <code>pl</code>).
        </p>
      </label>

      <p className="settings-section-title">Subtitles fast-path</p>
      <p className="hint">
        For YouTube videos with <strong>manual</strong> subtitles in a priority language, fetch the
        SRT directly via yt-dlp and skip download + Whisper. <strong>Auto-generated</strong>{" "}
        captions are intentionally ignored (lower quality than Whisper-medium for non-English).
        Single-video URLs only — pure-playlist URLs continue to download + transcribe normally.
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
        <span>Use subtitles when available (skip Whisper)</span>
      </label>
      {settings.useSubtitlesWhenAvailable ? (
        <>
          <label className="field">
            <span>Priority languages (comma-separated ISO codes)</span>
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
              placeholder="uk, ru, en"
            />
            <p className="hint">
              First match wins. Regional variants are matched (e.g. <code>en</code> matches{" "}
              <code>en-US</code>).
            </p>
          </label>
          <label className="field checkbox">
            <input
              type="checkbox"
              checked={settings.keepSrt}
              onChange={(e) => onChange({ ...settings, keepSrt: e.target.checked })}
            />
            <span>Also save the original .srt next to the .txt transcript</span>
          </label>
        </>
      ) : null}

      <button type="button" className="primary" disabled={saving} onClick={onSave}>
        {saving ? "Saving…" : "Save settings"}
      </button>
    </section>
  );
}
