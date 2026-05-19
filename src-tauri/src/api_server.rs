//! REST API server (M2 wave).
//!
//! Binds **only** to `127.0.0.1:<port>` per the project decision — never on
//! `0.0.0.0`, never on a public interface. Auth is a single static bearer token
//! from `AppSettings.api_server.bearer_token`.
//!
//! Endpoints:
//! - `GET  /v1/health`                — liveness, no auth
//! - `POST /v1/jobs`                  — submit a single job (JSON body)
//! - `GET  /v1/jobs/{id}`             — job state
//! - `GET  /v1/jobs/{id}/transcript`  — transcript text (when done)
//! - `POST /v1/jobs/{id}/cancel`      — cooperative cancel

use std::convert::Infallible;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::{
    extract::{Path, State},
    http::{header, HeaderMap, StatusCode},
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse, Response,
    },
    routing::{get, post},
    Json, Router,
};
use chrono::Utc;
use futures_util::stream::Stream;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;
use utoipa::{Modify, OpenApi, ToSchema};
use uuid::Uuid;

/// Maximum jobs running concurrently (API + batch combined). M2/M3 hardcoded;
/// can be made configurable later if needed.
const API_MAX_CONCURRENT_JOBS: usize = 2;
/// Max items per batch — guards against runaway clients.
const API_MAX_BATCH_SIZE: usize = 1000;

use tauri::Manager;

use crate::api_job_registry::{
    ApiBatchSnapshot, ApiJob, ApiJobCallback, ApiJobEvent, ApiJobProgress, ApiJobRegistry,
    ApiJobSink, ApiJobStatus,
};
use crate::cancel_registry::JobCancelRegistry;
use crate::job::{self, ProcessQueueItemOutcome};
use crate::pipeline;
use crate::progress::SinkHandle;
use crate::session_log;
use crate::settings::{self, AppSettings, TranscriptionMode};
use crate::webhook::{self, WebhookTarget};
use utoipa::openapi::security::{HttpAuthScheme, HttpBuilder, SecurityScheme};
use utoipa_swagger_ui::SwaggerUi;

#[derive(Clone)]
pub struct ApiServerState {
    pub app: tauri::AppHandle,
    pub job_registry: Arc<ApiJobRegistry>,
    pub bearer_token: String,
    pub concurrency: Arc<Semaphore>,
}

/// Build the axum router. Public so tests can drive it without binding a real socket.
pub fn router(state: Arc<ApiServerState>) -> Router {
    let api_routes = Router::new()
        .route("/v1/health", get(health))
        .route("/v1/jobs", post(submit_job))
        .route("/v1/jobs/:id", get(get_job))
        .route("/v1/jobs/:id/transcript", get(get_transcript))
        .route("/v1/jobs/:id/cancel", post(cancel_job))
        .route("/v1/jobs/:id/events", get(job_events_sse))
        .route("/v1/batches", post(submit_batch))
        .route("/v1/batches/:id", get(get_batch))
        .with_state(state);

    // Swagger UI is intentionally unauthenticated — discovery only. The OpenAPI
    // spec doesn't reveal anything the user couldn't read from settings.json.
    // The bearer token is still required for any actual API call.
    Router::new()
        .merge(SwaggerUi::new("/v1/docs").url("/v1/openapi.json", ApiDoc::openapi()))
        .merge(api_routes)
}

#[derive(OpenApi)]
#[openapi(
    info(
        title = "v2t REST API",
        description = "Local REST API for v2t. Binds only to 127.0.0.1 while the desktop app is running.",
    ),
    paths(
        health,
        submit_job,
        get_job,
        get_transcript,
        cancel_job,
        job_events_sse,
        submit_batch,
        get_batch,
    ),
    components(schemas(
        HealthResponse,
        ApiErrorBody,
        SubmitJobRequest,
        SubmitJobOptions,
        SubmitJobCallback,
        SubmitJobResponse,
        SubmitBatchRequest,
        BatchDefaults,
        SubmitBatchResponse,
        ApiJob,
        ApiJobStatus,
        ApiJobProgress,
        ApiJobCallback,
        ApiJobEvent,
        ApiBatchSnapshot,
        TranscriptionMode,
    )),
    modifiers(&BearerAuthAddon),
    tags(
        (name = "system", description = "Server liveness & metadata"),
        (name = "jobs", description = "Single-job lifecycle"),
        (name = "batches", description = "Bulk job submission"),
    ),
)]
pub struct ApiDoc;

