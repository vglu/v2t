import { useTranslation } from "react-i18next";
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

/** Distinguishes the subtitle fast-path from a Whisper transcription. The
 * backend signals it by setting `reason` to "from subs (<lang>)" on the final
 * `done` event; we swap the ✓ icon for 📝 so the user can see at a glance which
 * tracks were resolved without running Whisper. */
const SUBS_REASON_RE = /^from subs\b/i;

export function SubtaskRow({ subtask, isActive, onOpen, onRetry }: Props) {
  const { t } = useTranslation("queue");
  const effectiveStatus: SubtaskStatus =
    subtask.status === "pending" && isActive ? "running" : subtask.status;
  const reason = subtask.reason?.trim();
  const fromSubs = effectiveStatus === "done" && reason != null && SUBS_REASON_RE.test(reason);
  const icon = fromSubs ? "📝" : STATUS_ICON[effectiveStatus];

  return (
    <li
      className={`subtask-row subtask-row--${effectiveStatus}${fromSubs ? " subtask-row--from-subs" : ""}`}
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
          aria-label={t("subtask.retry_aria")}
          title={t("subtask.retry_title")}
          onClick={() => onRetry(subtask)}
        >
          {t("subtask.retry_label")}
        </button>
      ) : null}
    </li>
  );
}
