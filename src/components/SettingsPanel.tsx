import { open } from "@tauri-apps/plugin-dialog";
import { useEffect, useMemo, useState } from "react";
import {
  defaultDocumentsDir,
  defaultWhisperModelsDir,
  downloadMediaTools,
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
  saving: boolean;
};

function isProbablyWindows(): boolean {
  if (typeof navigator === "undefined") return false;
  return /Windows/i.test(navigator.userAgent);
}

export function SettingsPanel({
  settings,
  onChange,
  onSave,
  onPersistSettings,
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

  const showWinDownloads = useMemo(() => isProbablyWindows(), []);

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
          setToolDlMsg(`[${ev.payload.tool}] ${ev.payload.message}`);
          setToolDlProgress({
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

  async function onDownloadModel() {
    setModelDlBusy(true);
    setModelDlMsg(null);
    setModelDlProgress(null);
    try {
      await downloadWhisperModel(settings.whisperModel, settings.whisperModelsDir);
      setModelDlMsg("Model ready (verified SHA-1).");
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
          <span>Mode (cloud vs offline)</span>
          <select
            value={settings.transcriptionMode}
            onChange={(e) =>
              onChange({
                ...settings,
                transcriptionMode: e.target.value as TranscriptionMode,
              })
            }
          >
            <option value="httpApi">HTTP API — OpenAI-compatible cloud</option>
            <option value="localWhisper">
              Local Whisper — whisper.cpp on this PC (no API key)
            </option>
          </select>
        </label>

        {useLocal ? (
          <div className="local-whisper-block" data-testid="local-whisper-block">
            <p className="hint" style={{ marginTop: 0 }}>
              <strong>Offline path:</strong> install or build{" "}
              <code>whisper-cli</code> (whisper.cpp), then pick a <strong>ggml</strong> model below and
              download it (or run the queue — the first run can fetch the model).
            </p>

            <label className="field">
              <span>whisper-cli path (optional)</span>
              <input
                type="text"
                value={settings.whisperCliPath ?? ""}
                onChange={(e) =>
                  onChange({
                    ...settings,
                    whisperCliPath: e.target.value.trim() || null,
                  })
                }
                placeholder="Next to v2t.exe if empty — names: whisper-cli.exe or main.exe"
              />
            </label>

            <label className="field">
              <span>Folder for ggml .bin files</span>
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
              <span>Model (size on disk)</span>
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
                  Key is stored in the OS credential store, not in settings.json.
                </p>
              </div>
            </details>
          </>
        )}
      </div>

      <p className="settings-section-title">Media tools (ffmpeg, yt-dlp)</p>
      {showWinDownloads ? (
        <>
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
          One-click download is only on <strong>Windows</strong>. On macOS/Linux install{" "}
          <code>ffmpeg</code> and <code>yt-dlp</code> (e.g. package manager) and paste full paths below.
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