struct BearerAuthAddon;

impl Modify for BearerAuthAddon {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        let components = openapi
            .components
            .as_mut()
            .expect("utoipa always builds components when schemas are declared");
        components.add_security_scheme(
            "bearer",
            SecurityScheme::Http(HttpBuilder::new().scheme(HttpAuthScheme::Bearer).build()),
        );
    }
}

// ---------- Supervisor ----------

/// Owns the long-lived API job registry (persists across enable/disable cycles)
/// and the task handle of the running `axum::serve` future. `apply_settings`
/// reads the current `AppSettings` and reconciles the running state with it.
pub struct ApiServerSupervisor {
    job_registry: Arc<ApiJobRegistry>,
    task: Mutex<Option<tauri::async_runtime::JoinHandle<()>>>,
    last_port: Mutex<Option<u16>>,
}

impl ApiServerSupervisor {
    pub fn new() -> Self {
        Self {
            job_registry: Arc::new(ApiJobRegistry::new()),
            task: Mutex::new(None),
            last_port: Mutex::new(None),
        }
    }

    /// `(running, port_if_running)`. Running means the task is alive (axum is bound).
    /// In practice the task only exits on `stop()` or on a fatal serve error.
    pub fn status(&self) -> (bool, Option<u16>) {
        let port = self.last_port.lock().ok().and_then(|g| *g);
        let running = self.task.lock().ok().is_some_and(|g| g.is_some()) && port.is_some();
        (running, port)
    }

    pub fn stop(&self) {
        if let Ok(mut g) = self.task.lock() {
            if let Some(h) = g.take() {
                h.abort();
            }
        }
        if let Ok(mut g) = self.last_port.lock() {
            *g = None;
        }
    }

    /// Read current settings from disk and reconcile the server with them.
    /// Generates a bearer token on first enable and persists it back.
    pub fn apply_settings(&self, app: &tauri::AppHandle) -> Result<(), String> {
        self.stop();
        let mut s = settings::load(app)?;
        if !s.api_server.enabled {
            return Ok(());
        }
        if s.api_server.bearer_token.trim().is_empty() {
            s.api_server.bearer_token = generate_bearer_token();
            settings::save(app, &s)?;
            session_log::try_append(
                app,
                None,
                "api-server",
                "generated bearer token on first enable",
            );
        }
        let port = s.api_server.port;
        let state = Arc::new(ApiServerState {
            app: app.clone(),
            job_registry: self.job_registry.clone(),
            bearer_token: s.api_server.bearer_token.clone(),
            concurrency: Arc::new(Semaphore::new(API_MAX_CONCURRENT_JOBS)),
        });
        let app_for_log = app.clone();
        let handle = tauri::async_runtime::spawn(async move {
            if let Err(e) = serve(state, port).await {
                session_log::try_append(
                    &app_for_log,
                    None,
                    "api-server",
                    &format!("stopped: {e}"),
                );
            }
        });
        if let Ok(mut g) = self.task.lock() {
            *g = Some(handle);
        }
        if let Ok(mut g) = self.last_port.lock() {
            *g = Some(port);
        }
        Ok(())
    }
}

impl Default for ApiServerSupervisor {
    fn default() -> Self {
        Self::new()
    }
}

fn generate_bearer_token() -> String {
    let mut buf = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut buf);
    hex::encode(buf)
}

/// Bind + serve. Returns only on shutdown / fatal error.
pub async fn serve(state: Arc<ApiServerState>, port: u16) -> Result<(), String> {
    let addr = format!("127.0.0.1:{port}");
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .map_err(|e| format!("bind {addr}: {e}"))?;
    session_log::try_append(
        &state.app,
        None,
        "api-server",
        &format!("listening on http://{addr}"),
    );
    let router = router(state);
    axum::serve(listener, router)
        .await
        .map_err(|e| format!("axum serve: {e}"))
}

