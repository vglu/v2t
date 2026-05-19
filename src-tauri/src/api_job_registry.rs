//! In-memory state for REST-submitted jobs (M2 wave).
//!
//! Lifetime is tied to the running app: state is lost on restart, which is the
//! intentional MVP scope per the project decisions. SQLite persistence comes in M4.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use chrono::{DateTime, Utc};
use serde::Serialize;
use tokio::sync::broadcast;
use utoipa::ToSchema;

use crate::progress::{JobEvent, ProgressSink};

/// Capacity of the per-job SSE broadcast channel. Slow subscribers that lag
/// further than this many events get a `Lagged` error and resync from the
/// current snapshot — preferable to unbounded buffering.
const BROADCAST_CAPACITY: usize = 64;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ApiJobStatus {
    Queued,
    Running,
    Done,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ApiJobProgress {
    pub phase: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtask_index: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtask_total: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtask_percent: Option<u8>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ApiJobCallback {
    pub url: String,
    /// Optional shared secret used to sign webhook deliveries with HMAC-SHA256.
    #[serde(skip_serializing)]
    pub secret: Option<String>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ApiJob {
    pub id: String,
    pub status: ApiJobStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub source: String,
    pub source_kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progress: Option<ApiJobProgress>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transcript_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Membership in a batch submitted via `POST /v1/batches`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub batch_id: Option<String>,
    /// Callback metadata is kept so the webhook dispatcher can read it without
    /// holding a write lock for the whole delivery flow.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub callback: Option<ApiJobCallback>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ApiJobEvent {
    /// Snapshot of the job after a state change. Subscribers receive a series
    /// of these; UI logic stays the same as polling.
    Snapshot(ApiJob),
    /// Convenience signal that the channel is about to close (terminal status).
    /// SSE handlers can break their loop on this without re-checking status.
    Terminal,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ApiBatchSnapshot {
    pub id: String,
    pub created_at: DateTime<Utc>,
    pub total: usize,
    pub queued: usize,
    pub running: usize,
    pub done: usize,
    pub failed: usize,
    pub cancelled: usize,
    pub job_ids: Vec<String>,
}

#[derive(Debug)]
struct ApiBatch {
    id: String,
    created_at: DateTime<Utc>,
    job_ids: Vec<String>,
}

struct ApiJobEntry {
    job: ApiJob,
    /// Live updates for SSE subscribers. Kept alive for the lifetime of the
    /// entry; the registry never removes entries in M3 (in-memory scope).
    sender: broadcast::Sender<ApiJobEvent>,
}

#[derive(Default)]
pub struct ApiJobRegistry {
    jobs: RwLock<HashMap<String, ApiJobEntry>>,
    batches: RwLock<HashMap<String, ApiBatch>>,
}

impl ApiJobRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert a fresh job with `status = Queued`. Returns a `Sender` clone that
    /// the caller (job-execution task) keeps for emitting events.
    pub fn insert(&self, job: ApiJob) {
        if let Ok(mut g) = self.jobs.write() {
            let (sender, _) = broadcast::channel(BROADCAST_CAPACITY);
            let entry = ApiJobEntry { job, sender };
            // Best-effort snapshot to any future late-subscriber via `last_snapshot`
            // is not implemented here; SSE handler reads a fresh snapshot before
            // attaching the receiver, so initial state is never missed.
            g.insert(entry.job.id.clone(), entry);
        }
    }

    pub fn get(&self, id: &str) -> Option<ApiJob> {
        self.jobs.read().ok().and_then(|g| g.get(id).map(|e| e.job.clone()))
    }

    /// Subscribe to live updates for a job. Returns `None` if the job is unknown.
    pub fn subscribe(&self, id: &str) -> Option<broadcast::Receiver<ApiJobEvent>> {
        self.jobs
            .read()
            .ok()
            .and_then(|g| g.get(id).map(|e| e.sender.subscribe()))
    }

    fn broadcast(entry: &ApiJobEntry, event: ApiJobEvent) {
        // `send` errors only when there are no active receivers — that's normal
        // (most jobs run without a subscriber). Drop the error.
        let _ = entry.sender.send(event);
    }

    pub fn update_progress(&self, id: &str, progress: ApiJobProgress) {
        if let Ok(mut g) = self.jobs.write() {
            if let Some(entry) = g.get_mut(id) {
                if entry.job.status == ApiJobStatus::Queued {
                    entry.job.status = ApiJobStatus::Running;
                }
                entry.job.progress = Some(progress);
                entry.job.updated_at = Utc::now();
                Self::broadcast(entry, ApiJobEvent::Snapshot(entry.job.clone()));
            }
        }
    }

    pub fn mark_running(&self, id: &str) {
        if let Ok(mut g) = self.jobs.write() {
            if let Some(entry) = g.get_mut(id) {
                entry.job.status = ApiJobStatus::Running;
                entry.job.updated_at = Utc::now();
                Self::broadcast(entry, ApiJobEvent::Snapshot(entry.job.clone()));
            }
        }
    }

    pub fn mark_done(&self, id: &str, transcript_path: String) {
        if let Ok(mut g) = self.jobs.write() {
            if let Some(entry) = g.get_mut(id) {
                entry.job.status = ApiJobStatus::Done;
                entry.job.transcript_path = Some(transcript_path);
                entry.job.updated_at = Utc::now();
                Self::broadcast(entry, ApiJobEvent::Snapshot(entry.job.clone()));
                Self::broadcast(entry, ApiJobEvent::Terminal);
            }
        }
    }

    pub fn mark_failed(&self, id: &str, error: String) {
        if let Ok(mut g) = self.jobs.write() {
            if let Some(entry) = g.get_mut(id) {
                entry.job.status = if error == crate::pipeline::JOB_CANCELLED_MSG {
                    ApiJobStatus::Cancelled
                } else {
                    ApiJobStatus::Failed
                };
                entry.job.error = Some(error);
                entry.job.updated_at = Utc::now();
                Self::broadcast(entry, ApiJobEvent::Snapshot(entry.job.clone()));
                Self::broadcast(entry, ApiJobEvent::Terminal);
            }
        }
    }

    // ---------- Batches ----------

    pub fn insert_batch(&self, id: String, job_ids: Vec<String>) {
        if let Ok(mut g) = self.batches.write() {
            g.insert(
                id.clone(),
                ApiBatch {
                    id,
                    created_at: Utc::now(),
                    job_ids,
                },
            );
        }
    }

    pub fn batch_snapshot(&self, batch_id: &str) -> Option<ApiBatchSnapshot> {
        let batch_meta = {
            let g = self.batches.read().ok()?;
            let b = g.get(batch_id)?;
            (b.id.clone(), b.created_at, b.job_ids.clone())
        };
        let jobs_g = self.jobs.read().ok()?;
        let mut snap = ApiBatchSnapshot {
            id: batch_meta.0,
            created_at: batch_meta.1,
            total: batch_meta.2.len(),
            queued: 0,
            running: 0,
            done: 0,
            failed: 0,
            cancelled: 0,
            job_ids: batch_meta.2.clone(),
        };
        for jid in &batch_meta.2 {
            if let Some(entry) = jobs_g.get(jid) {
                match entry.job.status {
                    ApiJobStatus::Queued => snap.queued += 1,
                    ApiJobStatus::Running => snap.running += 1,
                    ApiJobStatus::Done => snap.done += 1,
                    ApiJobStatus::Failed => snap.failed += 1,
                    ApiJobStatus::Cancelled => snap.cancelled += 1,
                }
            }
        }
        Some(snap)
    }
}

/// `ProgressSink` that mirrors job progress into an [`ApiJobRegistry`] entry.
///
/// Terminal status (Done/Failed/Cancelled) is set by the *spawning task* after
/// the underlying `run_process_queue_item` future resolves — this sink only
/// reflects in-flight progress so `GET /jobs/{id}` shows movement.
pub struct ApiJobSink {
    registry: Arc<ApiJobRegistry>,
    job_id: String,
}

impl ApiJobSink {
    pub fn new(registry: Arc<ApiJobRegistry>, job_id: String) -> Self {
        Self { registry, job_id }
    }
}

impl ProgressSink for ApiJobSink {
    fn emit(&self, event: JobEvent) {
        match event {
            JobEvent::QueueJobProgress(p) => {
                self.registry.update_progress(
                    &self.job_id,
                    ApiJobProgress {
                        phase: p.phase,
                        message: p.message,
                        subtask_index: p.subtask_index,
                        subtask_total: p.subtask_total,
                        subtask_percent: p.subtask_percent,
                    },
                );
            }
            // Other events (pipeline log, subtask status, playlist, downloads)
            // are not surfaced through the API yet — M3 will expose them via SSE.
            JobEvent::PipelineLog(_)
            | JobEvent::SubtaskStatus(_)
            | JobEvent::PlaylistResolved(_)
            | JobEvent::ModelDownload(_)
            | JobEvent::ToolDownload(_) => {}
        }
    }
}
