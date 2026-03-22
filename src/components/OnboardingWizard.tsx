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
          Put <strong>ffmpeg</strong> and <strong>yt-dlp</strong> in the <strong>same folder</strong> as this
          app (next to <code>v2t.exe</code> on Windows, or <code>v2t</code> on Mac). A <code>bin</code>{" "}
          subfolder also works.
        </p>
        <p className="onboarding-tip">If they are installed elsewhere, you can paste full paths in Settings.</p>
      </>
    ),
  },
  {
    title: "Output folder & API",
    body: (
      <>
        <p>
          Open <strong>Settings</strong> and choose an <strong>output folder</strong> (where <code>.txt</code>{" "}
          files go).
        </p>
        <p>
          Add an <strong>API key</strong> from a provider that offers OpenAI-compatible{" "}
          <code>/audio/transcriptions</code>. The key is stored in your OS secure storage, not in a plain file.
        </p>
        <p className="onboarding-tip">
          <strong>Example (OpenAI):</strong>{" "}
          <a href="https://platform.openai.com/api-keys" target="_blank" rel="noopener noreferrer">
            platform.openai.com/api-keys
          </a>{" "}
          → create a secret key; keep base URL <code>https://api.openai.com/v1</code> and model{" "}
          <code>whisper-1</code> unless your account uses something else. Other clouds (e.g. Azure OpenAI,
          Groq) give keys in their own portals — paste those and match URL/model to their docs.
        </p>
        <p className="onboarding-tip">In Settings, open “Where do I get an API key?” for a longer checklist.</p>
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