// ---------- Request/response DTOs ----------

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct SubmitJobRequest {
    /// URL or absolute local file path. Required.
    source: String,
    /// `url` | `file` | `auto` (default).
    #[serde(default)]
    source_kind: Option<String>,
    /// Human-readable label used in output template (`{title}` etc.). Defaults
    /// to the source when omitted.
    #[serde(default)]
    display_label: Option<String>,
    #[serde(default)]
    options: SubmitJobOptions,
    #[serde(default)]
    callback: Option<SubmitJobCallback>,
}

#[derive(Debug, Clone, Default, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct SubmitJobOptions {
    language: Option<String>,
    /// Override `outputDir` for this job only. Falls back to settings when omitted.
    output_dir: Option<String>,
    /// `httpApi` | `localWhisper`. `browserWhisper` is rejected — it requires a webview.
    transcription_mode: Option<TranscriptionMode>,
    whisper_model: Option<String>,
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct SubmitJobCallback {
    url: String,
    secret: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct SubmitJobResponse {
    job_id: String,
    status: ApiJobStatus,
    /// Relative URL the caller can poll.
    location: String,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct HealthResponse {
    ok: bool,
    version: &'static str,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct ApiErrorBody {
    error: String,
}

// ---------- Handlers ----------

#[utoipa::path(
    get,
    path = "/v1/health",
    tag = "system",
    responses(
        (status = 200, body = HealthResponse, description = "Server is up"),
    )
)]
async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        ok: true,
        version: env!("CARGO_PKG_VERSION"),
    })
}

fn check_auth(state: &ApiServerState, headers: &HeaderMap) -> Result<(), Response> {
    let want = state.bearer_token.trim();
    if want.is_empty() {
        return Err(error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "API server token is not set (configure in Settings)",
        ));
    }
    let auth = headers
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");
    let got = auth.strip_prefix("Bearer ").unwrap_or("").trim();
    if got != want {
        return Err(error_response(
            StatusCode::UNAUTHORIZED,
            "Missing or invalid Authorization: Bearer <token>",
        ));
    }
    Ok(())
}

fn error_response(status: StatusCode, msg: impl Into<String>) -> Response {
    (
        status,
        Json(ApiErrorBody {
            error: msg.into(),
        }),
    )
        .into_response()
}

#[utoipa::path(
    post,
    path = "/v1/jobs",
    tag = "jobs",
    request_body = SubmitJobRequest,
    responses(
        (status = 202, body = SubmitJobResponse, description = "Job accepted"),
        (status = 400, body = ApiErrorBody, description = "Bad request"),
        (status = 401, body = ApiErrorBody, description = "Missing/invalid bearer token"),
    ),
    security(("bearer" = []))
)]
async fn submit_job(
    State(state): State<Arc<ApiServerState>>,
    headers: HeaderMap,
    Json(req): Json<SubmitJobRequest>,
) -> Response {
    if let Err(r) = check_auth(&state, &headers) {
        return r;
    }

    let prepared = match prepare_job(&state, &req, None) {
        Ok(p) => p,
        Err(r) => return r,
    };
    let job_id = prepared.job_id.clone();
    spawn_api_job(state.clone(), prepared);

    let body = SubmitJobResponse {
        job_id: job_id.clone(),
        status: ApiJobStatus::Queued,
        location: format!("/v1/jobs/{job_id}"),
    };
    (StatusCode::ACCEPTED, Json(body)).into_response()
}

/// Everything `submit_job` resolves before spawning. Lets `submit_batch` reuse
/// the same validation + registry-insert path per item.
struct PreparedJob {
    job_id: String,
    job_index: u32,
    source: String,
    source_kind: String,
    display_label: String,
    effective: AppSettings,
    callback: Option<ApiJobCallback>,
    cancel: CancellationToken,
}

