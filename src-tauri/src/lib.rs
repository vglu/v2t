mod api_key_store;
mod cancel_registry;
mod deps;
mod job;
mod model_download;
mod output_template;
mod pipeline;
mod process_kill;
mod scan;
mod settings;
mod tool_download;
mod transcribe;
mod whisper_catalog;
mod whisper_local;

use cancel_registry::JobCancelRegistry;
use tauri::Manager;
use job::ProcessQueueItemResult;
use settings::AppSettings;
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
    ffmpeg_path_override: Option<String>,
    yt_dlp_path_override: Option<String>,
    whisper_cli_path_override: Option<String>,
) -> deps::DependencyReport {
    deps::check_dependencies(
        ffmpeg_path_override.as_deref(),
        yt_dlp_path_override.as_deref(),
        whisper_cli_path_override.as_deref(),
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
) -> Result<pipeline::PrepareAudioResult, String> {
    let never = CancellationToken::new();
    pipeline::prepare_media_audio(
        None,
        source,
        source_kind,
        ffmpeg_path_override,
        yt_dlp_path_override,
        &never,
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
) -> Result<ProcessQueueItemResult, String> {
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
    registry.finish_job(&job_id);
    out
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
    tool_download::download_media_tools_windows(&app).await
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(JobCancelRegistry::default())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            load_settings,
            save_settings,
            check_dependencies,
            scan_media_folder,
            prepare_media_audio,
            process_queue_item,
            cancel_queue_job,
            list_whisper_models,
            default_whisper_models_dir,
            download_whisper_model,
            default_documents_dir,
            download_media_tools
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
