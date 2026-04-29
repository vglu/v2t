mod api_key_store;
mod audio_save;
mod cancel_registry;
mod deps;
mod gpu_detect;
mod job;
mod model_download;
mod output_template;
mod pipeline;
mod process_kill;
mod scan;
mod session_log;
mod settings;
mod temp_cleanup;
mod tool_download;
#[cfg(any(windows, target_os = "macos"))]
mod tool_manifest;
mod transcribe;
mod whisper_catalog;
mod whisper_local;
#[cfg(target_os = "macos")]
mod whisper_bottle_macos;
mod yt_dlp_metadata;
mod yt_dlp_progress;

use cancel_registry::JobCancelRegistry;
use session_log::SessionLog;
use std::path::PathBuf;
use tauri::Manager;
use job::{BrowserTrackInfo, ProcessQueueItemOutcome, ProcessQueueItemResult};
use settings::{AppSettings, WhisperAcceleration};
use tokio_util::sync::CancellationToken;

#[tauri::command]
fn load_settings(app: tauri::AppHandle) -> Result<AppSettings, String> {
    settings::load(&app)
}

#[tauri::command]
fn save_settings(app: tauri::AppHandle, settings: AppSettings) -> Result<(), String> {
    settings::save(&app, &settings)
}

#[tauri::command]
fn check_dependencies(
    app: tauri::AppHandle,
    ffmpeg_path_override: Option<String>,
    yt_dlp_path_override: Option<String>,
    whisper_cli_path_override: Option<String>,
    transcription_mode: Option<String>,
    whisper_model: Option<String>,
    whisper_models_dir: Option<String>,
) -> deps::DependencyReport {
    deps::check_dependencies(
        &app,
        ffmpeg_path_override.as_deref(),
        yt_dlp_path_override.as_deref(),
        whisper_cli_path_override.as_deref(),
        transcription_mode.as_deref(),
        whisper_model.as_deref(),
        whisper_models_dir.as_deref(),
    )
}

#[tauri::command]
fn scan_media_folder(path: String, recursive: bool) -> Result<Vec<String>, String> {
    let p = std::path::PathBuf::from(path.trim());
    scan::scan_media_folder(&p, recursive)
}

#[tauri::command]
async fn prepare_media_audio(
    source: String,
    source_kind: String,
    ffmpeg_path_override: Option<String>,
    yt_dlp_path_override: Option<String>,
    yt_dlp_js_runtimes: Option<String>,
) -> Result<pipeline::PrepareAudioResult, String> {
    let never = CancellationToken::new();
    pipeline::prepare_media_audio(
        None,
        source,
        source_kind,
        ffmpeg_path_override,
        yt_dlp_path_override,
        yt_dlp_js_runtimes,
        None,
        &never,
        false,
        None,
        None,
    )
    .await
}

#[tauri::command]
async fn process_queue_item(
    app: tauri::AppHandle,
    registry: tauri::State<'_, JobCancelRegistry>,
    job_id: String,
    job_index: u32,
    source: String,
    source_kind: String,
    display_label: String,
    settings: AppSettings,
    ffmpeg_path_override: Option<String>,
    yt_dlp_path_override: Option<String>,
) -> Result<ProcessQueueItemOutcome, String> {
    let cancel = registry.register_job(&job_id);
    let out = job::run_process_queue_item(
        app,
        job_id.clone(),
        job_index,
        source,
        source_kind,
        display_label,
        settings,
        ffmpeg_path_override,
        yt_dlp_path_override,
        cancel,
    )
    .await;
    match &out {
        Ok(ProcessQueueItemOutcome::Done { .. }) | Err(_) => registry.finish_job(&job_id),
        Ok(ProcessQueueItemOutcome::BrowserPrepared { .. }) => {}
    }
    out
}

#[tauri::command]
fn browser_queue_job_finish(
    app: tauri::AppHandle,
    registry: tauri::State<'_, JobCancelRegistry>,
    job_id: String,
    tracks: Vec<BrowserTrackInfo>,
    texts: Vec<String>,
    work_dir: String,
    delete_audio_after: bool,
    output_dir: String,
) -> Result<ProcessQueueItemResult, String> {
    let trimmed = output_dir.trim();
    if trimmed.is_empty() {
        return Err("Output folder is not set".to_string());
    }
    let out_dir = PathBuf::from(trimmed);
    let res = job::finish_browser_queue_job(
        &app,
        &registry,
        &job_id,
        &tracks,
        &texts,
        &work_dir,
        delete_audio_after,
        &out_dir,
    );
    registry.finish_job(&job_id);
    res
}

