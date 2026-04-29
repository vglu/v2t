import type { JobProgressSnapshot } from "../types/queue";

type Props = {
  progress: JobProgressSnapshot;
};

/** Compact one-line progress indicator: bar + N/M + percent + raw tail (speed/ETA). */
export function JobProgressBar({ progress }: Props) {
  const { phase, message, subtaskIndex, subtaskTotal, subtaskPercent } = progress;
  const hasSubtask = subtaskIndex != null && subtaskTotal != null;
  const percent = subtaskPercent ?? 0;
  // yt-dlp short_message: "5% of 45.67MiB at 1.50MiB/s ETA 00:30" — strip the
  // leading "N% " so we don't repeat the percent we already render separately.
  const tail = message.replace(/^\d+%\s*/, "").trim();

  return (
    <div className="job-progress" data-testid="job-progress">
      <div className="job-progress-meta">
        <span className="job-progress-phase">{phase}</span>
        {hasSubtask ? (
          <span className="job-progress-counter">
            {subtaskIndex}/{subtaskTotal}
          </span>
        ) : null}
        {subtaskPercent != null ? (
          <span className="job-progress-percent">{percent}%</span>
        ) : null}
        {tail ? <span className="job-progress-tail">{tail}</span> : null}
      </div>
      <progress
        className="job-progress-bar"
        max={100}
        value={percent}
        aria-label={`${phase} progress`}
      />
    </div>
  );
}
