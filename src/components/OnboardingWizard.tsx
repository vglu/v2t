import { open } from "@tauri-apps/plugin-dialog";
import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import {
  defaultDocumentsDir,
  defaultWhisperModelsDir,
  downloadMediaTools,
  downloadWhisperCli,
  downloadWhisperModel,
  listWhisperModels,
} from "../lib/invokeSafe";
import type { AppSettings, TranscriptionMode, WhisperModelMeta } from "../types/settings";

const TOTAL_STEPS = 6;

type ModeChoice = "cloud" | "local" | "later";

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

function isProbablyWindows(): boolean {
  if (typeof navigator === "undefined") return false;
  return /Windows/i.test(navigator.userAgent);
}

function isProbablyMac(): boolean {
  if (typeof navigator === "undefined") return false;
  return /Macintosh|Mac OS X/i.test(navigator.userAgent);
}

function stepTitle(step: number, modeChoice: ModeChoice): string {
  switch (step) {
    case 0:
      return "Welcome";
    case 1:
      return "Output folder";
    case 2:
      return "ffmpeg & yt-dlp";
    case 3:
      return "Transcription";
    case 4:
      if (modeChoice === "cloud") return "Cloud API";
      if (modeChoice === "local") return "Local Whisper";
      return "Finish in Settings";
    case 5:
      return "Run jobs";
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

  const prevOpen = useRef(false);
  const isWin = useMemo(() => isProbablyWindows(), []);
  const isMac = useMemo(() => isProbablyMac(), []);
  const showManagedToolDownloads = isWin || isMac;

  useEffect(() => {
    if (wizardOpen && !prevOpen.current) {
      setStep(0);
      setModeChoice(settings.transcriptionMode === "localWhisper" ? "local" : "cloud");
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

  async function onSetupWhisperCli() {
    setWhisperCliBusy(true);
    setWhisperCliError(null);
    setWhisperCliSuccess(false);
    setWhisperCliLineMsg(null);
    setWhisperCliProgress(null);
    try {
      const p = await downloadWhisperCli();
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
        setOutputError("Pick a folder with Browse, or use Use Documents (recommended).");
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
        setCloudError("Enter your API key to continue (or go back and choose another option).");
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

  const title = stepTitle(step, modeChoice);

  const body = (() => {
    switch (step) {
      case 0:
        return (
          <>
            <p>
              <strong>Video to Text</strong> turns video, audio, and links into text files. A short setup
              walks you through tools, transcription, and the queue.
            </p>
            <p className="onboarding-tip">The checklist on the main screen updates as you go.</p>
          </>
        );
      case 1:
        return (
          <>
            <p>
              Choose where <strong>.txt</strong> transcripts are saved. <strong>Documents</strong> is the
              default and matches the checklist when selected.
            </p>
            <label className="field onboarding-field">
              <span>Current folder</span>
              <input
                type="text"
                readOnly
                value={settings.outputDir ?? ""}
                placeholder="Not set"
              />
            </label>
            {outputError ? <p className="onboarding-error">{outputError}</p> : null}
            <div className="onboarding-row">
              <button type="button" disabled={busy} onClick={() => void useDocumentsAsOutput()}>
                Use Documents
              </button>
              <button type="button" disabled={busy} onClick={() => void pickOutputFolder()}>
                Browse…
              </button>
            </div>
            {pathsProbablyEqual(settings.outputDir, documentsPath) ? (
              <p className="onboarding-tip onboarding-success-hint">
                Using your Documents folder — this will show as ready in &quot;Before you start&quot;.
              </p>
            ) : settings.outputDir?.trim() ? (
              <p className="onboarding-tip">Custom folder — it will show as set once you continue.</p>
            ) : (
              <p className="onboarding-tip">Tip: Use Documents unless you need another location.</p>
            )}
          </>
        );
      case 2:
        return (
          <>
            <p>URL downloads need <strong>ffmpeg</strong> and <strong>yt-dlp</strong> on this computer.</p>
            {showManagedToolDownloads ? (
              <div className="onboarding-block">
                <button
                  type="button"
                  className="primary"
                  disabled={toolDlBusy || busy}
                  onClick={() => void onDownloadMediaTools()}
                >
                  {toolDlBusy ? "Downloading…" : "Install ffmpeg & yt-dlp for me"}
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
                    <strong>All set.</strong> ffmpeg and yt-dlp are installed; paths were saved. The checklist
                    above should show them as found.
                  </WizardSuccessBanner>
                ) : null}
                {toolDlError ? <WizardErrorBanner>{toolDlError}</WizardErrorBanner> : null}
                <p className="onboarding-tip">
                  Or install manually and set paths later in <strong>Settings</strong>.
                </p>
              </div>
            ) : (
              <p className="onboarding-tip">
                On Linux install <code>ffmpeg</code> and <code>yt-dlp</code> with your package manager, then
                set paths in <strong>Settings</strong>.
              </p>
            )}
          </>
        );
      case 3:
        return (
          <>
            <p>How should transcripts be produced?</p>
            <div className="onboarding-radio-group" role="radiogroup" aria-label="Transcription mode">
              <label className="onboarding-radio">
                <input
                  type="radio"
                  name="wiz-transcription"
                  checked={modeChoice === "cloud"}
                  onChange={() => setModeChoice("cloud")}
                />
                <span>
                  <strong>Cloud (HTTP API)</strong> — OpenAI-compatible <code>/audio/transcriptions</code>,
                  needs an API key.
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
                  <strong>Local Whisper</strong> — <code>whisper.cpp</code> on this PC, no API key; download
                  a model on the next step.
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
                  <strong>Decide in Settings later</strong> — skip API/model setup for now.
                </span>
              </label>
            </div>
          </>
        );
      case 4:
        if (modeChoice === "cloud") {
          return (
            <>
              <p>Enter your cloud provider details. The key is stored in the OS secure store, not in plain text.</p>
              <label className="field onboarding-field">
                <span>API base URL</span>
                <input
                  type="url"
                  value={settings.apiBaseUrl}
                  onChange={(e) => patchSettings({ apiBaseUrl: e.target.value })}
                  autoComplete="off"
                />
              </label>
              <label className="field onboarding-field">
                <span>Model name</span>
                <input
                  type="text"
                  value={settings.apiModel}
                  onChange={(e) => patchSettings({ apiModel: e.target.value })}
                  autoComplete="off"
                />
              </label>
              <label className="field onboarding-field">
                <span>API key</span>
                <input
                  type="password"
                  autoComplete="off"
                  value={settings.apiKey}
                  onChange={(e) => patchSettings({ apiKey: e.target.value })}
                />
              </label>
              {cloudError ? <p className="onboarding-error">{cloudError}</p> : null}
              <p className="onboarding-tip">
                <strong>OpenAI example:</strong>{" "}
                <a href="https://platform.openai.com/api-keys" target="_blank" rel="noopener noreferrer">
                  API keys
                </a>
                , base <code>https://api.openai.com/v1</code>, model <code>whisper-1</code>.
              </p>
            </>
          );
        }
        if (modeChoice === "local") {
          return (
            <>
              <p className="onboarding-tip onboarding-info-callout">
                <strong>Local Whisper</strong> is separate from ffmpeg above. The app can{" "}
                <strong>download the ggml model</strong> for you. For <code>whisper-cli</code>:{" "}
                <strong>Windows</strong> — official <code>whisper-bin-x64.zip</code> from{" "}
                <a href="https://github.com/ggml-org/whisper.cpp/releases" target="_blank" rel="noopener noreferrer">
                  ggml-org/whisper.cpp
                </a>{" "}
                (use the button below; includes DLLs). <strong>macOS</strong> — no CLI zip in those releases; the
                button looks for Homebrew paths or use <strong>Pick file…</strong> after{" "}
                <code>brew install whisper-cpp</code>. <strong>Linux</strong> — use your package manager or build
                from source, then set the path. You can also put <code>whisper-cli</code> next to the app so the
                checklist finds it without a path.
              </p>

              <div className="onboarding-local-step">
                <p className="onboarding-local-step-title">Step A — whisper-cli</p>
                <p className="onboarding-tip">
                  Optional path if the binary is not next to <code>v2t</code>. Use the setup button (Windows/macOS)
                  or <strong>Pick file…</strong> for <code>whisper-cli.exe</code> / <code>whisper-cli</code>.
                </p>
                <div className="row-gap">
                  {showManagedToolDownloads ? (
                    <button
                      type="button"
                      disabled={whisperCliBusy || busy}
                      onClick={() => void onSetupWhisperCli()}
                    >
                      {whisperCliBusy
                        ? "Working…"
                        : isWin
                          ? "Download whisper-cli for me (Windows)"
                          : "Find Homebrew whisper-cli (macOS)"}
                    </button>
                  ) : null}
                  <button type="button" disabled={busy} onClick={() => void pickWhisperCliExecutable()}>
                    Pick file…
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
                    <strong>whisper-cli path saved.</strong> The checklist should update; continue with the model
                    below.
                  </WizardSuccessBanner>
                ) : null}
                {whisperCliError ? <WizardErrorBanner>{whisperCliError}</WizardErrorBanner> : null}
                <label className="field onboarding-field">
                  <span>Executable path</span>
                  <div className="row-gap">
                    <input
                      type="text"
                      value={settings.whisperCliPath ?? ""}
                      onChange={(e) =>
                        patchSettings({ whisperCliPath: e.target.value.trim() || null })
                      }
                      placeholder="Leave empty if whisper-cli sits next to the app"
                      autoComplete="off"
                    />
                  </div>
                </label>
              </div>

              <div className="onboarding-local-step">
                <p className="onboarding-local-step-title">Step B — model file</p>
                <p className="onboarding-tip">
                  Choose size, then download. We verify SHA-1; when it matches, you will see a green confirmation
                  below and the checklist item <strong>Whisper model (.bin)</strong> turns green.
                </p>
                <label className="field onboarding-field">
                  <span>Folder for .bin files</span>
                  <div className="row-gap">
                    <input
                      type="text"
                      readOnly
                      value={settings.whisperModelsDir ?? ""}
                      placeholder={defaultModelsPath ?? "Default: app data / models"}
                    />
                    <button type="button" disabled={busy} onClick={() => void pickWhisperModelsDir()}>
                      Browse…
                    </button>
                  </div>
                </label>
                <label className="field onboarding-field">
                  <span>Model</span>
                  <select
                    value={settings.whisperModel}
                    onChange={(e) => {
                      patchSettings({ whisperModel: e.target.value });
                      setModelInstallSuccess(false);
                      setModelDlError(null);
                    }}
                  >
                    {whisperModels.map((m) => (
                      <option key={m.id} value={m.id}>
                        {m.id} — ~{m.sizeMib} MiB ({m.fileName})
                      </option>
                    ))}
                  </select>
                </label>
                <button type="button" disabled={modelDlBusy || busy} onClick={() => void onDownloadModel()}>
                  {modelDlBusy ? "Downloading…" : "Download / verify model"}
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
                    <strong>Model ready.</strong> File is on disk and SHA-1 matches. The checklist should show a
                    green dot for <strong>Whisper model (.bin)</strong>.
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
              You can switch between <strong>HTTP API</strong> and <strong>Local Whisper</strong>, set API keys,
              and download models anytime in <strong>Settings</strong>.
            </p>
            <button type="button" className="ghost" disabled={busy} onClick={onOpenSettings}>
              Open Settings now
            </button>
          </>
        );
      case 5:
        return (
          <>
            <p>
              Add files, folders, or paste links into the queue, then press <strong>Start queue</strong>.
            </p>
            <p className="onboarding-tip">
              If something fails, check the log at the bottom and the checklist above.
            </p>
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
          Step {step + 1} of {TOTAL_STEPS}
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
              Back
            </button>
          ) : (
            <span />
          )}
          <div className="onboarding-actions-right">
            <button type="button" className="ghost" disabled={busy} onClick={() => void handleFinish()}>
              Skip setup
            </button>
            {step === 5 ? (
              <button type="button" disabled={busy} onClick={onOpenSettings}>
                Open Settings
              </button>
            ) : null}
            {last ? (
              <button type="button" className="primary" disabled={busy} onClick={() => void handleFinish()}>
                {busy ? "Saving…" : "Done"}
              </button>
            ) : (
              <button
                type="button"
                className="primary"
                disabled={nextDisabled}
                onClick={() => void goNext()}
              >
                Next
              </button>
            )}
          </div>
        </div>
        <button type="button" className="onboarding-close" aria-label="Close" disabled={busy} onClick={onClose}>
          ×
        </button>
      </div>
    </div>
  );
}
