import { open } from "@tauri-apps/plugin-dialog";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Trans, useTranslation } from "react-i18next";
import {
  browserQueueJobFinish,
  cancelQueueJob,
  openSessionLog,
  processQueueItem,
  releaseQueueJobSlot,
  scanMediaFolder,
  sessionLogAppendUi,
} from "../lib/invokeSafe";
import { transcribeBrowserTracks } from "../lib/browserWhisper";
import {
  fileBasenameNoExt,
  newJobId,
  parseInputLines,
  shortLabel,
} from "../lib/queueUtils";
import type {
  JobProgressSnapshot,
  QueueJob,
  SubtaskState,
  SubtaskStatus,
} from "../types/queue";
import type { AppSettings } from "../types/settings";
import { JobProgressBar } from "./JobProgressBar";
import { SubtaskList } from "./SubtaskList";

type Props = {
  settings: AppSettings;
  /** Derived in App from deps + output folder + API key */
  readinessComplete: boolean;
};

const MAX_LOG = 200;

/** Match a log line emitted from the queue-job-progress listener that carries
 * a yt-dlp `[download] N% …` bucket. Used by the "Show download percentages"
 * checkbox to filter out the noisy progress chatter while keeping everything
 * else (item count, extract-audio, errors, etc.) visible. */
const YT_DLP_PERCENT_LINE_RE = /^\[\d{2}:\d{2}:\d{2}\] \[yt-dlp(?:-video)?\] \d+%/;

function IconRevealInFolder({ className }: { className?: string }) {
  return (
    <svg
      className={className}
      viewBox="0 0 24 24"
      fill="none"
      xmlns="http://www.w3.org/2000/svg"
      aria-hidden="true"
    >
      <path
        d="M3 7v10a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2V9a2 2 0 0 0-2-2h-6l-2-2H5a2 2 0 0 0-2 2Z"
        stroke="currentColor"
        strokeWidth="1.75"
        strokeLinejoin="round"
      />
    </svg>
  );
}

function IconOpenFile({ className }: { className?: string }) {
  return (
    <svg
      className={className}
      viewBox="0 0 24 24"
      fill="none"
      xmlns="http://www.w3.org/2000/svg"
      aria-hidden="true"
    >
      <path
        d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8l-6-6Z"
        stroke="currentColor"
        strokeWidth="1.75"
        strokeLinejoin="round"
      />
      <path
        d="M14 2v6h6M8 13h8M8 17h5M8 9h2"
        stroke="currentColor"
        strokeWidth="1.75"
        strokeLinecap="round"
      />
    </svg>
  );
}