fn prepare_job(
    state: &ApiServerState,
    req: &SubmitJobRequest,
    batch: Option<&BatchContext>,
) -> Result<PreparedJob, Response> {
    let source = req.source.trim().to_string();
    if source.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "source must be non-empty",
        ));
    }

    let source_kind = match req.source_kind.as_deref().map(str::trim) {
        Some("url") => "url".to_string(),
        Some("file") => "file".to_string(),
        Some("auto") | None | Some("") => {
            if pipeline::is_http_url(&source) {
                "url".to_string()
            } else {
                "file".to_string()
            }
        }
        Some(other) => {
            return Err(error_response(
                StatusCode::BAD_REQUEST,
                format!("sourceKind must be 'url', 'file', or 'auto' (got '{other}')"),
            ));
        }
    };

    let display_label = req
        .display_label
        .as_ref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| source.clone());

    let mut effective = settings::load(&state.app)
        .map_err(|e| error_response(StatusCode::INTERNAL_SERVER_ERROR, e))?;

    let options = merge_options(
        batch.and_then(|b| b.default_options.as_ref()),
        Some(&req.options),
    );

    if let Some(lang) = options.language {
        effective.language = Some(lang);
    }
    if let Some(dir) = options.output_dir {
        let t = dir.trim().to_string();
        if !t.is_empty() {
            effective.output_dir = Some(t);
        }
    }
    if let Some(mode) = options.transcription_mode {
        if matches!(mode, TranscriptionMode::BrowserWhisper) {
            return Err(error_response(
                StatusCode::BAD_REQUEST,
                "browserWhisper requires the webview UI and is not available over REST",
            ));
        }
        effective.transcription_mode = mode;
    } else if matches!(effective.transcription_mode, TranscriptionMode::BrowserWhisper) {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "Current settings use browserWhisper. Set transcriptionMode in the request body (e.g. 'localWhisper' or 'httpApi').",
        ));
    }
    if let Some(model) = options.whisper_model {
        effective.whisper_model = model;
    }
    if effective.output_dir.as_deref().map(str::trim).unwrap_or("").is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "outputDir not set in settings; pass options.outputDir in the request body",
        ));
    }

    let callback = req
        .callback
        .clone()
        .map(|c| ApiJobCallback {
            url: c.url,
            secret: c.secret,
        })
        .or_else(|| batch.and_then(|b| b.default_callback.clone()));

    let job_id = format!("api-{}", Uuid::new_v4());
    let job_index = batch.map(|b| b.next_index).unwrap_or(1);

    let now = Utc::now();
    let job = ApiJob {
        id: job_id.clone(),
        status: ApiJobStatus::Queued,
        created_at: now,
        updated_at: now,
        source: source.clone(),
        source_kind: source_kind.clone(),
        progress: None,
        transcript_path: None,
        error: None,
        batch_id: batch.map(|b| b.batch_id.clone()),
        callback: callback.clone(),
    };
    state.job_registry.insert(job);

    let cancel = {
        let cr = state.app.state::<JobCancelRegistry>();
        cr.register_job(&job_id)
    };

    Ok(PreparedJob {
        job_id,
        job_index,
        source,
        source_kind,
        display_label,
        effective,
        callback,
        cancel,
    })
}

struct BatchContext {
    batch_id: String,
    next_index: u32,
    default_options: Option<SubmitJobOptions>,
    default_callback: Option<ApiJobCallback>,
}

fn merge_options(
    default: Option<&SubmitJobOptions>,
    item: Option<&SubmitJobOptions>,
) -> SubmitJobOptions {
    let d = default;
    let i = item;
    SubmitJobOptions {
        language: i
            .and_then(|o| o.language.clone())
            .or_else(|| d.and_then(|o| o.language.clone())),
        output_dir: i
            .and_then(|o| o.output_dir.clone())
            .or_else(|| d.and_then(|o| o.output_dir.clone())),
        transcription_mode: i
            .and_then(|o| o.transcription_mode.clone())
            .or_else(|| d.and_then(|o| o.transcription_mode.clone())),
        whisper_model: i
            .and_then(|o| o.whisper_model.clone())
            .or_else(|| d.and_then(|o| o.whisper_model.clone())),
    }
}