/// If the UI aborts after `browserPrepared` without calling `browser_queue_job_finish`, free the cancel slot.
#[tauri::command]
fn release_queue_job_slot(registry: tauri::State<'_, JobCancelRegistry>, job_id: String) {
    registry.finish_job(&job_id);
}

#[tauri::command]
fn cancel_queue_job(
    registry: tauri::State<'_, JobCancelRegistry>,
    job_id: String,
) -> Result<(), String> {
    registry.cancel_job(&job_id);
    Ok(())
}

#[tauri::command]
fn list_whisper_models() -> Vec<whisper_catalog::WhisperModelListItem> {
    whisper_catalog::list_models_for_ui()
}

#[tauri::command]
fn default_whisper_models_dir(app: tauri::AppHandle) -> Result<String, String> {
    let p = model_download::resolve_models_dir(&app, None)?;
    Ok(p.to_string_lossy().into_owned())
}

#[tauri::command]
async fn download_whisper_model(
    app: tauri::AppHandle,
    model_id: String,
    models_dir: Option<String>,
) -> Result<(), String> {
    let dir = model_download::resolve_models_dir(&app, models_dir.as_deref())?;
    let entry = whisper_catalog::catalog_entry(&model_id)
        .ok_or_else(|| format!("Unknown whisper model: {model_id}"))?;
    model_download::download_whisper_model_file(&app, entry, &dir).await
}

#[tauri::command]
fn default_documents_dir(app: tauri::AppHandle) -> Result<String, String> {
    tool_download::default_documents_dir(&app)
        .map(|p| p.to_string_lossy().into_owned())
}

#[tauri::command]
async fn download_media_tools(
    app: tauri::AppHandle,
) -> Result<tool_download::DownloadedMediaTools, String> {
    tool_download::download_managed_media_tools(&app).await
}

#[tauri::command]
async fn download_whisper_cli(
    app: tauri::AppHandle,
    acceleration: Option<WhisperAcceleration>,
) -> Result<tool_download::DownloadedWhisperCli, String> {
    let acc = acceleration.unwrap_or_default();
    tool_download::download_whisper_cli_managed(&app, acc).await
}

#[tauri::command]
fn detect_gpu() -> gpu_detect::GpuInfo {
    gpu_detect::detect_gpu()
}

#[tauri::command]
async fn install_deno(
    app: tauri::AppHandle,
) -> Result<tool_download::InstalledDeno, String> {
    tool_download::install_deno_managed(&app).await
}

#[tauri::command]
fn session_log_append_ui(app: tauri::AppHandle, message: String) {
    session_log::try_append(&app, None, "ui", &message);
}

#[tauri::command]
fn open_session_log(app: tauri::AppHandle) -> Result<(), String> {
    let Some(log) = app.try_state::<SessionLog>() else {
        return Err("Session log is not available".to_string());
    };
    tauri_plugin_opener::open_path(log.log_path(), None::<&str>).map_err(|e| e.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(JobCancelRegistry::default())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            if let Some(log) = SessionLog::try_init(app.handle()) {
                app.manage(log);
            }
            // Sweep orphaned v2t-work-* dirs older than 24 h. Off the main thread —
            // the scan touches the temp dir but does not block UI startup.
            std::thread::spawn(|| {
                let report =
                    temp_cleanup::run_cleanup(std::time::Duration::from_secs(24 * 3600));
                if report.removed > 0 || report.errors > 0 {
                    eprintln!(
                        "[v2t] temp cleanup: removed={} bytes_freed={} errors={}",
                        report.removed, report.bytes_freed, report.errors
                    );
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            load_settings,
            save_settings,
            check_dependencies,
            scan_media_folder,
            prepare_media_audio,
            process_queue_item,
            browser_queue_job_finish,
            release_queue_job_slot,
            cancel_queue_job,
            list_whisper_models,
            default_whisper_models_dir,
            download_whisper_model,
            default_documents_dir,
            download_media_tools,
            download_whisper_cli,
            detect_gpu,
            install_deno,
            session_log_append_ui,
            open_session_log
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| {
            if let tauri::RunEvent::ExitRequested { .. } = event {
                if let Some(reg) = app_handle.try_state::<JobCancelRegistry>() {
                    reg.cancel_all();
                }
            }
        });
}