export function QueuePanel({ settings, readinessComplete }: Props) {
  const { t } = useTranslation("queue");
  const { recursiveFolderScan } = settings;
  const [urlDraft, setUrlDraft] = useState("");
  const [jobs, setJobs] = useState<QueueJob[]>([]);
  const [logLines, setLogLines] = useState<string[]>([]);
  const [jobProgress, setJobProgress] = useState<
    Record<string, JobProgressSnapshot>
  >({});
  const [logVisible, setLogVisible] = useState(false);
  const [showDownloadPercents, setShowDownloadPercents] = useState(false);
  const runningRef = useRef(false);
  const stopRequestedRef = useRef(false);
  const currentJobIdRef = useRef<string | null>(null);
  const [queueRunning, setQueueRunning] = useState(false);

  const setQueueActive = useCallback((active: boolean) => {
    runningRef.current = active;
    setQueueRunning(active);
  }, []);

  const appendLog = useCallback((line: string) => {
    const ts = new Date().toISOString().slice(11, 19);
    const formatted = `[${ts}] ${line}`;
    setLogLines((prev) => {
      const next = [...prev, formatted];
      return next.length > MAX_LOG ? next.slice(-MAX_LOG) : next;
    });
    void sessionLogAppendUi(line);
  }, []);

  const addJobs = useCallback((incoming: Omit<QueueJob, "id" | "status">[]) => {
    if (incoming.length === 0) return;
    setJobs((prev) => [
      ...prev,
      ...incoming.map((j) => ({ ...j, id: newJobId(), status: "pending" as const })),
    ]);
  }, []);

  const onPathsDropped = useCallback(
    (paths: string[]) => {
      const items = paths.map((source) => ({
        kind: "file" as const,
        source,
        displayLabel: fileBasenameNoExt(source),
      }));
      addJobs(items);
      appendLog(t("msg.added_paths_drop", { count: items.length }));
    },
    [addJobs, appendLog],
  );

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    const ac = new AbortController();
    void import("@tauri-apps/api/webview")
      .then(({ getCurrentWebview }) =>
        getCurrentWebview().onDragDropEvent((event) => {
          if (ac.signal.aborted) return;
          if (event.payload.type === "drop") {
            onPathsDropped(event.payload.paths);
          }
        }),
      )
      .then((fn) => {
        if (!ac.signal.aborted) unlisten = fn;
      })
      .catch(() => {
        /* web / tests without Tauri */
      });
    return () => {
      ac.abort();
      unlisten?.();
    };
  }, [onPathsDropped]);

  useEffect(() => {
    let unlistenProgress: (() => void) | undefined;
    let unlistenPipeline: (() => void) | undefined;
    let unlistenPlaylist: (() => void) | undefined;
    let unlistenSubtask: (() => void) | undefined;
    const ac = new AbortController();
    void import("@tauri-apps/api/event")
      .then(async ({ listen }) => {
        const u1 = await listen<{
          jobId: string;
          phase: string;
          message: string;
          subtaskIndex?: number;
          subtaskTotal?: number;
          subtaskPercent?: number;
        }>("queue-job-progress", (ev) => {
          if (ac.signal.aborted) return;
          const p = ev.payload;
          appendLog(`[${p.phase}] ${p.message}`);
          if (p.jobId) {
            setJobProgress((prev) => ({
              ...prev,
              [p.jobId]: {
                phase: p.phase,
                message: p.message,
                subtaskIndex: p.subtaskIndex,
                subtaskTotal: p.subtaskTotal,
                subtaskPercent: p.subtaskPercent,
              },
            }));
          }
        });
        const u2 = await listen<{
          jobId: string;
          label: string;
          message: string;
        }>("pipeline-log", (ev) => {
          if (ac.signal.aborted) return;
          appendLog(`[${ev.payload.label}] ${ev.payload.message}`);
        });
        const u3 = await listen<{
          jobId: string;
          playlistTitle?: string | null;
          subtasks: Array<{
            id: string;
            index: number;
            title: string;
            originalUrl: string;
          }>;
        }>("playlist-resolved", (ev) => {
          if (ac.signal.aborted) return;
          const { jobId, playlistTitle, subtasks } = ev.payload;
          if (!jobId || !Array.isArray(subtasks) || subtasks.length === 0) return;
          const initial: SubtaskState[] = subtasks.map((s) => ({
            id: s.id,
            index: s.index,
            title: s.title,
            originalUrl: s.originalUrl,
            status: "pending",
          }));
          setJobs((prev) =>
            prev.map((j) =>
              j.id === jobId
                ? {
                    ...j,
                    playlistTitle: playlistTitle ?? undefined,
                    subtasks: initial,
                  }
                : j,
            ),
          );
        });
        const u4 = await listen<{
          jobId: string;
          subtaskIndex: number;
          status: SubtaskStatus;
          reason?: string | null;
        }>("subtask-status", (ev) => {
          if (ac.signal.aborted) return;
          const { jobId, subtaskIndex, status, reason } = ev.payload;
          if (!jobId || !subtaskIndex) return;
          setJobs((prev) =>
            prev.map((j) => {
              if (j.id !== jobId || !j.subtasks) return j;
              return {
                ...j,
                subtasks: j.subtasks.map((s) =>
                  s.index === subtaskIndex
                    ? {
                        ...s,
                        status,
                        reason: reason ?? undefined,
                      }
                    : s,
                ),
              };
            }),
          );
        });
        return [u1, u2, u3, u4] as const;
      })
      .then((tuple) => {
        if (!ac.signal.aborted && tuple) {
          unlistenProgress = tuple[0];
          unlistenPipeline = tuple[1];
          unlistenPlaylist = tuple[2];
          unlistenSubtask = tuple[3];
        }
      })
      .catch(() => {
        /* web / e2e without Tauri */
      });
    return () => {
      ac.abort();
      unlistenProgress?.();
      unlistenPipeline?.();
      unlistenPlaylist?.();
      unlistenSubtask?.();
    };
  }, [appendLog]);

  function addUrlsFromDraft() {
    const lines = parseInputLines(urlDraft);
    if (lines.length === 0) return;
    addJobs(
      lines.map((source) => ({
        kind: "url" as const,
        source,
        displayLabel: shortLabel(source),
      })),
    );
    appendLog(t("msg.added_urls", { count: lines.length }));
    setUrlDraft("");
  }

  async function addFolder() {
    const dir = await open({ directory: true, multiple: false });
    if (typeof dir !== "string" || !dir.trim()) return;
    appendLog(t("msg.scanning_folder", { path: shortLabel(dir, 80) }));
    const files = await scanMediaFolder(dir.trim(), recursiveFolderScan);
    if (!files || files.length === 0) {
      appendLog(t("msg.no_media_found"));
      return;
    }
    addJobs(
      files.map((source) => ({
        kind: "file" as const,
        source,
        displayLabel: fileBasenameNoExt(source),
      })),
    );
    appendLog(t("msg.enqueued_folder", { count: files.length }));
  }

  async function addFilesViaDialog() {
    const picked = await open({
      multiple: true,
      filters: [
        {
          name: "Media",
          extensions: [
            "mp4",
            "mkv",
            "mov",
            "webm",
            "avi",
            "wmv",
            "m4v",
            "mp3",
            "wav",
            "m4a",
            "flac",
            "ogg",
            "opus",
            "aac",
            "wma",
          ],
        },
      ],
    });
    if (picked === null) return;
    const list = Array.isArray(picked) ? picked : [picked];
    if (list.length === 0) return;
    addJobs(
      list.map((source) => ({
        kind: "file" as const,
        source,
        displayLabel: fileBasenameNoExt(source),
      })),
    );
    appendLog(t("msg.added_files_picker", { count: list.length }));
  }

  const cancelPendingSnapshot = useCallback((slice: QueueJob[]) => {
    const ids = new Set(slice.map((j) => j.id));
    setJobs((prev) =>
      prev.map((j) =>
        ids.has(j.id) && j.status === "pending"
          ? { ...j, status: "cancelled" as const, detail: "Cancelled" }
          : j,
      ),
    );
  }, []);

  function stopQueue() {
    if (!runningRef.current) return;
    stopRequestedRef.current = true;
    void cancelQueueJob(currentJobIdRef.current);
    appendLog(t("msg.stop_requested"));
  }

  async function startQueue() {
    if (runningRef.current) {
      appendLog(t("msg.queue_already_running"));
      return;
    }
    const pending = jobs.filter((j) => j.status === "pending");
    if (pending.length === 0) {
      appendLog(t("msg.nothing_to_run"));
      return;
    }
    stopRequestedRef.current = false;
    setQueueActive(true);
    appendLog(t("msg.starting_queue", { count: pending.length }));
    try {
      for (let i = 0; i < pending.length; i++) {
        if (stopRequestedRef.current) {
          cancelPendingSnapshot(pending.slice(i));
          appendLog(t("msg.queue_stopped"));
          break;
        }
        const job = pending[i]!;
        const jobIndex = i + 1;
        setJobs((prev) =>
          prev.map((j) =>
            j.id === job.id ? { ...j, status: "running", detail: undefined } : j,
          ),
        );
        appendLog(t("msg.running", { label: job.displayLabel }));
        currentJobIdRef.current = job.id;
        try {
          const outcome = await processQueueItem({
            jobId: job.id,
            jobIndex,
            source: job.source,
            sourceKind: job.kind === "url" ? "url" : "file",
            displayLabel: job.displayLabel,
            settings,
          });

          let result: { transcriptPath: string; summary: string };
          if (outcome.kind === "browserPrepared") {
            const outDir = settings.outputDir?.trim();
            if (!outDir) {
              throw new Error(t("error.output_dir_required"));
            }
            let texts: string[];
            try {
              texts = await transcribeBrowserTracks({
                whisperModelId: outcome.whisperModelId,
                tracks: outcome.tracks,
                language: outcome.language,
                shouldStop: () => stopRequestedRef.current,
                onProgress: (m) => appendLog(`[browser] ${m}`),
              });
            } catch (e) {
              void releaseQueueJobSlot(job.id);
              const msg =
                e instanceof Error
                  ? e.message
                  : String(e);
              appendLog(`[browser-error] ${msg}`);
              void sessionLogAppendUi(`[browser-error] ${msg}`);
              throw e;
            }
            result = await browserQueueJobFinish({
              jobId: job.id,
              tracks: outcome.tracks,
              texts,
              workDir: outcome.workDir,
              deleteAudioAfter: outcome.deleteAudioAfter,
              outputDir: outDir,
            });
          } else {
            result = outcome;
          }

          setJobs((prev) =>
            prev.map((j) =>
              j.id === job.id
                ? {
                    ...j,
                    status: "done",
                    detail: result.summary,
                    transcriptPath: result.transcriptPath,
                  }
                : j,
            ),
          );
        } catch (err) {
          const detail =
            err instanceof Error ? err.message : t("error.process_failed");
          const cancelled =
            /job cancelled/i.test(detail) || /^cancelled$/i.test(detail);
          setJobs((prev) =>
            prev.map((j) =>
              j.id === job.id
                ? {
                    ...j,
                    status: cancelled ? "cancelled" : "error",
                    detail: cancelled ? t("error.stopped_by_user") : detail,
                  }
                : j,
            ),
          );
          appendLog(
            cancelled
              ? t("msg.stopped_label", { label: job.displayLabel })
              : job.displayLabel !== detail
                ? t("msg.error_label", { detail, label: job.displayLabel })
                : t("msg.error_no_label", { detail }),
          );
        } finally {
          currentJobIdRef.current = null;
        }
      }
    } finally {
      setQueueActive(false);
      stopRequestedRef.current = false;
      appendLog(t("msg.queue_idle"));
    }
  }

  function clearDone() {
    let droppedIds: string[] = [];
    setJobs((prev) => {
      droppedIds = prev.filter((j) => j.status === "done").map((j) => j.id);
      return prev.filter((j) => j.status !== "done");
    });
    if (droppedIds.length) {
      setJobProgress((prev) => {
        const next = { ...prev };
        for (const id of droppedIds) delete next[id];
        return next;
      });
    }
    appendLog(t("msg.cleared_done"));
  }

  function clearAll() {
    if (runningRef.current) return;
    setJobs([]);
    setJobProgress({});
    appendLog(t("msg.cleared_all"));
  }

  function copyLog() {
    void navigator.clipboard.writeText(logLines.join("\n"));
    appendLog(t("msg.log_copied"));
  }

  async function openLogFile() {
    const ok = await openSessionLog();
    if (!ok) {
      appendLog(t("msg.session_log_unavailable"));
    }
  }

  async function revealTranscriptInFolder(path: string) {
    try {
      const { revealItemInDir } = await import("@tauri-apps/plugin-opener");
      await revealItemInDir(path);
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      appendLog(t("error.open_folder", { message: msg }));
    }
  }

  async function openTranscriptFile(path: string) {
    try {
      const { openPath } = await import("@tauri-apps/plugin-opener");
      await openPath(path);
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      appendLog(t("error.open_file", { message: msg }));
    }
  }

  const openSubtaskLink = useCallback(
    async (url: string) => {
      try {
        const { openUrl } = await import("@tauri-apps/plugin-opener");
        await openUrl(url);
      } catch (e) {
        const msg = e instanceof Error ? e.message : String(e);
        appendLog(t("error.open_link", { message: msg }));
      }
    },
    [appendLog, t],
  );

  const retrySubtask = useCallback(
    (subtask: SubtaskState) => {
      const cleanUrl = stripPlaylistParams(subtask.originalUrl);
      addJobs([
        {
          kind: "url" as const,
          source: cleanUrl,
          displayLabel: shortLabel(`Retry: ${subtask.title}`),
        },
      ]);
      appendLog(t("msg.retry_enqueued", { title: subtask.title }));
    },
    [addJobs, appendLog, t],
  );

  const queueEmpty = jobs.length === 0;

  const visibleLogLines = useMemo(() => {
    if (showDownloadPercents) return logLines;
    return logLines.filter((l) => !YT_DLP_PERCENT_LINE_RE.test(l));
  }, [logLines, showDownloadPercents]);

  return (
    <section className="queue-panel" aria-label={t("panel_aria")}>
      <h2>{t("title")}</h2>

      {queueEmpty ? (
        <div
          className={`queue-empty-hint ${readinessComplete ? "queue-empty-hint-ok" : "queue-empty-hint-warn"}`}
          data-testid="queue-empty-hint"
        >
          {readinessComplete ? (
            <Trans i18nKey="empty_hint.ready_when_you_are" t={t} components={{ strong: <strong /> }} />
          ) : (
            <Trans i18nKey="empty_hint.finish_setup_first" t={t} components={{ strong: <strong /> }} />
          )}
        </div>
      ) : null}

      <div
        className="drop-zone"
        data-testid="drop-zone"
        onDragOver={(e) => {
          e.preventDefault();
          e.stopPropagation();
        }}
      >
        <p>{t("drop_zone")}</p>
        <div className="queue-toolbar">
          <button type="button" onClick={() => void addFilesViaDialog()}>
            {t("btn.add_files")}
          </button>
          <button type="button" onClick={() => void addFolder()}>
            {t("btn.add_folder")}
          </button>
        </div>
      </div>

      <label className="field url-field">
        <span>{t("url_field.label")}</span>
        <textarea
          data-testid="url-input"
          value={urlDraft}
          onChange={(e) => setUrlDraft(e.target.value)}
          rows={4}
          placeholder={t("url_field.placeholder")}
        />
      </label>
      <button type="button" data-testid="add-urls" onClick={addUrlsFromDraft}>
        {t("btn.add_urls")}
      </button>

      <div className="queue-run-row">
        <button
          type="button"
          className="primary"
          data-testid="start-queue"
          onClick={() => void startQueue()}
        >
          {t("btn.start")}
        </button>
        <button
          type="button"
          data-testid="stop-queue"
          disabled={!queueRunning}
          onClick={stopQueue}
        >
          {t("btn.stop")}
        </button>
        <button type="button" onClick={clearDone}>
          {t("btn.clear_done")}
        </button>
        <button type="button" onClick={clearAll}>
          {t("btn.clear_all")}
        </button>
      </div>

      <div className="queue-table-wrap">
        <table className="queue-table">
          <thead>
            <tr>
              <th>{t("table.label_header")}</th>
              <th>{t("table.kind_header")}</th>
              <th>{t("table.status_header")}</th>
              <th className="queue-table-actions-head" aria-label={t("table.actions_header")} />
            </tr>
          </thead>
          <tbody>
            {jobs.length === 0 ? (
              <tr>
                <td colSpan={4} className="muted">
                  {t("table.no_jobs")}
                </td>
              </tr>
            ) : (
              jobs.map((j) => {
                const outPath = j.transcriptPath;
                const canOpenResult = j.status === "done" && Boolean(outPath?.trim());
                const progress =
                  j.status === "running" ? jobProgress[j.id] : undefined;
                return (
                  <FragmentRow
                    key={j.id}
                    job={j}
                    progress={progress}
                    canOpenResult={canOpenResult}
                    outPath={outPath}
                    onReveal={revealTranscriptInFolder}
                    onOpen={openTranscriptFile}
                    onOpenSubtaskLink={openSubtaskLink}
                    onRetrySubtask={retrySubtask}
                  />
                );
              })
            )}
          </tbody>
        </table>
      </div>

      <div className="log-panel">
        <div className="log-header">
          <button
            type="button"
            className="log-toggle"
            aria-expanded={logVisible}
            aria-controls="queue-log-body"
            onClick={() => setLogVisible((v) => !v)}
          >
            {logVisible ? t("log.hide") : t("log.show")}
          </button>
          <div className="log-header-actions">
            <label className="log-filter">
              <input
                type="checkbox"
                checked={showDownloadPercents}
                onChange={(e) => setShowDownloadPercents(e.target.checked)}
              />
              <span>{t("log.show_percents")}</span>
            </label>
            <button type="button" onClick={() => void openLogFile()}>
              {t("log.open_file")}
            </button>
            <button type="button" onClick={copyLog}>
              {t("log.copy")}
            </button>
          </div>
        </div>
        <pre
          className={logVisible ? "log-body" : "log-body sr-only"}
          id="queue-log-body"
          data-testid="queue-log"
        >
          {visibleLogLines.length === 0 ? t("log.empty") : visibleLogLines.join("\n")}
        </pre>
      </div>
    </section>
  );
}

