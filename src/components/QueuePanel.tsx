import { open } from "@tauri-apps/plugin-dialog";
import { useCallback, useEffect, useRef, useState } from "react";
import {
  cancelQueueJob,
  processQueueItem,
  scanMediaFolder,
} from "../lib/invokeSafe";
import { newJobId, parseInputLines, shortLabel } from "../lib/queueUtils";
import type { QueueJob } from "../types/queue";
import type { AppSettings } from "../types/settings";

type Props = {
  settings: AppSettings;
  /** Derived in App from deps + output folder + API key */
  readinessComplete: boolean;
};

const MAX_LOG = 200;

export function QueuePanel({ settings, readinessComplete }: Props) {
  const { recursiveFolderScan } = settings;
  const [urlDraft, setUrlDraft] = useState("");
  const [jobs, setJobs] = useState<QueueJob[]>([]);
  const [logLines, setLogLines] = useState<string[]>([]);
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
    setLogLines((prev) => {
      const next = [...prev, `[${ts}] ${line}`];
      return next.length > MAX_LOG ? next.slice(-MAX_LOG) : next;
    });
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
        displayLabel: shortLabel(source),
      }));
      addJobs(items);
      appendLog(`Added ${items.length} path(s) from drop`);
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
    const ac = new AbortController();
    void import("@tauri-apps/api/event")
      .then(async ({ listen }) => {
        const u1 = await listen<{ phase: string; message: string }>(
          "queue-job-progress",
          (ev) => {
            if (ac.signal.aborted) return;
            appendLog(`[${ev.payload.phase}] ${ev.payload.message}`);
          },
        );
        const u2 = await listen<{
          jobId: string;
          label: string;
          message: string;
        }>("pipeline-log", (ev) => {
          if (ac.signal.aborted) return;
          appendLog(`[${ev.payload.label}] ${ev.payload.message}`);
        });
        return [u1, u2] as const;
      })
      .then((pair) => {
        if (!ac.signal.aborted && pair) {
          unlistenProgress = pair[0];
          unlistenPipeline = pair[1];
        }
      })
      .catch(() => {
        /* web / e2e without Tauri */
      });
    return () => {
      ac.abort();
      unlistenProgress?.();
      unlistenPipeline?.();
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
    appendLog(`Added ${lines.length} URL(s)`);
    setUrlDraft("");
  }

  async function addFolder() {
    const dir = await open({ directory: true, multiple: false });
    if (typeof dir !== "string" || !dir.trim()) return;
    appendLog(`Scanning folder: ${shortLabel(dir, 80)}`);
    const files = await scanMediaFolder(dir.trim(), recursiveFolderScan);
    if (!files || files.length === 0) {
      appendLog("No media files found (or scan failed)");
      return;
    }
    addJobs(
      files.map((source) => ({
        kind: "file" as const,
        source,
        displayLabel: shortLabel(source),
      })),
    );
    appendLog(`Enqueued ${files.length} file(s) from folder`);
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
        displayLabel: shortLabel(source),
      })),
    );
    appendLog(`Added ${list.length} file(s) from picker`);
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
    appendLog(
      "Stop requested — killing current step if possible; remaining jobs will be cancelled",
    );
  }

  async function startQueue() {
    if (runningRef.current) {
      appendLog("Queue is already running");
      return;
    }
    const pending = jobs.filter((j) => j.status === "pending");
    if (pending.length === 0) {
      appendLog("Nothing to run (no pending jobs)");
      return;
    }
    stopRequestedRef.current = false;
    setQueueActive(true);
    appendLog(`Starting queue (${pending.length} job(s))`);
    try {
      for (let i = 0; i < pending.length; i++) {
        if (stopRequestedRef.current) {
          cancelPendingSnapshot(pending.slice(i));
          appendLog("Queue stopped (remaining jobs cancelled)");
          break;
        }
        const job = pending[i]!;
        const jobIndex = i + 1;
        setJobs((prev) =>
          prev.map((j) =>
            j.id === job.id ? { ...j, status: "running", detail: undefined } : j,
          ),
        );
        appendLog(`Run: ${job.displayLabel}`);
        currentJobIdRef.current = job.id;
        try {
          const result = await processQueueItem({
            jobId: job.id,
            jobIndex,
            source: job.source,
            sourceKind: job.kind === "url" ? "url" : "file",
            displayLabel: job.displayLabel,
            settings,
          });
          setJobs((prev) =>
            prev.map((j) =>
              j.id === job.id
                ? { ...j, status: "done", detail: result.summary }
                : j,
            ),
          );
        } catch (err) {
          const detail =
            err instanceof Error ? err.message : "process_queue_item failed";
          const cancelled =
            /job cancelled/i.test(detail) || /^cancelled$/i.test(detail);
          setJobs((prev) =>
            prev.map((j) =>
              j.id === job.id
                ? {
                    ...j,
                    status: cancelled ? "cancelled" : "error",
                    detail: cancelled ? "Stopped by user" : detail,
                  }
                : j,
            ),
          );
          appendLog(
            cancelled ? `Stopped: ${job.displayLabel}` : `Error: ${job.displayLabel}`,
          );
        } finally {
          currentJobIdRef.current = null;
        }
      }
    } finally {
      setQueueActive(false);
      stopRequestedRef.current = false;
      appendLog("Queue idle");
    }
  }

  function clearDone() {
    setJobs((prev) => prev.filter((j) => j.status !== "done"));
    appendLog("Cleared finished jobs");
  }

  function clearAll() {
    if (runningRef.current) return;
    setJobs([]);
    appendLog("Cleared queue");
  }

  function copyLog() {
    void navigator.clipboard.writeText(logLines.join("\n"));
    appendLog("Log copied to clipboard");
  }

  const queueEmpty = jobs.length === 0;

  return (
    <section className="queue-panel" aria-label="Queue">
      <h2>Queue</h2>

      {queueEmpty ? (
        <div
          className={`queue-empty-hint ${readinessComplete ? "queue-empty-hint-ok" : "queue-empty-hint-warn"}`}
          data-testid="queue-empty-hint"
        >
          {readinessComplete ? (
            <>
              <strong>Ready when you are.</strong> Drop files or folders here, paste video links, or use the
              buttons below — then press <strong>Start queue</strong>.
            </>
          ) : (
            <>
              <strong>Finish setup first.</strong> Follow the checklist above (green dots). Open{" "}
              <strong>Settings</strong> if anything is missing, or use <strong>Setup guide</strong> for a short
              tour.
            </>
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
        <p>Drop files or folders here (native drop uses system paths).</p>
        <div className="queue-toolbar">
          <button type="button" onClick={() => void addFilesViaDialog()}>
            Add files…
          </button>
          <button type="button" onClick={() => void addFolder()}>
            Add folder…
          </button>
        </div>
      </div>

      <label className="field url-field">
        <span>URLs (one per line)</span>
        <textarea
          data-testid="url-input"
          value={urlDraft}
          onChange={(e) => setUrlDraft(e.target.value)}
          rows={4}
          placeholder="https://www.youtube.com/watch?v=…"
        />
      </label>
      <button type="button" data-testid="add-urls" onClick={addUrlsFromDraft}>
        Add URLs
      </button>

      <div className="queue-run-row">
        <button
          type="button"
          className="primary"
          data-testid="start-queue"
          onClick={() => void startQueue()}
        >
          Start queue
        </button>
        <button
          type="button"
          data-testid="stop-queue"
          disabled={!queueRunning}
          onClick={stopQueue}
        >
          Stop queue
        </button>
        <button type="button" onClick={clearDone}>
          Clear done
        </button>
        <button type="button" onClick={clearAll}>
          Clear all
        </button>
      </div>

      <div className="queue-table-wrap">
        <table className="queue-table">
          <thead>
            <tr>
              <th>Label</th>
              <th>Kind</th>
              <th>Status</th>
            </tr>
          </thead>
          <tbody>
            {jobs.length === 0 ? (
              <tr>
                <td colSpan={3} className="muted">
                  No jobs yet
                </td>
              </tr>
            ) : (
              jobs.map((j) => (
                <tr key={j.id} data-testid="queue-row">
                  <td title={j.source}>{j.displayLabel}</td>
                  <td>{j.kind}</td>
                  <td data-testid={`job-status-${j.id}`}>{j.status}</td>
                </tr>
              ))
            )}
          </tbody>
        </table>
      </div>

      <div className="log-panel">
        <div className="log-header">
          <span>Log</span>
          <button type="button" onClick={copyLog}>
            Copy
          </button>
        </div>
        <pre className="log-body" data-testid="queue-log">
          {logLines.length === 0 ? "…" : logLines.join("\n")}
        </pre>
      </div>
    </section>
  );
}
