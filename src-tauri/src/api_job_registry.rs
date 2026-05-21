//! State for REST-submitted jobs (M2/M3 in-memory + M4 SQLite persistence).
//!
//! In-memory `HashMap`s are the fast read path and the source of SSE broadcast
//! channels (which can't be persisted). Every state change is also written
//! through to a SQLite DB (`init_db`), so jobs and batches survive an app
//! restart. On startup, any job left `queued`/`running` is marked `interrupted`
//! (the process that was running it is gone).

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, RwLock};

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
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
    /// Was queued/running when the app stopped — restored from DB on next start
    /// but no longer actually executing.
    Interrupted,
}

impl ApiJobStatus {
    fn as_str(self) -> &'static str {
        match self {
            ApiJobStatus::Queued => "queued",
            ApiJobStatus::Running => "running",
            ApiJobStatus::Done => "done",
            ApiJobStatus::Failed => "failed",
            ApiJobStatus::Cancelled => "cancelled",
            ApiJobStatus::Interrupted => "interrupted",
        }
    }

    fn from_str(s: &str) -> ApiJobStatus {
        match s {
            "running" => ApiJobStatus::Running,
            "done" => ApiJobStatus::Done,
            "failed" => ApiJobStatus::Failed,
            "cancelled" => ApiJobStatus::Cancelled,
            "interrupted" => ApiJobStatus::Interrupted,
            // "queued" or anything unexpected → Queued
            _ => ApiJobStatus::Queued,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
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
    /// `None` until `init_db` runs (pure in-memory before that, e.g. in tests).
    db: Mutex<Option<Connection>>,
}

fn callback_to_json(cb: &Option<ApiJobCallback>) -> Option<String> {
    cb.as_ref().and_then(|c| {
        serde_json::to_string(&serde_json::json!({ "url": c.url, "secret": c.secret })).ok()
    })
}

fn callback_from_json(s: Option<String>) -> Option<ApiJobCallback> {
    let raw = s?;
    let v: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let url = v.get("url")?.as_str()?.to_string();
    let secret = v.get("secret").and_then(|x| x.as_str()).map(str::to_string);
    Some(ApiJobCallback { url, secret })
}

impl ApiJobRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Open (or create) the SQLite DB at `path`, create tables, restore existing
    /// jobs/batches into memory, and demote any in-flight job to `Interrupted`.
    /// Best-effort: on any DB error the registry stays in pure in-memory mode.
    pub fn init_db(&self, path: &Path) -> Result<(), String> {
        let conn = Connection::open(path).map_err(|e| format!("open api db: {e}"))?;
        let _ = conn.pragma_update(None, "journal_mode", "WAL");
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS api_jobs (
                id TEXT PRIMARY KEY,
                status TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                source TEXT NOT NULL,
                source_kind TEXT NOT NULL,
                progress_json TEXT,
                transcript_path TEXT,
                error TEXT,
                batch_id TEXT,
                callback_json TEXT
            );
            CREATE TABLE IF NOT EXISTS api_batches (
                id TEXT PRIMARY KEY,
                created_at TEXT NOT NULL,
                job_ids_json TEXT NOT NULL
            );",
        )
        .map_err(|e| format!("create api tables: {e}"))?;

        // Any job that was queued/running when we stopped is no longer executing.
        conn.execute(
            "UPDATE api_jobs SET status = 'interrupted', updated_at = ?1
             WHERE status IN ('queued', 'running')",
            params![Utc::now().to_rfc3339()],
        )
        .map_err(|e| format!("mark interrupted: {e}"))?;

        let loaded_jobs = load_jobs(&conn)?;
        let loaded_batches = load_batches(&conn)?;

        if let Ok(mut g) = self.jobs.write() {
            for job in loaded_jobs {
                let (sender, _) = broadcast::channel(BROADCAST_CAPACITY);
                g.insert(job.id.clone(), ApiJobEntry { job, sender });
            }
        }
        if let Ok(mut g) = self.batches.write() {
            for b in loaded_batches {
                g.insert(b.id.clone(), b);
            }
        }
        if let Ok(mut g) = self.db.lock() {
            *g = Some(conn);
        }
        Ok(())
    }

    /// Write-through one job row. No-op when the DB isn't open. Errors are
    /// swallowed — persistence must never break the live request path.
    fn persist_job(&self, job: &ApiJob) {
        let Ok(g) = self.db.lock() else { return };
        let Some(conn) = g.as_ref() else { return };
        let progress_json = job
            .progress
            .as_ref()
            .and_then(|p| serde_json::to_string(p).ok());
        let _ = conn.execute(
            "INSERT OR REPLACE INTO api_jobs
             (id, status, created_at, updated_at, source, source_kind,
              progress_json, transcript_path, error, batch_id, callback_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                job.id,
                job.status.as_str(),
                job.created_at.to_rfc3339(),
                job.updated_at.to_rfc3339(),
                job.source,
                job.source_kind,
                progress_json,
                job.transcript_path,
                job.error,
                job.batch_id,
                callback_to_json(&job.callback),
            ],
        );
    }

    fn persist_batch(&self, id: &str, created_at: DateTime<Utc>, job_ids: &[String]) {
        let Ok(g) = self.db.lock() else { return };
        let Some(conn) = g.as_ref() else { return };
        let _ = conn.execute(
            "INSERT OR REPLACE INTO api_batches (id, created_at, job_ids_json)
             VALUES (?1, ?2, ?3)",
            params![
                id,
                created_at.to_rfc3339(),
                serde_json::to_string(job_ids).unwrap_or_else(|_| "[]".to_string()),
            ],
        );
    }

    /// Insert a fresh job with `status = Queued`. Returns a `Sender` clone that
    /// the caller (job-execution task) keeps for emitting events.
    pub fn insert(&self, job: ApiJob) {
        self.persist_job(&job);
        if let Ok(mut g) = self.jobs.write() {
            let (sender, _) = broadcast::channel(BROADCAST_CAPACITY);
            let entry = ApiJobEntry { job, sender };
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
        let snapshot = {
            let Ok(mut g) = self.jobs.write() else { return };
            let Some(entry) = g.get_mut(id) else { return };
            if entry.job.status == ApiJobStatus::Queued {
                entry.job.status = ApiJobStatus::Running;
            }
            entry.job.progress = Some(progress);
            entry.job.updated_at = Utc::now();
            Self::broadcast(entry, ApiJobEvent::Snapshot(entry.job.clone()));
            entry.job.clone()
        };
        self.persist_job(&snapshot);
    }

    pub fn mark_running(&self, id: &str) {
        let snapshot = {
            let Ok(mut g) = self.jobs.write() else { return };
            let Some(entry) = g.get_mut(id) else { return };
            entry.job.status = ApiJobStatus::Running;
            entry.job.updated_at = Utc::now();
            Self::broadcast(entry, ApiJobEvent::Snapshot(entry.job.clone()));
            entry.job.clone()
        };
        self.persist_job(&snapshot);
    }

    pub fn mark_done(&self, id: &str, transcript_path: String) {
        let snapshot = {
            let Ok(mut g) = self.jobs.write() else { return };
            let Some(entry) = g.get_mut(id) else { return };
            entry.job.status = ApiJobStatus::Done;
            entry.job.transcript_path = Some(transcript_path);
            entry.job.updated_at = Utc::now();
            Self::broadcast(entry, ApiJobEvent::Snapshot(entry.job.clone()));
            Self::broadcast(entry, ApiJobEvent::Terminal);
            entry.job.clone()
        };
        self.persist_job(&snapshot);
    }

    pub fn mark_failed(&self, id: &str, error: String) {
        let snapshot = {
            let Ok(mut g) = self.jobs.write() else { return };
            let Some(entry) = g.get_mut(id) else { return };
            entry.job.status = if error == crate::pipeline::JOB_CANCELLED_MSG {
                ApiJobStatus::Cancelled
            } else {
                ApiJobStatus::Failed
            };
            entry.job.error = Some(error);
            entry.job.updated_at = Utc::now();
            Self::broadcast(entry, ApiJobEvent::Snapshot(entry.job.clone()));
            Self::broadcast(entry, ApiJobEvent::Terminal);
            entry.job.clone()
        };
        self.persist_job(&snapshot);
    }

    // ---------- Batches ----------

    pub fn insert_batch(&self, id: String, job_ids: Vec<String>) {
        let created_at = Utc::now();
        self.persist_batch(&id, created_at, &job_ids);
        if let Ok(mut g) = self.batches.write() {
            g.insert(
                id.clone(),
                ApiBatch {
                    id,
                    created_at,
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
                    // Interrupted jobs are terminal-but-not-successful; count them
                    // with failures so batch progress still sums to total.
                    ApiJobStatus::Interrupted => snap.failed += 1,
                }
            }
        }
        Some(snap)
    }
}

