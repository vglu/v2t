import type { DependencyReport } from "../types/settings";

type Props = {
  report: DependencyReport | null;
};

export function DependencyBar({ report }: Props) {
  if (!report) {
    return (
      <div className="deps deps-unknown" data-testid="deps-unknown">
        Tools: unknown (run inside Tauri to detect ffmpeg / yt-dlp)
      </div>
    );
  }
  const ok = report.ffmpegFound && report.ytDlpFound;
  return (
    <div
      className={ok ? "deps deps-ok" : "deps deps-bad"}
      data-testid="deps-status"
    >
      <span data-testid="ffmpeg-status">
        ffmpeg: {report.ffmpegFound ? "ok" : "missing"}
      </span>
      {" · "}
      <span data-testid="ytdlp-status">
        yt-dlp: {report.ytDlpFound ? "ok" : "missing"}
      </span>
      {" · "}
      <span data-testid="whisper-status">
        whisper-cli: {report.whisperCliFound ? "ok" : "missing"}
      </span>
    </div>
  );
}
