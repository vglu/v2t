import { open } from "@tauri-apps/plugin-dialog";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  browserQueueJobFinish,
  cancelQueueJob,
  acquireMediaItem,
  transcribeArtifact,
  discardArtifact,
  estimateOcrBatch,
  openSessionLog,
  releaseQueueJobSlot,
  saveFailedLinks,
  scanMediaFolder,
  sessionLogAppendUi,
} from "../lib/invokeSafe";
import {
  fileBasenameNoExt,
  isUnsupportedMediaUrl,
  newJobId,
  parseInputLines,
  shortLabel,
} from "../lib/queueUtils";
import { SPEECH_LANGUAGE_OPTIONS } from "../lib/speechLanguages";
import { reconcileProfileId } from "../lib/profiles";
import type {
  JobProgressSnapshot,
  QueueItem,
  SubtaskState,
  SubtaskStatus,
} from "../types/queue";
import { itemStatus } from "../types/queue";
import type { AppSettings, CostEstimate } from "../types/settings";
import { JobProgressBar } from "./JobProgressBar";
import { SubtaskList } from "./SubtaskList";

type Props = {
  settings: AppSettings;
  /** Derived in App from deps + output folder + API key */
  readinessComplete: boolean;
  onOpenOutputSettings?: () => void;
  /** Persist quick Defaults toggles (VTT / video / audio) immediately. */
  onPersistSettings?: (next: AppSettings) => void | Promise<void>;
};

const MAX_LOG = 200;

function shortOutputFolder(path: string | null | undefined, fallback: string): string {
  const trimmed = path?.trim();
  if (!trimmed) return fallback;
  const parts = trimmed.replace(/\\/g, "/").split("/").filter(Boolean);
  if (parts.length === 0) return fallback;
  if (parts.length === 1) return parts[0];
  return parts.slice(-2).join("\\");
}

function joinPaths(a: string, b: string): string {
  const left = a.trim().replace(/[\\\/]+$/, "");
  const right = b.trim().replace(/^[\\\/]+/, "");
  return `${left}\\${right}`;
}

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