fn load_jobs(conn: &Connection) -> Result<Vec<ApiJob>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, status, created_at, updated_at, source, source_kind,
                    progress_json, transcript_path, error, batch_id, callback_json
             FROM api_jobs",
        )
        .map_err(|e| format!("prepare load jobs: {e}"))?;
    let rows = stmt
        .query_map([], |row| {
            let created: String = row.get(2)?;
            let updated: String = row.get(3)?;
            let progress_json: Option<String> = row.get(6)?;
            let callback_json: Option<String> = row.get(10)?;
            Ok(ApiJob {
                id: row.get(0)?,
                status: ApiJobStatus::from_str(&row.get::<_, String>(1)?),
                created_at: parse_dt(&created),
                updated_at: parse_dt(&updated),
                source: row.get(4)?,
                source_kind: row.get(5)?,
                progress: progress_json.and_then(|s| serde_json::from_str(&s).ok()),
                transcript_path: row.get(7)?,
                error: row.get(8)?,
                batch_id: row.get(9)?,
                callback: callback_from_json(callback_json),
            })
        })
        .map_err(|e| format!("query jobs: {e}"))?;
    let mut out = Vec::new();
    for r in rows {
        if let Ok(job) = r {
            out.push(job);
        }
    }
    Ok(out)
}

