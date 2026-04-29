import type { SubtaskState } from "../types/queue";
import { SubtaskRow } from "./SubtaskRow";

type Props = {
  subtasks: SubtaskState[];
  /** 1-based active item from `queue-job-progress.subtaskIndex`. */
  activeIndex?: number;
  onOpen: (url: string) => void;
  onRetry: (subtask: SubtaskState) => void;
};

/** Per-video rows for a resolved YouTube playlist job. Renders nothing for
 * non-playlist jobs (single videos, local files) — they have no subtasks. */
export function SubtaskList({ subtasks, activeIndex, onOpen, onRetry }: Props) {
  if (subtasks.length === 0) return null;
  return (
    <ul className="subtask-list" data-testid="subtask-list">
      {subtasks.map((s) => (
        <SubtaskRow
          key={s.id}
          subtask={s}
          isActive={activeIndex === s.index}
          onOpen={onOpen}
          onRetry={onRetry}
        />
      ))}
    </ul>
  );
}
