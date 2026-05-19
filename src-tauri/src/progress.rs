//! Progress event sink: decouples deep pipeline code from `tauri::AppHandle::emit`.
//!
//! Today's only implementation is [`TauriSink`], which preserves the legacy
//! behaviour — emit to the webview *and* append to the session log. New sinks
//! (broadcast for SSE, webhook for HTTP API, fan-out) will plug in here without
//! touching `pipeline.rs` / `job.rs` / `*_download.rs` again.

use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::session_log;
use crate::yt_dlp_metadata::PlaylistResolvedPayload;

/// Owned, cheaply-cloned handle to a sink.
pub type SinkHandle = Arc<dyn ProgressSink>;

pub trait ProgressSink: Send + Sync {
    fn emit(&self, event: JobEvent);
}

// ---------- Wire-format payloads ----------
// Each variant has its own struct so serde produces *exactly* the JSON the
// JS side already listens for (event name + camelCase fields, no extra keys).

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PipelineLogEvent {
    pub job_id: String,
    pub label: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueJobProgressEvent {
    pub job_id: String,
    pub phase: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtask_index: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtask_total: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtask_percent: Option<u8>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubtaskStatusEvent {
    pub job_id: String,
    pub subtask_index: u32,
    pub status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelDownloadEvent {
    pub model_id: String,
    pub phase: String,
    pub bytes_received: u64,
    pub total_bytes: Option<u64>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolDownloadEvent {
    pub tool: String,
    pub phase: String,
    pub bytes_received: u64,
    pub total_bytes: Option<u64>,
    pub message: String,
}

impl ToolDownloadEvent {
    /// Used by `whisper_bottle_macos`; on Windows/Linux that module is `cfg`'d out, so the
    /// constructor appears unused there.
    #[allow(dead_code)]
    pub fn new(
        tool: impl Into<String>,
        phase: impl Into<String>,
        bytes_received: u64,
        total_bytes: Option<u64>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            tool: tool.into(),
            phase: phase.into(),
            bytes_received,
            total_bytes,
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum JobEvent {
    PipelineLog(PipelineLogEvent),
    QueueJobProgress(QueueJobProgressEvent),
    SubtaskStatus(SubtaskStatusEvent),
    PlaylistResolved(PlaylistResolvedPayload),
    ModelDownload(ModelDownloadEvent),
    ToolDownload(ToolDownloadEvent),
}

// ---------- TauriSink: bit-for-bit reproduction of the legacy behaviour ----------

pub struct TauriSink {
    app: AppHandle,
}

impl TauriSink {
    pub fn new(app: AppHandle) -> Self {
        Self { app }
    }

    pub fn handle(app: AppHandle) -> SinkHandle {
        Arc::new(Self::new(app))
    }
}

impl ProgressSink for TauriSink {
    fn emit(&self, event: JobEvent) {
        match event {
            JobEvent::PipelineLog(p) => {
                let _ = self.app.emit("pipeline-log", &p);
                session_log::try_append(&self.app, Some(&p.job_id), &p.label, &p.message);
            }
            JobEvent::QueueJobProgress(p) => {
                let _ = self.app.emit("queue-job-progress", &p);
                session_log::try_append(&self.app, Some(&p.job_id), &p.phase, &p.message);
            }
            JobEvent::SubtaskStatus(p) => {
                let log_msg = match &p.reason {
                    Some(r) if !r.is_empty() => {
                        format!("subtask {}: {} ({r})", p.subtask_index, p.status)
                    }
                    _ => format!("subtask {}: {}", p.subtask_index, p.status),
                };
                let job_id = p.job_id.clone();
                let _ = self.app.emit("subtask-status", &p);
                session_log::try_append(&self.app, Some(&job_id), "subtask", &log_msg);
            }
            JobEvent::PlaylistResolved(p) => {
                // Legacy: no session_log entry for this event; UI is the only consumer.
                let _ = self.app.emit("playlist-resolved", &p);
            }
            JobEvent::ModelDownload(p) => {
                let _ = self.app.emit("model-download-progress", &p);
            }
            JobEvent::ToolDownload(p) => {
                let _ = self.app.emit("tool-download-progress", &p);
            }
        }
    }
}