type FragmentRowProps = {
  job: QueueJob;
  progress: JobProgressSnapshot | undefined;
  canOpenResult: boolean;
  outPath: string | null | undefined;
  onReveal: (p: string) => void;
  onOpen: (p: string) => void;
  onOpenSubtaskLink: (url: string) => void;
  onRetrySubtask: (subtask: SubtaskState) => void;
};

function FragmentRow({
  job,
  progress,
  canOpenResult,
  outPath,
  onReveal,
  onOpen,
  onOpenSubtaskLink,
  onRetrySubtask,
}: FragmentRowProps) {
  const { t: tQueue } = useTranslation("queue");
  const showProgress = job.status === "running" && progress != null;
  const subtasks = job.subtasks;
  const showSubtasks =
    Array.isArray(subtasks) &&
    subtasks.length > 0 &&
    (job.status === "running" || job.status === "error");
  const headerLabel =
    subtasks && subtasks.length > 0
      ? tQueue("table.playlist_header", {
          title: job.playlistTitle?.trim() || job.displayLabel,
          count: subtasks.length,
        })
      : job.displayLabel;
  return (
    <>
      <tr data-testid="queue-row">
        <td title={job.source}>{headerLabel}</td>
        <td>{job.kind}</td>
        <td>
          <div className="queue-status-cell">
            {job.status === "done" ? (
              <span className="queue-status-check" aria-hidden>
                ✓
              </span>
            ) : null}
            <span data-testid={`job-status-${job.id}`}>{job.status}</span>
          </div>
        </td>
        <td className="queue-table-actions-cell">
          {canOpenResult && outPath ? (
            <div className="queue-table-actions-inner">
              <button
                type="button"
                className="queue-table-action-btn queue-table-action-btn--icon"
                title={tQueue("actions.show_in_folder")}
                aria-label={tQueue("actions.show_in_folder")}
                onClick={() => onReveal(outPath)}
              >
                <IconRevealInFolder className="queue-action-icon" />
              </button>
              <button
                type="button"
                className="queue-table-action-btn queue-table-action-btn--icon"
                title={tQueue("actions.open_file")}
                aria-label={tQueue("actions.open_file")}
                onClick={() => onOpen(outPath)}
              >
                <IconOpenFile className="queue-action-icon" />
              </button>
            </div>
          ) : null}
        </td>
      </tr>
      {showProgress && progress ? (
        <tr className="queue-progress-row" data-testid="queue-progress-row">
          <td colSpan={4}>
            <JobProgressBar progress={progress} />
          </td>
        </tr>
      ) : null}
      {showSubtasks && subtasks ? (
        <tr className="queue-progress-row" data-testid="queue-subtasks-row">
          <td colSpan={4}>
            <SubtaskList
              subtasks={subtasks}
              activeIndex={progress?.subtaskIndex}
              onOpen={onOpenSubtaskLink}
              onRetry={onRetrySubtask}
            />
          </td>
        </tr>
      ) : null}
    </>
  );
}

/** Strip `list=` and friends from a YouTube watch URL so retry never re-fans
 * out into the whole playlist. yt-dlp's `youtube_watch_url_should_use_no_playlist`
 * (pipeline.rs) catches the same case server-side, but doing it here too is
 * defense in depth and gives the user a cleaner-looking URL in the queue. */
export function stripPlaylistParams(url: string): string {
  try {
    const u = new URL(url);
    u.searchParams.delete("list");
    u.searchParams.delete("index");
    u.searchParams.delete("start_radio");
    return u.toString();
  } catch {
    return url;
  }
}