fn spawn_api_job(state: Arc<ApiServerState>, prepared: PreparedJob) {
    let PreparedJob {
        job_id,
        job_index,
        source,
        source_kind,
        display_label,
        effective,
        callback,
        cancel,
    } = prepared;
    let app = state.app.clone();
    let registry = state.job_registry.clone();
    let concurrency = state.concurrency.clone();
    let ffmpeg_override = effective.ffmpeg_path.clone();
    let yt_dlp_override = effective.yt_dlp_path.clone();

    tauri::async_runtime::spawn(async move {
        // Wait for a permit before doing anything heavy. Batches of 1000 items
        // queue up here; cancellation still works while waiting (the spawned
        // run_process_queue_item observes the token).
        let _permit = match concurrency.acquire_owned().await {
            Ok(p) => p,
            Err(_) => return, // semaphore closed → app shutting down
        };
        if cancel.is_cancelled() {
            registry.mark_failed(&job_id, pipeline::JOB_CANCELLED_MSG.to_string());
            let cr = app.state::<JobCancelRegistry>();
            cr.finish_job(&job_id);
            return;
        }

        let sink: SinkHandle = Arc::new(ApiJobSink::new(registry.clone(), job_id.clone()));
        registry.mark_running(&job_id);

        let outcome = job::run_process_queue_item(
            app.clone(),
            sink,
            job_id.clone(),
            job_index,
            source.clone(),
            source_kind,
            display_label,
            effective,
            ffmpeg_override,
            yt_dlp_override,
            cancel,
        )
        .await;

        {
            let cr = app.state::<JobCancelRegistry>();
            cr.finish_job(&job_id);
        }

        let webhook_event;
        let webhook_data: serde_json::Value;
        match outcome {
            Ok(ProcessQueueItemOutcome::Done {
                transcript_path,
                summary,
            }) => {
                registry.mark_done(&job_id, transcript_path.clone());
                webhook_event = "job.completed";
                webhook_data = serde_json::json!({
                    "status": "done",
                    "transcriptPath": transcript_path,
                    "summary": summary,
                });
            }
            Ok(ProcessQueueItemOutcome::BrowserPrepared { .. }) => {
                let msg = "browserWhisper outcome reached an API job — this is a bug; reject earlier."
                    .to_string();
                registry.mark_failed(&job_id, msg.clone());
                webhook_event = "job.failed";
                webhook_data = serde_json::json!({ "status": "failed", "error": msg });
            }
            Err(e) => {
                registry.mark_failed(&job_id, e.clone());
                let is_cancel = e == pipeline::JOB_CANCELLED_MSG;
                webhook_event = if is_cancel { "job.cancelled" } else { "job.failed" };
                webhook_data = serde_json::json!({
                    "status": if is_cancel { "cancelled" } else { "failed" },
                    "error": e,
                });
            }
        }

        if let Some(cb) = callback {
            let target = WebhookTarget {
                url: cb.url,
                secret: cb.secret,
            };
            if let Err(err) =
                webhook::deliver(&target, webhook_event, &job_id, &webhook_data).await
            {
                session_log::try_append(
                    &app,
                    Some(&job_id),
                    "webhook",
                    &format!("delivery failed: {err}"),
                );
            }
        }
    });
}

#[utoipa::path(
    get,
    path = "/v1/jobs/{id}",
    tag = "jobs",
    params(("id" = String, Path, description = "Job id (api-<uuid>)")),
    responses(
        (status = 200, body = ApiJob),
        (status = 401, body = ApiErrorBody),
        (status = 404, body = ApiErrorBody),
    ),
    security(("bearer" = []))
)]
async fn get_job(
    State(state): State<Arc<ApiServerState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    if let Err(r) = check_auth(&state, &headers) {
        return r;
    }
    match state.job_registry.get(&id) {
        Some(job) => (StatusCode::OK, Json(job)).into_response(),
        None => error_response(StatusCode::NOT_FOUND, format!("Unknown job: {id}")),
    }
}

#[utoipa::path(
    get,
    path = "/v1/jobs/{id}/transcript",
    tag = "jobs",
    params(("id" = String, Path, description = "Job id")),
    responses(
        (status = 200, content_type = "text/plain", body = String, description = "Transcript text"),
        (status = 401, body = ApiErrorBody),
        (status = 404, body = ApiErrorBody),
        (status = 409, body = ApiErrorBody, description = "Job not yet done"),
    ),
    security(("bearer" = []))
)]
async fn get_transcript(
    State(state): State<Arc<ApiServerState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    if let Err(r) = check_auth(&state, &headers) {
        return r;
    }
    let Some(job) = state.job_registry.get(&id) else {
        return error_response(StatusCode::NOT_FOUND, format!("Unknown job: {id}"));
    };
    if job.status != ApiJobStatus::Done {
        return error_response(
            StatusCode::CONFLICT,
            format!("Job {id} is not done (status: {:?})", job.status),
        );
    }
    let Some(path) = job.transcript_path else {
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Job marked done but transcript path is missing",
        );
    };
    match std::fs::read_to_string(&path) {
        Ok(body) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
            body,
        )
            .into_response(),
        Err(e) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to read transcript file: {e}"),
        ),
    }
}

