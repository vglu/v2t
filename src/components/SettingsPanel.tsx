import { open } from "@tauri-apps/plugin-dialog";
import { useEffect, useMemo, useState } from "react";
import {
  defaultDocumentsDir,
  defaultWhisperModelsDir,
  downloadMediaTools,
  downloadWhisperCli,
  downloadWhisperModel,
  listWhisperModels,
} from "../lib/invokeSafe";
import type { AppSettings, TranscriptionMode, WhisperModelMeta } from "../types/settings";

type Props = {
  settings: AppSettings;
  onChange: (s: AppSettings) => void;
  onSave: () => void;
  /** Save merged settings (e.g. after auto-download paths). */
  onPersistSettings: (s: AppSettings) => Promise<void>;
  /** Re-run dependency check (e.g. after model download). */
  onRefreshReadiness?: () => void;
  saving: boolean;
};

function isProbablyWindows(): boolean {
  if (typeof navigator === "undefined") return false;
  return /Windows/i.test(navigator.userAgent);
}

function isProbablyMac(): boolean {
  if (typeof navigator === "undefined") return false;
  return /Macintosh|Mac OS X/i.test(navigator.userAgent);
}

export function SettingsPanel({
  settings,
  onChange,
  onSave,
  onPersistSettings,
  onRefreshReadiness,
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

  const isWin = useMemo(() => isProbablyWindows(), []);
  const isMac = useMemo(() => isProbablyMac(), []);
  const showManagedToolDownloads = isWin || isMac;

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
      const p = await downloadWhisperCli();
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

  const useLocal = settings.transcriptionMode === "localWhisper";

  return (
    <section className="settings-panel" aria-label="Settings">
      <h2>Settings</h2>

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
      <div className="settings-highlight" data-testid="transcription-mode-card">
        <label className="field" style={{ marginBottom: "0.5rem" }}>
          <span>Transcription mode</span>
          <select
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
          </select>
        </label>
        <p className="hint settings-mode-summary">
          {useLocal
            ? "No API key. You need the whisper-cli executable and one ggml .bin model on disk."
            : "Uses your provider’s API key. whisper-cli and local models are not used."}
        </p>

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
                <strong>macOS:</strong> Apple does not publish a CLI zip in those releases; the button looks for{" "}
                <code>brew install whisper-cpp</code> paths, or use <strong>Pick file…</strong>.{" "}
                <strong>Linux:</strong> use your distro / build from source.
              </p>
              <div className="row-gap" style={{ marginBottom: "0.5rem" }}>
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
                        : "Find Homebrew whisper-cli (macOS)"}
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
                <p className="hint" style={{ marginTop: "0.35rem" }}>
                  {whisperCliDlMsg}
                </p>
              ) : null}
              {whisperCliInstallSuccess ? (
                <div className="onboarding-success-callout" role="status" style={{ marginTop: "0.5rem" }}>
                  <span className="onboarding-check-circle" aria-hidden>
                    ✓
                  </span>
                  <div className="onboarding-success-callout-text">
                    <strong>whisper-cli path saved.</strong> The checklist should update after refresh.
                  </div>
                </div>
              ) : null}
              {whisperCliDlError ? (
                <div className="onboarding-error-callout" role="alert" style={{ marginTop: "0.5rem" }}>
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

              <div className="row-gap" style={{ marginTop: "0.35rem" }}>
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
          <div className="row-gap" style={{ marginBottom: "0.5rem" }}>
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
        </div>
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
        />
      </label>

      <button type="button" className="primary" disabled={saving} onClick={onSave}>
        {saving ? "Saving…" : "Save settings"}
      </button>
    </section>
  );
}