fn load_batches(conn: &Connection) -> Result<Vec<ApiBatch>, String> {
    let mut stmt = conn
        .prepare("SELECT id, created_at, job_ids_json FROM api_batches")
        .map_err(|e| format!("prepare load batches: {e}"))?;
    let rows = stmt
        .query_map([], |row| {
            let created: String = row.get(1)?;
            let ids_json: String = row.get(2)?;
            Ok(ApiBatch {
                id: row.get(0)?,
                created_at: parse_dt(&created),
                job_ids: serde_json::from_str(&ids_json).unwrap_or_default(),
            })
        })
        .map_err(|e| format!("query batches: {e}"))?;
    let mut out = Vec::new();
    for r in rows {
        if let Ok(b) = r {
            out.push(b);
        }
    }
    Ok(out)
}

fn parse_dt(s: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(s)
        .map(|d| d.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_job(id: &str, status: ApiJobStatus) -> ApiJob {
        let now = Utc::now();
        ApiJob {
            id: id.to_string(),
            status,
            created_at: now,
            updated_at: now,
            source: "https://example.com/a.mp3".to_string(),
            source_kind: "url".to_string(),
            progress: None,
            transcript_path: None,
            error: None,
            batch_id: Some("batch-1".to_string()),
            callback: None,
        }
    }

    #[test]
    fn persists_and_restores_with_interrupted_demotion() {
        let dir = std::env::temp_dir().join(format!("v2t-test-{}", uuid_like()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("api.db");

        // First "session": one running job + one done job, plus a batch.
        {
            let reg = ApiJobRegistry::new();
            reg.init_db(&db).unwrap();
            reg.insert(sample_job("j-run", ApiJobStatus::Queued));
            reg.mark_running("j-run");
            reg.insert(sample_job("j-done", ApiJobStatus::Queued));
            reg.mark_done("j-done", "/out/j-done.txt".to_string());
            reg.insert_batch("batch-1".to_string(), vec!["j-run".into(), "j-done".into()]);
        }

        // Second "session": reopen the same DB.
        let reg2 = ApiJobRegistry::new();
        reg2.init_db(&db).unwrap();

        // The running job should have been demoted to Interrupted on restart.
        assert_eq!(reg2.get("j-run").unwrap().status, ApiJobStatus::Interrupted);
        // The done job + its transcript path survive untouched.
        let done = reg2.get("j-done").unwrap();
        assert_eq!(done.status, ApiJobStatus::Done);
        assert_eq!(done.transcript_path.as_deref(), Some("/out/j-done.txt"));
        // Batch membership survives; interrupted counts toward "failed" in the sum.
        let snap = reg2.batch_snapshot("batch-1").unwrap();
        assert_eq!(snap.total, 2);
        assert_eq!(snap.done, 1);
        assert_eq!(snap.failed, 1);

        let _ = std::fs::remove_dir_all(&dir);
    }

    // Tiny unique-ish suffix without pulling uuid into the test.
    fn uuid_like() -> u128 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
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