#[utoipa::path(
    post,
    path = "/v1/jobs/{id}/cancel",
    tag = "jobs",
    params(("id" = String, Path, description = "Job id")),
    responses(
        (status = 202, description = "Cancellation signal sent"),
        (status = 401, body = ApiErrorBody),
        (status = 404, body = ApiErrorBody),
    ),
    security(("bearer" = []))
)]
async fn cancel_job(
    State(state): State<Arc<ApiServerState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    if let Err(r) = check_auth(&state, &headers) {
        return r;
    }
    let cancel_registry = state.app.state::<JobCancelRegistry>();
    let cancelled = cancel_registry.cancel_job(&id);
    if !cancelled {
        return error_response(
            StatusCode::NOT_FOUND,
            format!("No active job with id: {id}"),
        );
    }
    (StatusCode::ACCEPTED, Json(serde_json::json!({ "cancelled": true }))).into_response()
}

// ---------- Batches ----------

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct SubmitBatchRequest {
    /// Per-item job specs. Empty array rejected.
    items: Vec<SubmitJobRequest>,
    /// Optional per-batch defaults applied to every item; item-level fields win.
    #[serde(default)]
    defaults: Option<BatchDefaults>,
}

#[derive(Debug, Default, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct BatchDefaults {
    #[serde(default)]
    options: Option<SubmitJobOptions>,
    #[serde(default)]
    callback: Option<SubmitJobCallback>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct SubmitBatchResponse {
    batch_id: String,
    job_ids: Vec<String>,
    /// Relative URL the caller can poll.
    location: String,
}

#[utoipa::path(
    post,
    path = "/v1/batches",
    tag = "batches",
    request_body = SubmitBatchRequest,
    responses(
        (status = 202, body = SubmitBatchResponse, description = "Batch accepted"),
        (status = 400, body = ApiErrorBody, description = "Bad request / too many items / invalid item"),
        (status = 401, body = ApiErrorBody),
    ),
    security(("bearer" = []))
)]
async fn submit_batch(
    State(state): State<Arc<ApiServerState>>,
    headers: HeaderMap,
    Json(req): Json<SubmitBatchRequest>,
) -> Response {
    if let Err(r) = check_auth(&state, &headers) {
        return r;
    }

    if req.items.is_empty() {
        return error_response(StatusCode::BAD_REQUEST, "items must not be empty");
    }
    if req.items.len() > API_MAX_BATCH_SIZE {
        return error_response(
            StatusCode::BAD_REQUEST,
            format!("batch size {} exceeds max {API_MAX_BATCH_SIZE}", req.items.len()),
        );
    }

    let batch_id = format!("batch-{}", Uuid::new_v4());
    let default_callback = req.defaults.as_ref().and_then(|d| {
        d.callback.as_ref().map(|c| ApiJobCallback {
            url: c.url.clone(),
            secret: c.secret.clone(),
        })
    });
    let default_options = req.defaults.and_then(|d| d.options);

    // Prepare every item first so a bad item rejects the whole batch atomically
    // (no half-registered batches). Only after all pass do we spawn the workers.
    let mut prepared: Vec<PreparedJob> = Vec::with_capacity(req.items.len());
    for (i, item) in req.items.iter().enumerate() {
        let ctx = BatchContext {
            batch_id: batch_id.clone(),
            next_index: (i + 1) as u32,
            default_options: default_options.clone(),
            default_callback: default_callback.clone(),
        };
        match prepare_job(&state, item, Some(&ctx)) {
            Ok(p) => prepared.push(p),
            Err(r) => {
                // Roll back any jobs we already registered for this batch — they
                // are still pending in the registry; cancel them so users don't
                // see orphans. (Best-effort; entries remain visible as Cancelled.)
                for p in &prepared {
                    let cr = state.app.state::<JobCancelRegistry>();
                    cr.cancel_job(&p.job_id);
                    state
                        .job_registry
                        .mark_failed(&p.job_id, pipeline::JOB_CANCELLED_MSG.to_string());
                }
                return r;
            }
        }
    }

    let job_ids: Vec<String> = prepared.iter().map(|p| p.job_id.clone()).collect();
    state.job_registry.insert_batch(batch_id.clone(), job_ids.clone());

    for p in prepared {
        spawn_api_job(state.clone(), p);
    }

    let body = SubmitBatchResponse {
        batch_id: batch_id.clone(),
        job_ids,
        location: format!("/v1/batches/{batch_id}"),
    };
    (StatusCode::ACCEPTED, Json(body)).into_response()
}