export function QueuePanel({
  settings,
  readinessComplete,
  onOpenOutputSettings,
  onPersistSettings,
}: Props) {
  const { t } = useTranslation("queue");
  const { recursiveFolderScan } = settings;
  const [urlDraft, setUrlDraft] = useState("");
  const [sourceMode, setSourceMode] = useState<"links" | "files">("links");
  const [batchLanguage, setBatchLanguage] = useState<string | null>(
    settings.language,
  );
  const [items, setItems] = useState<QueueItem[]>([]);
  const [logLines, setLogLines] = useState<string[]>([]);
  const [itemProgress, setItemProgress] = useState<
    Record<string, JobProgressSnapshot>
  >({});
  const [logVisible, setLogVisible] = useState(false);
  const [showDownloadPercents, setShowDownloadPercents] = useState(false);
  const stopDownloadsRef = useRef(false);
  const stopTranscriptionRef = useRef(false);
  const [queueRunning, setQueueRunning] = useState(false);
  const [ocrEstimate, setOcrEstimate] = useState<CostEstimate | null>(null);
  const [brokenLinksPath, setBrokenLinksPath] = useState("");

  const setQueueActive = useCallback((active: boolean) => {
    setQueueRunning(active);
  }, []);

  useEffect(() => {
    const IMAGE_DOC_EXTS = new Set([
      "jpg", "jpeg", "png", "gif", "webp", "bmp", "tiff", "tif", "pdf", "docx",
    ]);
    function isVisionSource(src: string) {
      const ext = src.split(".").pop()?.toLowerCase() ?? "";
      return IMAGE_DOC_EXTS.has(ext);
    }
    const ocrSources = items
      .filter((j) => j.acquire.status === "pending" && isVisionSource(j.source))
      .map((j) => j.source);
    if (ocrSources.length === 0 || settings.visionMode !== "gemini") {
      setOcrEstimate(null);
      return;
    }
    estimateOcrBatch(ocrSources).then((est) => setOcrEstimate(est ?? null));
  }, [items, settings.visionMode]);

  const appendLog = useCallback((line: string) => {
    const ts = new Date().toISOString().slice(11, 19);
    const formatted = `[${ts}] ${line}`;
    setLogLines((prev) => {
      const next = [...prev, formatted];
      return next.length > MAX_LOG ? next.slice(-MAX_LOG) : next;
    });
    void sessionLogAppendUi(line);
  }, []);

  useEffect(() => {
    setBatchLanguage(settings.language);
  }, [settings.language]);

  const addItems = useCallback(
    (
      incoming: Array<{
        kind: QueueItem["kind"];
        source: string;
        displayLabel: string;
      }>,
    ) => {
      if (incoming.length === 0) return;
      setItems((prev) => [
        ...prev,
        ...incoming.map((j) => {
          const unsupported =
            j.kind === "url" && isUnsupportedMediaUrl(j.source);
          return {
            id: newJobId(),
            ...j,
            language: batchLanguage,
            acquire: {
              status: unsupported ? ("unsupported" as const) : ("pending" as const),
              attempts: 0,
              detail: unsupported ? "Unsupported" : undefined,
            },
            transcribe: {
              status: unsupported ? ("cancelled" as const) : ("pending" as const),
              attempts: 0,
              detail: unsupported ? "Cancelled" : undefined,
            },
            transcriptPath: undefined,
            subtasks: undefined,
            artifact: undefined,
            browserPrepared: undefined,
          } as QueueItem;
        }),
      ]);
    },
    [batchLanguage],
  );

  const changeBatchLanguage = useCallback((next: string | null) => {
    // Only affects newly added items. Already queued rows keep their own language.
    setBatchLanguage(next);
  }, []);

  const changeItemLanguage = useCallback((itemId: string, next: string | null) => {
    setItems((prev) =>
      prev.map((it) => {
        const canEdit =
          it.acquire.status !== "running" &&
          it.transcribe.status !== "running" &&
          it.transcribe.status !== "done" &&
          it.acquire.status !== "unsupported";
        if (!canEdit) return it;
        if (it.id !== itemId) return it;
        return { ...it, language: next };
      }),
    );
  }, []);

  const onPathsDropped = useCallback(
    (paths: string[]) => {
      const incoming = paths.map((source) => ({
        kind: "file" as const,
        source,
        displayLabel: fileBasenameNoExt(source),
      }));
      addItems(incoming);
      appendLog(t("msg.added_paths_drop", { count: incoming.length }));
    },
    [addItems, appendLog],
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
            setItemProgress((prev) => ({
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
          setItems((prev) => {
            const it = prev.find((x) => x.id === jobId);
            if (it) {
              it.playlistTitle = playlistTitle ?? undefined;
              it.subtasks = initial;
            }
            return [...prev];
          });
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
          setItems((prev) => {
            const it = prev.find((x) => x.id === jobId);
            if (it?.subtasks) {
              it.subtasks = it.subtasks.map((s) =>
                s.index === subtaskIndex
                  ? {
                      ...s,
                      status,
                      reason: reason ?? undefined,
                    }
                  : s,
              );
            }
            return [...prev];
          });
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
    addItems(
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
    addItems(
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
    addItems(
      list.map((source) => ({
        kind: "file" as const,
        source,
        displayLabel: fileBasenameNoExt(source),
      })),
    );
    appendLog(t("msg.added_files_picker", { count: list.length }));
  }

  const cancelAcquirePendingSnapshot = useCallback((slice: QueueItem[]) => {
    const ids = new Set(slice.map((j) => j.id));
    setItems((prev) => {
      for (const it of prev) {
        if (!ids.has(it.id)) continue;
        if (it.acquire.status === "pending" || it.acquire.status === "waiting") {
          it.acquire.status = "cancelled";
          it.acquire.detail = "Cancelled";
        }
      }
      return [...prev];
    });
  }, []);

  const cancelTranscribePendingSnapshot = useCallback((slice: QueueItem[]) => {
    const ids = new Set(slice.map((j) => j.id));
    setItems((prev) => {
      for (const it of prev) {
        if (!ids.has(it.id)) continue;
        if (it.transcribe.status === "pending") {
          it.transcribe.status = "cancelled";
          it.transcribe.detail = "Cancelled";
        }
      }
      return [...prev];
    });
  }, []);

  function stopDownloads() {
    if (!queueRunning) return;
    stopDownloadsRef.current = true;

    // Cancel running acquire stage tokens (cooperative cancellation).
    for (const it of items) {
      if (it.acquire.status === "running") {
        void cancelQueueJob(`${it.id}:acquire`);
      }
    }
    cancelAcquirePendingSnapshot(items);
    appendLog(t("msg.stop_requested"));
  }

  function stopTranscription() {
    if (!queueRunning) return;
    stopTranscriptionRef.current = true;

    for (const it of items) {
      if (it.transcribe.status === "running") {
        void cancelQueueJob(`${it.id}:transcribe`);
      }
    }
    cancelTranscribePendingSnapshot(
      items.filter((it) => it.acquire.status === "done"),
    );
    appendLog(t("msg.stop_requested"));
  }

  function stopAll() {
    if (!queueRunning) return;
    stopDownloadsRef.current = true;
    stopTranscriptionRef.current = true;

    const runningIds = new Set<string>();
    for (const it of items) {
      if (it.acquire.status === "running" || it.transcribe.status === "running") {
        runningIds.add(it.id);
      }
    }
    for (const id of runningIds) {
      void cancelQueueJob(id);
    }
    cancelAcquirePendingSnapshot(items);
    cancelTranscribePendingSnapshot(items.filter((it) => it.acquire.status === "done"));
    appendLog(t("msg.stop_requested"));
  }

  async function startQueue() {
    // Start both loops together; they run independently and stop buttons
    // target different stage tokens.
    if (queueRunning) {
      appendLog(t("msg.queue_already_running"));
      return;
    }

    const pendingAcquire = items.filter(
      (it) => it.acquire.status === "pending" || it.acquire.status === "waiting",
    );
    if (pendingAcquire.length === 0) {
      appendLog(t("msg.nothing_to_run"));
      return;
    }

    stopDownloadsRef.current = false;
    stopTranscriptionRef.current = false;
    setQueueActive(true);
    appendLog(t("msg.starting_queue", { count: pendingAcquire.length }));

    // Same array reference the runners mutate; must be visible in `finally`
    // so Stop/cancel can discard leftover temp work dirs.
    const runnerItems = items;

    try {
      const { runAcquireLoop, runTranscribeLoop, PendingArtifactsBackpressure } =
        await import("../lib/queueRunners");
      const backpressure = PendingArtifactsBackpressure.fromSettings(settings);

      const onUpdate = () => setItems([...runnerItems]);
      const onStageError = (
        item: QueueItem,
        stage: "acquire" | "transcribe",
        message: string,
      ) => {
        appendLog(`Error [${stage}]: ${message} — ${item.displayLabel}`);
      };

      const invoke = {
        acquireMediaItem,
        transcribeArtifact,
        browserQueueJobFinish,
        releaseQueueJobSlot,
      };

      await Promise.allSettled([
        runAcquireLoop({
          items: runnerItems,
          settings,
          invoke,
          shouldStop: () => stopDownloadsRef.current,
          backpressure,
          onUpdate,
          onStageError,
        }),
        runTranscribeLoop({
          items: runnerItems,
          settings,
          invoke,
          shouldStop: () => stopTranscriptionRef.current,
          backpressure,
          onUpdate,
          onStageError,
        }),
      ]);
    } finally {
      // Drop temp work dirs for items that will not be transcribed (Stop /
      // cancel). Successful jobs already delete work_dir in Rust; clearAll also
      // discards. Without this, Stop left pending acquire→transcribe artifacts.
      const leftovers = runnerItems.filter(
        (it) =>
          !!it.artifact &&
          (it.transcribe.status === "cancelled" ||
            it.acquire.status === "cancelled" ||
            it.acquire.status === "error"),
      );
      for (const it of leftovers) {
        if (!it.artifact) continue;
        try {
          await discardArtifact(it.artifact);
        } catch {
          // Best-effort: do not block queue idle on a stuck temp lock.
        }
        it.artifact = undefined;
      }
      if (leftovers.length) {
        setItems([...runnerItems]);
      }

      setQueueActive(false);
      stopDownloadsRef.current = false;
      stopTranscriptionRef.current = false;
      appendLog(t("msg.queue_idle"));
    }
  }

  function clearDone() {
    let droppedIds: string[] = [];
    setItems((prev) => {
      droppedIds = prev
        .filter((j) => j.transcribe.status === "done")
        .map((j) => j.id);
      return prev.filter((j) => j.transcribe.status !== "done");
    });
    if (droppedIds.length) {
      setItemProgress((prev) => {
        const next = { ...prev };
        for (const id of droppedIds) delete next[id];
        return next;
      });
    }
    appendLog(t("msg.cleared_done"));
  }

  async function clearAll() {
    if (queueRunning) return;

    const toDiscard =
      items.filter((it) => it.artifact && it.transcribe.status !== "done");
    for (const it of toDiscard) {
      if (!it.artifact) continue;
      await discardArtifact(it.artifact);
    }

    setItems([]);
    setItemProgress({});
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

  const retryAcquireStage = useCallback(
    (item: QueueItem) => {
      const staleArtifact = item.artifact;
      if (staleArtifact) {
        void discardArtifact(staleArtifact).catch(() => {
          // Best-effort: temp may already be gone.
        });
      }

      setItems((prev) => {
        const it = prev.find((x) => x.id === item.id);
        if (!it) return prev;

        it.acquire.status = "pending";
        it.acquire.detail = undefined;
        it.acquire.failureKind = undefined;
        // Spec: retry for acquire resets attempts.
        it.acquire.attempts = 0;

        // Restart stage B from scratch after a successful re-acquire.
        it.transcribe.status = "pending";
        it.transcribe.detail = undefined;
        it.transcriptPath = null;
        it.artifact = undefined;
        it.browserPrepared = undefined;
        return [...prev];
      });
      appendLog(t("msg.retry_enqueued", { title: item.displayLabel }));
    },
    [appendLog, t],
  );

  const retryTranscribeStage = useCallback(
    (item: QueueItem) => {
      setItems((prev) => {
        const it = prev.find((x) => x.id === item.id);
        if (!it) return prev;

        it.transcribe.status = "pending";
        it.transcribe.detail = undefined;
        it.transcriptPath = null;

        if (it.subtasks) {
          it.subtasks = it.subtasks.map((s) => ({
            ...s,
            status: "pending",
            reason: undefined,
          }));
        }

        return [...prev];
      });
      appendLog(t("msg.retry_enqueued", { title: item.displayLabel }));
    },
    [appendLog, t],
  );

  const visibleLogLines = useMemo(() => {
    if (showDownloadPercents) return logLines;
    return logLines.filter((l) => !YT_DLP_PERCENT_LINE_RE.test(l));
  }, [logLines, showDownloadPercents]);

  const pendingUrlCount = useMemo(
    () =>
      urlDraft
        .split(/\r?\n/)
        .map((line) => line.trim())
        .filter(Boolean).length,
    [urlDraft],
  );

  const failedUrlItems = useMemo(
    () =>
      items.filter(
        (it) => it.acquire.status === "error" && it.kind === "url",
      ),
    [items],
  );
  const failedUrlCount = failedUrlItems.length;

  const unsupportedUrlItems = useMemo(
    () =>
      items.filter(
        (it) => it.acquire.status === "unsupported" && it.kind === "url",
      ),
    [items],
  );

  const saveBrokenLinks = useCallback(async () => {
    const outDir = settings.outputDir?.trim();
    if (!outDir) {
      appendLog(t("error.output_dir_required"));
      return;
    }
    if (failedUrlItems.length === 0) return;

    // Keep order as in the queue; backend will de-dupe and normalize.
    const failedUrls = failedUrlItems.map((j) => j.source);
    const unsupportedUrls = unsupportedUrlItems.map((j) => j.source);

    const defaultLinksFile = joinPaths(outDir, "broken_links.txt");
    const linksFilePath = brokenLinksPath.trim() || defaultLinksFile;

    appendLog(t("msg.broken_links_saving", { file: linksFilePath }));
    const savedCount = await saveFailedLinks({
      linksFilePath,
      failedUrls,
      unsupportedUrls,
      copyToDir: outDir,
    });
    const n = typeof savedCount === "number" ? savedCount : 0;
    appendLog(t("msg.broken_links_saved", { count: n, file: linksFilePath }));
  }, [
    appendLog,
    brokenLinksPath,
    failedUrlItems,
    saveFailedLinks,
    settings.outputDir,
    t,
    unsupportedUrlItems,
  ]);

  const addUrlsLabel =
    pendingUrlCount <= 0
      ? t("btn.add_urls")
      : pendingUrlCount === 1
        ? t("btn.add_item")
        : t("btn.add_items", { count: pendingUrlCount });

  const outputFormatChips = [
    {
      id: "txt",
      label: "TXT",
      kind: "txt" as const,
      pressed: true,
      disabled: true,
      title: t("profile.chip_txt_always"),
    },
    {
      id: "vtt",
      label: "VTT",
      kind: "vtt" as const,
      pressed: settings.exportWebVtt,
      disabled: !onPersistSettings,
      title: t("profile.chip_vtt"),
    },
    {
      id: "video",
      label: "Video",
      kind: "video" as const,
      pressed: settings.keepDownloadedVideo,
      disabled: !onPersistSettings,
      title: t("profile.chip_video"),
    },
    {
      id: "audio",
      label: "Audio",
      kind: "audio" as const,
      pressed: settings.keepDownloadedAudio,
      disabled: !onPersistSettings,
      title: t("profile.chip_audio"),
    },
    {
      id: "info",
      label: "Info",
      kind: "info" as const,
      pressed: settings.keepMediaInfo,
      disabled: !onPersistSettings,
      title: t("profile.chip_info"),
    },
  ];

  const toggleOutputChip = useCallback(
    (id: "vtt" | "video" | "audio" | "info") => {
      if (!onPersistSettings) return;
      const next: AppSettings = { ...settings };
      if (id === "vtt") {
        next.exportWebVtt = !settings.exportWebVtt;
        // Diarize needs WebVTT timings — drop the flag when VTT is off.
        if (!next.exportWebVtt) {
          next.diarizeSpeakers = false;
          next.labelSpeakers = false;
        }
      } else if (id === "video") {
        next.keepDownloadedVideo = !settings.keepDownloadedVideo;
      } else if (id === "audio") {
        next.keepDownloadedAudio = !settings.keepDownloadedAudio;
      } else {
        next.keepMediaInfo = !settings.keepMediaInfo;
      }
      void onPersistSettings(reconcileProfileId(next));
    },
    [onPersistSettings, settings],
  );

  const outputFolderLabel = shortOutputFolder(
    settings.outputDir,
    t("steps.result.not_set"),
  );

  const languageSelect = (
    <label className="quick-language">
      <span>{t("language.label")}</span>
      <select
        data-testid="batch-language-select"
        aria-label={t("language.label")}
        value={batchLanguage ?? ""}
        onChange={(event) => changeBatchLanguage(event.target.value || null)}
      >
        {SPEECH_LANGUAGE_OPTIONS.map(({ value, key, defaultLabel }) => (
          <option key={value || "auto"} value={value}>
            {t(`language.options.${key}`, { defaultValue: defaultLabel })}
          </option>
        ))}
      </select>
    </label>
  );

  return (
    <section
      className="queue-panel"
      aria-label={t("panel_aria")}
      data-testid="queue-panel"
      data-queue-running={queueRunning ? "true" : "false"}
    >
      <header className="queue-intro">
        <h2>{t("workspace.title")}</h2>
        <p>{t("workspace.subtitle")}</p>
      </header>

      <section className="source-composer">
        <div className="source-mode-switch" role="tablist">
          <button
            type="button"
            role="tab"
            aria-selected={sourceMode === "links"}
            className={sourceMode === "links" ? "source-mode--active" : ""}
            onClick={() => setSourceMode("links")}
          >
            {t("steps.source.links")}
          </button>
          <button
            type="button"
            role="tab"
            aria-selected={sourceMode === "files"}
            className={sourceMode === "files" ? "source-mode--active" : ""}
            onClick={() => setSourceMode("files")}
          >
            {t("steps.source.files")}
          </button>
        </div>

        <div hidden={sourceMode !== "links"} className="source-panel">
          <label className="field url-field">
            <span className="sr-only">{t("url_field.label")}</span>
            <textarea
              data-testid="url-input"
              value={urlDraft}
              onChange={(e) => setUrlDraft(e.target.value)}
              rows={3}
              placeholder={t("url_field.placeholder")}
            />
          </label>
        </div>

        <div
          hidden={sourceMode !== "files"}
          className="drop-zone source-panel"
          data-testid="drop-zone"
          onDragOver={(e) => {
            e.preventDefault();
            e.stopPropagation();
          }}
        >
          <p>{t("drop_zone")}</p>
        </div>

        <div className="source-composer-actions">
          {languageSelect}
          {sourceMode === "links" ? (
            <button
              type="button"
              className="primary motion-button motion-button--add"
              data-testid="add-urls"
              onClick={addUrlsFromDraft}
            >
              {addUrlsLabel}
            </button>
          ) : (
            <div className="queue-toolbar">
              <button
                type="button"
                className="motion-button"
                onClick={() => void addFilesViaDialog()}
              >
                {t("btn.add_files")}
              </button>
              <button
                type="button"
                className="motion-button"
                onClick={() => void addFolder()}
              >
                {t("btn.add_folder")}
              </button>
            </div>
          )}
        </div>
      </section>

      <div className="batch-profile-bar" data-testid="batch-profile-bar">
        <strong className="batch-profile-label">{t("profile.title")}</strong>
        <div className="batch-profile-facts">
          <div className="batch-profile-fact">
            <span>{t("profile.save_to")}</span>
            <strong title={settings.outputDir ?? undefined}>
              {outputFolderLabel}
            </strong>
          </div>
          <div className="batch-profile-fact batch-profile-fact--formats">
            <span>{t("profile.output")}</span>
            <div
              className="format-plaques"
              role="group"
              aria-label={t("profile.output")}
            >
              {outputFormatChips.map((chip) => (
                <button
                  key={chip.id}
                  type="button"
                  className={[
                    "format-plaque",
                    `format-plaque--${chip.kind}`,
                    chip.pressed ? "format-plaque--on" : "format-plaque--off",
                  ].join(" ")}
                  aria-pressed={chip.pressed}
                  disabled={chip.disabled}
                  title={chip.title}
                  data-testid={`output-chip-${chip.id}`}
                  onClick={() => {
                    if (chip.id === "txt") return;
                    toggleOutputChip(chip.id as "vtt" | "video" | "audio" | "info");
                  }}
                >
                  {chip.label}
                </button>
              ))}
            </div>
          </div>
        </div>
        {onOpenOutputSettings ? (
          <button
            type="button"
            className="profile-change-button"
            onClick={onOpenOutputSettings}
          >
            {t("profile.change")}
          </button>
        ) : null}
      </div>

      {ocrEstimate !== null && ocrEstimate.imageCount > 0 && (
        <div className="ocr-cost-badge">
          🖼 {ocrEstimate.imageCount} image{ocrEstimate.imageCount !== 1 ? "s" : ""}/doc
          {settings.geminiModel === "gemini-2.5-flash-lite"
            ? ` — est. $${ocrEstimate.flashLiteCostUsd.toFixed(4)}`
            : ` — est. $${ocrEstimate.flashCostUsd.toFixed(4)}`}
          {settings.geminiFreeTier &&
            ` | ~${Math.ceil(ocrEstimate.freeTierSeconds / 60)} min on free tier`}
        </div>
      )}

      <section className="batch-workspace">
        <div className="batch-workspace-head">
          <div>
            <h3>
              {t("steps.queue.title")}
              {items.length > 0 ? (
                <span className="batch-count"> · {items.length}</span>
              ) : null}
            </h3>
            <p>
              {items.length === 0
                ? t("steps.start.empty")
                : t("steps.start.count", { count: items.length })}
            </p>
          </div>
          <div className="queue-run-row">
            <button
              type="button"
              className="primary motion-button motion-button--start"
              data-testid="start-queue"
              disabled={
                !readinessComplete ||
                queueRunning ||
                items.filter(
                  (it) =>
                    it.acquire.status === "pending" ||
                    it.acquire.status === "waiting",
                ).length === 0
              }
              onClick={() => void startQueue()}
            >
              {t("btn.start")}
            </button>

            {queueRunning ? (
              <>
                <button
                  type="button"
                  data-testid="stop-downloads"
                  onClick={stopDownloads}
                >
                  {t("btn.stop_downloads")}
                </button>
                <button
                  type="button"
                  data-testid="stop-transcription"
                  onClick={stopTranscription}
                >
                  {t("btn.stop_transcription")}
                </button>
                <button type="button" data-testid="stop-all" onClick={stopAll}>
                  {t("btn.stop_all")}
                </button>
              </>
            ) : null}

            {failedUrlCount > 0 ? (
              <div className="broken-links-save">
                <input
                  className="broken-links-input"
                  type="text"
                  value={brokenLinksPath}
                  placeholder={t("broken_links.path_placeholder")}
                  onChange={(e) => setBrokenLinksPath(e.target.value)}
                  aria-label={t("broken_links.path_label")}
                />
                <button
                  type="button"
                  className="quiet-action"
                  data-testid="save-broken-links"
                  onClick={() => void saveBrokenLinks()}
                >
                  {t("btn.save_broken_links")}
                </button>
              </div>
            ) : null}

            {items.some((it) => ["done", "error", "cancelled"].includes(itemStatus(it))) ? (
              <button type="button" className="quiet-action" onClick={clearDone}>
                {t("btn.clear_done")}
              </button>
            ) : null}

            {items.length > 0 ? (
              <button
                type="button"
                className="quiet-action"
                onClick={() => void clearAll()}
              >
                {t("btn.clear_all")}
              </button>
            ) : null}
          </div>
        </div>

        {items.length > 0 ? (
          <div className="queue-lanes">
            <div className="queue-lane queue-lane--downloads">
              <h4>{t("queue_lanes.download_title")}</h4>
              {items.some((it) => it.acquire.status !== "done") ? (
                <div className="queue-table-wrap">
                  <table className="queue-table">
                    <thead>
                      <tr>
                        <th>{t("table.label_header")}</th>
                        <th>{t("table.kind_header")}</th>
                        <th>
                          {t("table.language_header", { defaultValue: "Language" })}
                        </th>
                        <th>{t("table.status_header")}</th>
                        <th
                          className="queue-table-actions-head"
                          aria-label={t("table.actions_header")}
                        />
                      </tr>
                    </thead>
                    <tbody>
                      {items
                        .filter((it) => it.acquire.status !== "done")
                        .map((it) => (
                          <QueueItemRow
                            key={it.id}
                            item={it}
                            lane="download"
                            progress={
                              it.acquire.status === "running"
                                ? itemProgress[it.id]
                                : undefined
                            }
                            canOpenResult={false}
                            outPath={it.transcriptPath}
                            onReveal={revealTranscriptInFolder}
                            onOpen={openTranscriptFile}
                            onLanguageChange={changeItemLanguage}
                            onOpenSubtaskLink={openSubtaskLink}
                            onRetryStage={retryAcquireStage}
                            onRetrySubtask={() => retryTranscribeStage(it)}
                          />
                        ))}
                    </tbody>
                  </table>
                </div>
              ) : (
                <div className="batch-empty-state">
                  <span>{t("steps.queue.empty")}</span>
                </div>
              )}
            </div>

            <div className="queue-lane queue-lane--transcribe">
              <h4>{t("queue_lanes.transcription_title")}</h4>
              {items.some((it) => it.acquire.status === "done") ? (
                <div className="queue-table-wrap">
                  <table className="queue-table">
                    <thead>
                      <tr>
                        <th>{t("table.label_header")}</th>
                        <th>{t("table.kind_header")}</th>
                        <th>
                          {t("table.language_header", { defaultValue: "Language" })}
                        </th>
                        <th>{t("table.status_header")}</th>
                        <th
                          className="queue-table-actions-head"
                          aria-label={t("table.actions_header")}
                        />
                      </tr>
                    </thead>
                    <tbody>
                      {items
                        .filter((it) => it.acquire.status === "done")
                        .map((it) => {
                          const outPath = it.transcriptPath;
                          const canOpenResult =
                            it.transcribe.status === "done" &&
                            Boolean(outPath?.trim());
                          const rawProgress =
                            it.transcribe.status === "running"
                              ? itemProgress[it.id]
                              : undefined;
                          // Ignore leftover download-phase snapshots so the bar
                          // does not stick at yt-dlp 100% while whisper starts.
                          const progress =
                            rawProgress &&
                            rawProgress.phase !== "yt-dlp" &&
                            rawProgress.phase !== "yt-dlp-video" &&
                            rawProgress.phase !== "ffmpeg" &&
                            rawProgress.phase !== "audio-save" &&
                            rawProgress.phase !== "prepare"
                              ? rawProgress
                              : undefined;
                          return (
                            <QueueItemRow
                              key={it.id}
                              item={it}
                              lane="transcribe"
                              progress={progress}
                              canOpenResult={canOpenResult}
                              outPath={outPath}
                              onReveal={revealTranscriptInFolder}
                              onOpen={openTranscriptFile}
                              onLanguageChange={changeItemLanguage}
                              onOpenSubtaskLink={openSubtaskLink}
                              onRetryStage={retryTranscribeStage}
                              onRetrySubtask={() => retryTranscribeStage(it)}
                            />
                          );
                        })}
                    </tbody>
                  </table>
                </div>
              ) : (
                <div className="batch-empty-state">
                  <span>{t("steps.queue.empty")}</span>
                </div>
              )}
            </div>
          </div>
        ) : (
          <div className="batch-empty-state">
            <span>{t("steps.queue.empty")}</span>
          </div>
        )}
      </section>

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
          {logVisible || logLines.length > 0 ? (
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
          ) : null}
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

type QueueItemRowProps = {
  item: QueueItem;
  lane: "download" | "transcribe";
  progress: JobProgressSnapshot | undefined;
  canOpenResult: boolean;
  outPath: string | null | undefined;
  onReveal: (p: string) => void;
  onOpen: (p: string) => void;
  onLanguageChange: (itemId: string, language: string | null) => void;
  onOpenSubtaskLink: (url: string) => void;
  onRetryStage: (item: QueueItem) => void;
  onRetrySubtask: () => void;
};

function QueueItemRow({
  item,
  lane,
  progress,
  canOpenResult,
  outPath,
  onReveal,
  onOpen,
  onLanguageChange,
  onOpenSubtaskLink,
  onRetryStage,
  onRetrySubtask,
}: QueueItemRowProps) {
  const { t: tQueue } = useTranslation("queue");

  const status = itemStatus(item);
  const showDoneCheck = status === "done";

  const showWaitingForTranscription =
    lane === "transcribe" &&
    item.acquire.status === "done" &&
    item.transcribe.status === "pending";

  const showProgress =
    showWaitingForTranscription ||
    (lane === "download"
      ? item.acquire.status === "running" && progress != null
      : item.transcribe.status === "running" && progress != null);

  const displayProgress: JobProgressSnapshot | undefined = showWaitingForTranscription
    ? {
        phase: "queue",
        message: tQueue("progress.waiting_transcription", {
          defaultValue: "Waiting for transcription…",
        }),
      }
    : progress;

  const subtasks = item.subtasks;
  const showSubtasks =
    lane === "transcribe" &&
    Array.isArray(subtasks) &&
    subtasks.length > 0 &&
    (item.transcribe.status === "running" || item.transcribe.status === "error");

  const headerLabel =
    subtasks && subtasks.length > 0
      ? tQueue("table.playlist_header", {
          title: item.playlistTitle?.trim() || item.displayLabel,
          count: subtasks.length,
        })
      : item.displayLabel;

  const retryThisStage =
    lane === "download"
      ? item.acquire.status === "error"
      : item.transcribe.status === "error";

  const disableLanguageSelect =
    item.acquire.status === "running" ||
    item.transcribe.status === "running" ||
    item.transcribe.status === "done" ||
    item.acquire.status === "unsupported";

  return (
    <>
      <tr data-testid="queue-row">
        <td title={item.source}>{headerLabel}</td>
        <td>{item.kind}</td>
        <td>
          <select
            className="queue-language-select"
            aria-label={tQueue("language.item_aria", {
              title: headerLabel,
              defaultValue: `Video language for ${headerLabel}`,
            })}
            value={item.language ?? ""}
            disabled={disableLanguageSelect}
            onChange={(event) =>
              onLanguageChange(item.id, event.target.value || null)
            }
          >
            {SPEECH_LANGUAGE_OPTIONS.map(({ value, key, defaultLabel }) => (
              <option key={value || "auto"} value={value}>
                {tQueue(`language.options.${key}`, {
                  defaultValue: defaultLabel,
                })}
              </option>
            ))}
          </select>
        </td>
        <td>
          <div className="queue-status-cell">
            {showDoneCheck ? (
              <span className="queue-status-check" aria-hidden>
                ✓
              </span>
            ) : null}
            <span data-testid={`job-status-${item.id}`}>{status}</span>
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

          {retryThisStage ? (
            <div className="queue-table-actions-inner">
              <button
                type="button"
                className="queue-table-action-btn queue-table-action-btn--icon"
                title={tQueue("subtask.retry_title")}
                aria-label={tQueue("subtask.retry_aria")}
                onClick={() => onRetryStage(item)}
              >
                ↻
              </button>
            </div>
          ) : null}
        </td>
      </tr>

      {showProgress && displayProgress ? (
        <tr className="queue-progress-row" data-testid="queue-progress-row">
          <td colSpan={5}>
            <JobProgressBar progress={displayProgress} />
          </td>
        </tr>
      ) : null}

      {showSubtasks && subtasks ? (
        <tr className="queue-progress-row" data-testid="queue-subtasks-row">
          <td colSpan={5}>
            <SubtaskList
              subtasks={subtasks}
              activeIndex={progress?.subtaskIndex}
              onOpen={onOpenSubtaskLink}
              onRetry={() => onRetrySubtask()}
            />
          </td>
        </tr>
      ) : null}
    </>
  );
}

/** Strip `list=` from a YouTube watch URL so retry enqueues a single video,
 * not the whole playlist again. */
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
