import type { SubtaskState, SubtaskStatus } from "../types/queue";

type Props = {
  subtask: SubtaskState;
  /** True when this subtask is the currently downloading / transcribing item.
   * Pending rows for the active subtask render as ▶ even before the backend
   * sends an explicit `running` status (yt-dlp item event flows through the
   * progress bar; we mirror it here). */
  isActive: boolean;
  onOpen: (url: string) => void;
  onRetry: (subtask: SubtaskState) => void;
};

const STATUS_ICON: Record<SubtaskStatus, string> = {
  pending: "⏸",
  running: "▶",
  done: "✓",
  skipped: "⏭",
  error: "✗",
};

export function SubtaskRow({ subtask, isActive, onOpen, onRetry }: Props) {
  const effectiveStatus: SubtaskStatus =
    subtask.status === "pending" && isActive ? "running" : subtask.status;
  const icon = STATUS_ICON[effectiveStatus];
  const reason = subtask.reason?.trim();

  return (
    <li
      className={`subtask-row subtask-row--${effectiveStatus}`}
      data-testid={`subtask-row-${subtask.index}`}
    >
      <span className="subtask-icon" aria-hidden>
        {icon}
      </span>
      <span className="subtask-index">{subtask.index}.</span>
      <button
        type="button"
        className="subtask-link"
        title={subtask.originalUrl}
        onClick={() => onOpen(subtask.originalUrl)}
      >
        {subtask.title}
      </button>
      {reason ? (
        <span className="subtask-reason" title={reason}>
          {reason}
        </span>
      ) : null}
      {effectiveStatus === "error" ? (
        <button
          type="button"
          className="subtask-retry"
          aria-label="Retry this video"
          title="Retry this video"
          onClick={() => onRetry(subtask)}
        >
          ↻ retry
        </button>
      ) : null}
    </li>
  );
}