#[utoipa::path(
    get,
    path = "/v1/batches/{id}",
    tag = "batches",
    params(("id" = String, Path, description = "Batch id (batch-<uuid>)")),
    responses(
        (status = 200, body = ApiBatchSnapshot),
        (status = 401, body = ApiErrorBody),
        (status = 404, body = ApiErrorBody),
    ),
    security(("bearer" = []))
)]
async fn get_batch(
    State(state): State<Arc<ApiServerState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    if let Err(r) = check_auth(&state, &headers) {
        return r;
    }
    match state.job_registry.batch_snapshot(&id) {
        Some(snap) => (StatusCode::OK, Json(snap)).into_response(),
        None => error_response(StatusCode::NOT_FOUND, format!("Unknown batch: {id}")),
    }
}

// ---------- SSE: live job events ----------

#[utoipa::path(
    get,
    path = "/v1/jobs/{id}/events",
    tag = "jobs",
    params(("id" = String, Path, description = "Job id")),
    responses(
        (status = 200, content_type = "text/event-stream", description = "SSE stream: event=snapshot|terminal, data=ApiJobEvent JSON"),
        (status = 401, body = ApiErrorBody),
        (status = 404, body = ApiErrorBody),
    ),
    security(("bearer" = []))
)]
async fn job_events_sse(
    State(state): State<Arc<ApiServerState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    if let Err(r) = check_auth(&state, &headers) {
        return r;
    }
    // Snapshot first, *then* subscribe — that way the receiver attaches before
    // any further state change, but the initial event still reflects "now".
    let Some(initial) = state.job_registry.get(&id) else {
        return error_response(StatusCode::NOT_FOUND, format!("Unknown job: {id}"));
    };
    let Some(rx) = state.job_registry.subscribe(&id) else {
        return error_response(StatusCode::NOT_FOUND, format!("Unknown job: {id}"));
    };

    let stream = job_events_stream(initial, rx);
    Sse::new(stream)
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
        .into_response()
}

fn job_events_stream(
    initial: ApiJob,
    mut rx: tokio::sync::broadcast::Receiver<ApiJobEvent>,
) -> impl Stream<Item = Result<Event, Infallible>> {
    async_stream::stream! {
        // Always emit a snapshot first so a late subscriber sees current state.
        if let Ok(ev) = sse_event_for("snapshot", &ApiJobEvent::Snapshot(initial.clone())) {
            yield Ok(ev);
        }
        // If the job is already terminal at subscribe time, close immediately.
        if matches!(
            initial.status,
            ApiJobStatus::Done | ApiJobStatus::Failed | ApiJobStatus::Cancelled
        ) {
            let _ = sse_event_for("terminal", &ApiJobEvent::Terminal);
            return;
        }
        loop {
            match rx.recv().await {
                Ok(event) => {
                    let kind = match &event {
                        ApiJobEvent::Snapshot(_) => "snapshot",
                        ApiJobEvent::Terminal => "terminal",
                    };
                    if let Ok(out) = sse_event_for(kind, &event) {
                        yield Ok(out);
                    }
                    if matches!(event, ApiJobEvent::Terminal) {
                        break;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                    // Subscriber fell too far behind. Don't try to reconstruct
                    // missed events — let the client re-fetch via GET /jobs/{id}.
                    continue;
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    }
}

fn sse_event_for<T: Serialize>(name: &str, payload: &T) -> Result<Event, serde_json::Error> {
    let json = serde_json::to_string(payload)?;
    Ok(Event::default().event(name).data(json))
}
