import type { AppSettings, DependencyReport } from "../types/settings";

type Props = {
  report: DependencyReport | null;
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

export function ReadinessPanel({ report, settings, onOpenSettings }: Props) {
  const toolsUnknown = report === null;
  const ffmpegOk = report?.ffmpegFound ?? false;
  const ytDlpOk = report?.ytDlpFound ?? false;
  const outputOk = Boolean(settings.outputDir?.trim());
  const useLocal = settings.transcriptionMode === "localWhisper";
  const credOk = useLocal
    ? (report?.whisperCliFound ?? false)
    : Boolean(settings.apiKey?.trim());

  const allOk = !toolsUnknown && ffmpegOk && ytDlpOk && outputOk && credOk;

  const rows = [
    {
      id: "ffmpeg",
      label: "ffmpeg",
      ok: toolsUnknown ? false : ffmpegOk,
      hint: toolsUnknown
        ? "Checking… (run inside the desktop app)"
        : ffmpegOk
          ? "Found — audio can be normalized."
          : "Missing — place ffmpeg next to the app or set path in Settings.",
    },
    {
      id: "ytdlp",
      label: "yt-dlp",
      ok: toolsUnknown ? false : ytDlpOk,
      hint: toolsUnknown
        ? "Checking…"
        : ytDlpOk
          ? "Found — URLs can be downloaded."
          : "Missing — place yt-dlp next to the app or set path in Settings.",
    },
    {
      id: "output",
      label: "Output folder",
      ok: outputOk,
      hint: outputOk
        ? "Set — transcripts will save here."
        : "Not set — choose a folder in Settings.",
    },
    useLocal
      ? {
          id: "whisper",
          label: "whisper-cli",
          ok: toolsUnknown ? false : (report?.whisperCliFound ?? false),
          hint: toolsUnknown
            ? "Checking…"
            : report?.whisperCliFound
              ? "Found — local transcription can run offline."
              : "Missing — build whisper.cpp, set path in Settings, or place whisper-cli (or main) next to the app.",
        }
      : {
          id: "api",
          label: "API key",
          ok: credOk,
          hint: credOk
            ? "Set — transcription API is ready."
            : "Missing — add your key in Settings (saved in OS secure storage).",
        },
  ];

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
            : "Complete the items below (we check them for you)."}
        </p>
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
            Tip: open <strong>Settings</strong> — on Windows or macOS you can use{" "}
            <strong>Download ffmpeg &amp; yt-dlp for me</strong>, or place the binaries next to the app
            and set paths under <strong>I’ll install … myself</strong>.
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
