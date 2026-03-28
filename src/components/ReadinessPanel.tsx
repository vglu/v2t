import type { AppSettings, DependencyReport } from "../types/settings";

type Props = {
  report: DependencyReport | null;
  /** OS Documents path (for “using Documents” hint). */
  documentsPath: string | null;
  settings: Pick<
    AppSettings,
    "outputDir" | "apiKey" | "transcriptionMode" | "whisperCliPath"
  >;
  onOpenSettings: () => void;
};

function StatusDot({ ok }: { ok: boolean }) {
  return (
    <span className={`readiness-dot ${ok ? "readiness-dot-ok" : "readiness-dot-bad"}`} aria-hidden />
  );
}

function pathsProbablyEqual(a: string | null | undefined, b: string | null | undefined): boolean {
  const x = a?.trim();
  const y = b?.trim();
  if (!x || !y) return false;
  return x.replace(/\\/g, "/").toLowerCase() === y.replace(/\\/g, "/").toLowerCase();
}

export function ReadinessPanel({ report, documentsPath, settings, onOpenSettings }: Props) {
  const toolsUnknown = report === null;
  const ffmpegOk = report?.ffmpegFound ?? false;
  const ytDlpOk = report?.ytDlpFound ?? false;
  const outputOk = Boolean(settings.outputDir?.trim());
  const outputIsDocuments = pathsProbablyEqual(settings.outputDir, documentsPath);

  const useLocal = settings.transcriptionMode === "localWhisper";
  const useBrowser = settings.transcriptionMode === "browserWhisper";

  const whisperCliOk = !toolsUnknown && (report?.whisperCliFound ?? false);
  const modelOk = !toolsUnknown && (report?.whisperModelReady ?? false);
  const apiKeyOk = Boolean(settings.apiKey?.trim());

  const toolsReady = !toolsUnknown && ffmpegOk && ytDlpOk;
  const transcriptionReady = useLocal
    ? whisperCliOk && modelOk
    : useBrowser
      ? true
      : apiKeyOk;

  const allOk = toolsReady && outputOk && transcriptionReady;

  const rows: {
    id: string;
    label: string;
    ok: boolean;
    hint: string;
  }[] = [
    {
      id: "ffmpeg",
      label: "ffmpeg",
      ok: toolsUnknown ? false : ffmpegOk,
      hint: toolsUnknown
        ? "Checking… (run inside the desktop app)"
        : ffmpegOk
          ? "Found — audio can be normalized."
          : "Missing — install from Settings or place next to the app.",
    },
    {
      id: "ytdlp",
      label: "yt-dlp",
      ok: toolsUnknown ? false : ytDlpOk,
      hint: toolsUnknown
        ? "Checking…"
        : ytDlpOk
          ? "Found — URLs can be downloaded."
          : "Missing — install from Settings or place next to the app.",
    },
    {
      id: "output",
      label: "Output folder",
      ok: outputOk,
      hint: !outputOk
        ? "Not set — choose a folder in the setup guide or Settings."
        : outputIsDocuments
          ? "Set — using Documents (default). Transcripts save here."
          : "Set — transcripts will save here.",
    },
  ];

  if (useLocal) {
    rows.push(
      {
        id: "whisper-cli",
        label: "whisper-cli (executable)",
        ok: toolsUnknown ? false : whisperCliOk,
        hint: toolsUnknown
          ? "Checking…"
          : whisperCliOk
            ? "Found — local engine ready."
            : "Missing — pick whisper-cli in Settings (or put whisper-cli / main next to the app).",
      },
      {
        id: "ggml-model",
        label: "Whisper model (.bin)",
        ok: toolsUnknown ? false : modelOk,
        hint: toolsUnknown
          ? "Checking…"
          : modelOk
            ? "Verified on disk (SHA-1) — ready for offline transcription."
            : "Missing or checksum mismatch — use Download / verify model in Settings or setup.",
      },
    );
  } else if (useBrowser) {
    rows.push({
      id: "wasm-whisper",
      label: "In-app Whisper (WASM)",
      ok: true,
      hint: "Runs in the app (Transformers.js). First job may download the model; no API key or whisper-cli.",
    });
  } else {
    rows.push({
      id: "api",
      label: "Cloud API key",
      ok: apiKeyOk,
      hint: apiKeyOk
        ? "Saved in OS secure storage — cloud transcription ready."
        : "Missing — add your key in the setup guide or Settings (not used for Local Whisper).",
    });
  }

  return (
    <section
      className={`readiness ${allOk ? "readiness-all-ok" : "readiness-needs-work"}`}
      aria-label="Setup checklist"
      data-testid="readiness-panel"
    >
      <div className="readiness-head">
        <h2 className="readiness-title">Before you start</h2>
        <p className="readiness-sub">
          {allOk
            ? "All set — add files or URLs below and press Start queue."
            : "Complete the checklist (we detect tools and files automatically)."}
        </p>
        {useLocal ? (
          <p className="readiness-mode-hint">
            Mode: <strong>Local Whisper</strong> — cloud API key is not required.
          </p>
        ) : useBrowser ? (
          <p className="readiness-mode-hint">
            Mode: <strong>In-app Whisper</strong> — WASM in the UI layer; no API key or whisper-cli (ffmpeg / yt-dlp
            still required for URLs).
          </p>
        ) : (
          <p className="readiness-mode-hint">
            Mode: <strong>Cloud API</strong> — whisper-cli and ggml model are not required.
          </p>
        )}
        {toolsUnknown ? (
          <p className="readiness-tools-unknown" data-testid="deps-unknown">
            Tools: unknown (run inside the desktop app to detect ffmpeg / yt-dlp)
          </p>
        ) : null}
        {!allOk ? (
          <button type="button" className="readiness-settings-btn" onClick={onOpenSettings}>
            Open Settings
          </button>
        ) : null}
        {!toolsUnknown && (!ffmpegOk || !ytDlpOk) ? (
          <p className="readiness-tool-hint" data-testid="readiness-tool-hint">
            Tip: in <strong>Settings</strong> — on Windows or macOS use{" "}
            <strong>Download ffmpeg &amp; yt-dlp for me</strong>, or install manually.
          </p>
        ) : null}
      </div>
      <ul className="readiness-list">
        {rows.map((row) => (
          <li key={row.id} className="readiness-row">
            <StatusDot ok={row.ok} />
            <div className="readiness-row-text">
              <span className="readiness-row-label">{row.label}</span>
              <span className="readiness-row-hint">{row.hint}</span>
            </div>
          </li>
        ))}
      </ul>
      <span
        className={`sr-only deps ${
          toolsUnknown ? "deps-unknown" : ffmpegOk && ytDlpOk ? "deps-ok" : "deps-bad"
        }`}
        data-testid="deps-status"
      />
      <span className="sr-only" data-testid="ffmpeg-status">
        {toolsUnknown ? "unknown" : ffmpegOk ? "ok" : "missing"}
      </span>
      <span className="sr-only" data-testid="ytdlp-status">
        {toolsUnknown ? "unknown" : ytDlpOk ? "ok" : "missing"}
      </span>
    </section>
  );
}
