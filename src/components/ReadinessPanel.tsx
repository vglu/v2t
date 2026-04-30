import { Trans, useTranslation } from "react-i18next";
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

  return (
    <section
      className={`readiness ${allOk ? "readiness-all-ok" : "readiness-needs-work"}`}
      aria-label={t("panel_aria")}
      data-testid="readiness-panel"
    >
      <div className="readiness-head">
        <h2 className="readiness-title">{t("title")}</h2>
        <p className="readiness-sub">
          {allOk ? t("sub.all_ok") : t("sub.needs_work")}
        </p>
        {useLocal ? (
          <p className="readiness-mode-hint">
            <Trans i18nKey="mode_hint.local" t={t} components={{ strong: <strong /> }} />
          </p>
        ) : useBrowser ? (
          <p className="readiness-mode-hint">
            <Trans i18nKey="mode_hint.browser" t={t} components={{ strong: <strong /> }} />
          </p>
        ) : (
          <p className="readiness-mode-hint">
            <Trans i18nKey="mode_hint.cloud" t={t} components={{ strong: <strong /> }} />
          </p>
        )}
        {toolsUnknown ? (
          <p className="readiness-tools-unknown" data-testid="deps-unknown">
            {t("tools_unknown")}
          </p>
        ) : null}
        {!allOk ? (
          <button
            type="button"
            className="readiness-settings-btn"
            data-testid="readiness-open-settings"
            onClick={onOpenSettings}
          >
            {t("open_settings_btn")}
          </button>
        ) : null}
        {!toolsUnknown && (!ffmpegOk || !ytDlpOk) ? (
          <p className="readiness-tool-hint" data-testid="readiness-tool-hint">
            <Trans i18nKey="tool_hint" t={t} components={{ strong: <strong /> }} />
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
