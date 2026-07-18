import { Trans, useTranslation } from "react-i18next";
import type { PrefsTarget } from "../types/preferences";
import type { AppSettings, DependencyReport } from "../types/settings";

type Props = {
  report: DependencyReport | null;
  /** OS Documents path (for “using Documents” hint). */
  documentsPath: string | null;
  settings: Pick<
    AppSettings,
    "outputDir" | "apiKey" | "transcriptionMode" | "whisperCliPath"
  >;
  onOpenPreferences: (target?: PrefsTarget) => void;
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

export function ReadinessPanel({
  report,
  documentsPath,
  settings,
  onOpenPreferences,
}: Props) {
  const { t } = useTranslation("readiness");
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

  function fixTarget(): PrefsTarget {
    if (!outputOk) return { depth: "essentials", focus: "output-dir" };
    if (!toolsUnknown && !ffmpegOk) return { depth: "advanced", focus: "ffmpeg" };
    if (!toolsUnknown && !ytDlpOk) return { depth: "advanced", focus: "yt-dlp" };
    if (useLocal && (!whisperCliOk || !modelOk)) {
      return { depth: "engine", focus: "whisper-model" };
    }
    if (!useLocal && !useBrowser && !apiKeyOk) {
      return { depth: "engine", focus: "api-credentials" };
    }
    return { depth: "essentials" };
  }

  const rows: {
    id: string;
    label: string;
    ok: boolean;
    hint: string;
  }[] = [
    {
      id: "ffmpeg",
      label: t("row.ffmpeg.label"),
      ok: toolsUnknown ? false : ffmpegOk,
      hint: toolsUnknown
        ? t("row.ffmpeg.hint_checking")
        : ffmpegOk
          ? t("row.ffmpeg.hint_ok")
          : t("row.ffmpeg.hint_missing"),
    },
    {
      id: "ytdlp",
      label: t("row.ytdlp.label"),
      ok: toolsUnknown ? false : ytDlpOk,
      hint: toolsUnknown
        ? t("row.ytdlp.hint_checking")
        : ytDlpOk
          ? t("row.ytdlp.hint_ok")
          : t("row.ytdlp.hint_missing"),
    },
    {
      id: "output",
      label: t("row.output.label"),
      ok: outputOk,
      hint: !outputOk
        ? t("row.output.hint_missing")
        : outputIsDocuments
          ? t("row.output.hint_documents")
          : t("row.output.hint_custom"),
    },
  ];

  if (useLocal) {
    rows.push(
      {
        id: "whisper-cli",
        label: t("row.whisper_cli.label"),
        ok: toolsUnknown ? false : whisperCliOk,
        hint: toolsUnknown
          ? t("row.whisper_cli.hint_checking")
          : whisperCliOk
            ? t("row.whisper_cli.hint_ok")
            : t("row.whisper_cli.hint_missing"),
      },
      {
        id: "ggml-model",
        label: t("row.ggml.label"),
        ok: toolsUnknown ? false : modelOk,
        hint: toolsUnknown
          ? t("row.ggml.hint_checking")
          : modelOk
            ? t("row.ggml.hint_ok")
            : t("row.ggml.hint_missing"),
      },
    );
  } else if (useBrowser) {
    rows.push({
      id: "wasm-whisper",
      label: t("row.wasm_whisper.label"),
      ok: true,
      hint: t("row.wasm_whisper.hint"),
    });
  } else {
    rows.push({
      id: "api",
      label: t("row.api_key.label"),
      ok: apiKeyOk,
      hint: apiKeyOk ? t("row.api_key.hint_ok") : t("row.api_key.hint_missing"),
    });
  }

  const rowsList = (
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
  );

  // Always present (sr-only) so automated checks and screen readers see status
  // regardless of whether the panel is collapsed.
  const statusSpans = (
    <>
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
    </>
  );

  const modeLabel = useLocal
    ? t("ready.mode.local", { defaultValue: "On this computer" })
    : useBrowser
      ? t("ready.mode.app", { defaultValue: "Inside the app" })
      : t("ready.mode.online", { defaultValue: "Online service" });

  // All green → collapse to a single satisfied line. The 5-row checklist is
  // onboarding reassurance; once everything's set it shouldn't eat half the
  // window on every launch. Click to re-expand the details.
  if (allOk) {
    return (
      <div
        className="readiness readiness-all-ok readiness-compact"
        data-testid="readiness-panel"
        role="status"
      >
        <span className="readiness-dot readiness-dot-ok" aria-hidden />
        <strong className="readiness-summary-text">
          {t("ready.title", { defaultValue: "Ready" })}
        </strong>
        <span className="readiness-summary-detail">
          {t("ready.detail", {
            mode: modeLabel,
            defaultValue: `New batches use: ${modeLabel}`,
          })}
        </span>
        {statusSpans}
      </div>
    );
  }

  return (
    <section
      className="readiness setup-notice"
      aria-label={t("panel_aria")}
      data-testid="readiness-panel"
    >
      <div className="setup-notice-main">
        <span className="setup-notice-mark" aria-hidden>!</span>
        <div>
          <h2>{t("simple.title")}</h2>
          <p>{t("simple.body")}</p>
        </div>
        <button
          type="button"
          className="readiness-settings-btn"
          data-testid="readiness-open-settings"
          onClick={() => onOpenPreferences(fixTarget())}
        >
          {t("open_settings_btn")}
        </button>
      </div>
      <details className="setup-notice-details">
        <summary>{t("simple.details")}</summary>
        {toolsUnknown ? (
          <p className="readiness-tools-unknown" data-testid="deps-unknown">
            {t("tools_unknown")}
          </p>
        ) : null}
        {rowsList}
        {!toolsUnknown && (!ffmpegOk || !ytDlpOk) ? (
          <p className="readiness-tool-hint" data-testid="readiness-tool-hint">
            <Trans i18nKey="tool_hint" t={t} components={{ strong: <strong /> }} />
          </p>
        ) : null}
      </details>
      {statusSpans}
    </section>
  );
}
