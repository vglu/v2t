import { useEffect, useState } from "react";

const STEPS = [
  {
    title: "Welcome",
    body: (
      <>
        <p>
          <strong>Video to Text</strong> turns video, audio, and links into text files. You only need a few
          one-time setup steps.
        </p>
        <p className="onboarding-tip">Use the checklist on the main screen — it updates automatically.</p>
      </>
    ),
  },
  {
    title: "ffmpeg & yt-dlp",
    body: (
      <>
        <p>
          On <strong>Windows</strong> or <strong>macOS</strong>, open <strong>Settings</strong> and use{" "}
          <strong>Download ffmpeg &amp; yt-dlp for me</strong> — or put the binaries next to the app (e.g.{" "}
          <code>v2t.exe</code> / <code>v2t</code>, or a <code>bin</code> folder). On Linux, install via your
          package manager and paste full paths under <strong>I’ll install … myself</strong>.
        </p>
        <p className="onboarding-tip">
          Output folder: use <strong>Use Documents</strong> in Settings or pick any folder with Browse.
        </p>
      </>
    ),
  },
  {
    title: "Cloud API or local Whisper",
    body: (
      <>
        <p>
          In <strong>Settings → Transcription &amp; models</strong>, choose <strong>HTTP API</strong> (cloud,
          needs API key) or <strong>Local Whisper</strong> (offline with whisper.cpp — pick a model size and
          press <strong>Download / verify model</strong>).
        </p>
        <p>
          For the cloud, add an <strong>API key</strong> from a provider with OpenAI-compatible{" "}
          <code>/audio/transcriptions</code>. The key is stored in OS secure storage.
        </p>
        <p className="onboarding-tip">
          <strong>OpenAI example:</strong>{" "}
          <a href="https://platform.openai.com/api-keys" target="_blank" rel="noopener noreferrer">
            API keys
          </a>
          , base <code>https://api.openai.com/v1</code>, model <code>whisper-1</code>.
        </p>
      </>
    ),
  },
  {
    title: "Run jobs",
    body: (
      <>
        <p>
          Drop files onto the queue area, paste YouTube links, or use <strong>Add files</strong> /{" "}
          <strong>Add folder</strong>. Then press <strong>Start queue</strong>.
        </p>
        <p className="onboarding-tip">If something fails, check the log at the bottom and the checklist above.</p>
      </>
    ),
  },
] as const;

type Props = {
  open: boolean;
  onOpenSettings: () => void;
  onFinish: () => void | Promise<void>;
  /** Close overlay only (wizard may show again on next launch). */
  onClose: () => void;
};

export function OnboardingWizard({ open, onOpenSettings, onFinish, onClose }: Props) {
  const [step, setStep] = useState(0);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (open) setStep(0);
  }, [open]);

  if (!open) return null;

  const last = step >= STEPS.length - 1;

  async function handleFinish() {
    setBusy(true);
    try {
      await onFinish();
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="onboarding-backdrop" role="presentation">
      <div
        className="onboarding-modal"
        role="dialog"
        aria-modal="true"
        aria-labelledby="onboarding-title"
        data-testid="onboarding-wizard"
      >
        <p className="onboarding-step-label">
          Step {step + 1} of {STEPS.length}
        </p>
        <h2 id="onboarding-title" className="onboarding-modal-title">
          {STEPS[step]!.title}
        </h2>
        <div className="onboarding-body">{STEPS[step]!.body}</div>
        <div className="onboarding-actions">
          {step > 0 ? (
            <button type="button" disabled={busy} onClick={() => setStep((s) => s - 1)}>
              Back
            </button>
          ) : (
            <span />
          )}
          <div className="onboarding-actions-right">
            <button type="button" className="ghost" disabled={busy} onClick={() => void handleFinish()}>
              Skip setup
            </button>
            {step === 2 ? (
              <button type="button" disabled={busy} onClick={onOpenSettings}>
                Open Settings
              </button>
            ) : null}
            {last ? (
              <button type="button" className="primary" disabled={busy} onClick={() => void handleFinish()}>
                {busy ? "Saving…" : "Done"}
              </button>
            ) : (
              <button type="button" className="primary" disabled={busy} onClick={() => setStep((s) => s + 1)}>
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
