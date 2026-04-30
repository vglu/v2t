import { useTranslation } from "react-i18next";
import type { DependencyReport } from "../types/settings";

type Props = {
  report: DependencyReport | null;
};

export function DependencyBar({ report }: Props) {
  const { t } = useTranslation("readiness");
  if (!report) {
    return (
      <div className="deps deps-unknown" data-testid="deps-unknown">
        {t("deps_bar.tools_unknown")}
      </div>
    );
  }
  const ok = report.ffmpegFound && report.ytDlpFound;
  const okLabel = t("deps_bar.status_ok");
  const missingLabel = t("deps_bar.status_missing");
  const noLabel = t("deps_bar.status_no");
  return (
    <div
      className={ok ? "deps deps-ok" : "deps deps-bad"}
      data-testid="deps-status"
    >
      <span data-testid="ffmpeg-status">
        {t("deps_bar.ffmpeg_label")} {report.ffmpegFound ? okLabel : missingLabel}
      </span>
      {" · "}
      <span data-testid="ytdlp-status">
        {t("deps_bar.ytdlp_label")} {report.ytDlpFound ? okLabel : missingLabel}
      </span>
      {" · "}
      <span data-testid="whisper-status">
        {t("deps_bar.whisper_label")} {report.whisperCliFound ? okLabel : missingLabel}
      </span>
      {" · "}
      <span data-testid="whisper-model-status">
        {t("deps_bar.ggml_label")} {report.whisperModelReady ? okLabel : noLabel}
      </span>
    </div>
  );
}
