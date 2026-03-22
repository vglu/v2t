use chrono::Utc;
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use tauri::Emitter;
use tokio_util::sync::CancellationToken;

use crate::deps;
use crate::model_download;
use crate::output_template;
use crate::pipeline;
use crate::settings::{AppSettings, TranscriptionMode};
use crate::transcribe;
use crate::whisper_catalog;
use crate::whisper_local;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueJobProgress {
    pub job_id: String,
    pub phase: String,
    pub message: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessQueueItemResult {
    pub transcript_path: String,
    pub summary: String,
}

fn emit_progress(app: &tauri::AppHandle, job_id: &str, phase: &str, message: &str) {
    let payload = QueueJobProgress {
        job_id: job_id.to_string(),
        phase: phase.to_string(),
        message: message.to_string(),
    };
    let _ = app.emit("queue-job-progress", &payload);
}

fn require_output_dir(settings: &AppSettings) -> Result<PathBuf, String> {
    let Some(ref dir) = settings.output_dir else {
        return Err("Choose output folder in Settings".to_string());
    };
    let t = dir.trim();
    if t.is_empty() {
        return Err("Choose output folder in Settings".to_string());
    }
    Ok(PathBuf::from(t))
}

pub async fn run_process_queue_item(
    app: tauri::AppHandle,
    job_id: String,
    job_index: u32,
    source: String,
    source_kind: String,
    display_label: String,
    settings: AppSettings,
    ffmpeg_path_override: Option<String>,
    yt_dlp_path_override: Option<String>,
    cancel: CancellationToken,
) -> Result<ProcessQueueItemResult, String> {
    let out_dir = require_output_dir(&settings)?;

    let ffmpeg_pb = deps::resolve_tool_path(ffmpeg_path_override.as_deref(), "ffmpeg")
        .ok_or_else(|| "ffmpeg not found (settings or folder next to app)".to_string())?;

    emit_progress(&app, &job_id, "prepare", "Preparing audio (yt-dlp / ffmpeg)…");

    let prep = pipeline::prepare_media_audio(
        Some((&app, job_id.as_str())),
        source.clone(),
        source_kind,
        ffmpeg_path_override,
        yt_dlp_path_override,
        &cancel,
    )
    .await?;

    if prep.wav_paths.is_empty() {
        return Err("No WAV paths produced".to_string());
    }

    let work_dir = PathBuf::from(&prep.wav_paths[0])
        .parent()
        .map(Path::to_path_buf)
        .ok_or("WAV path has no parent")?;

    let date = Utc::now().format("%Y-%m-%d").to_string();
    let lang = settings.language.as_deref();
    let n_tracks = prep.wav_paths.len();

    let mut last_transcript: Option<PathBuf> = None;

    for (ti, wav_s) in prep.wav_paths.iter().enumerate() {
        if cancel.is_cancelled() {
            return Err(pipeline::JOB_CANCELLED_MSG.to_string());
        }
        let track = (ti + 1) as u32;
        let wav_path = Path::new(wav_s);
        emit_progress(
            &app,
            &job_id,
            "transcribe",
            &format!(
                "Transcribing track {}/{} (splitting if file is large)…",
                track, n_tracks
            ),
        );

        let text = match settings.transcription_mode {
            TranscriptionMode::HttpApi => {
                transcribe::transcribe_wav_maybe_split(
                    wav_path,
                    &ffmpeg_pb,
                    &work_dir,
                    &settings.api_base_url,
                    &settings.api_model,
                    &settings.api_key,
                    lang,
                    &cancel,
                )
                .await?
            }
            TranscriptionMode::LocalWhisper => {
                let models_dir = model_download::resolve_models_dir(
                    &app,
                    settings.whisper_models_dir.as_deref(),
                )?;
                std::fs::create_dir_all(&models_dir)
                    .map_err(|e| format!("create models dir: {e}"))?;
                let entry = whisper_catalog::catalog_entry(&settings.whisper_model).ok_or_else(|| {
                    format!(
                        "Unknown whisper model '{}' (pick a model in Settings)",
                        settings.whisper_model
                    )
                })?;
                let model_path = models_dir.join(entry.file_name);
                if !model_path.is_file() {
                    emit_progress(
                        &app,
                        &job_id,
                        "model",
                        &format!(
                            "Downloading ggml model '{}' (~{} MiB)…",
                            entry.id, entry.size_mib
                        ),
                    );
                    model_download::download_whisper_model_file(&app, entry, &models_dir).await?;
                } else if !model_download::file_matches_sha1(&model_path, entry.sha1_hex)? {
                    return Err(format!(
                        "Model file {} failed SHA-1 check. Delete it and download again.",
                        model_path.display()
                    ));
                }
                let cli = deps::resolve_whisper_cli_path(settings.whisper_cli_path.as_deref())
                    .ok_or_else(|| {
                        "whisper-cli not found. Build whisper.cpp, set path in Settings, or place whisper-cli (or main) next to the app.".to_string()
                    })?;
                whisper_local::transcribe_wav_maybe_split_whisper(
                    wav_path,
                    &cli,
                    &model_path,
                    &ffmpeg_pb,
                    &work_dir,
                    lang,
                    &cancel,
                    &job_id,
                )
                .await?
            }
        };

        emit_progress(&app, &job_id, "save", &format!("Writing transcript {track}/{n_tracks}…"));

        let filename = output_template::format_output_filename(
            &settings.filename_template,
            &display_label,
            &date,
            job_index,
            track,
            &source,
        );

        let dest_path = out_dir.join(&filename);
        if let Some(parent) = dest_path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("create_dir_all: {e}"))?;
        }
        fs::write(&dest_path, text.as_bytes())
            .map_err(|e| format!("Failed to write transcript: {e}"))?;
        last_transcript = Some(dest_path);
    }

    let transcript_path = last_transcript
        .ok_or("No transcript written")?
        .canonicalize()
        .map_err(|e| e.to_string())?
        .to_str()
        .ok_or("Transcript path UTF-8")?
        .to_string();

    if settings.delete_audio_after {
        let _ = fs::remove_dir_all(&work_dir);
    }

    let summary = if n_tracks == 1 {
        format!("Saved: {}", transcript_path)
    } else {
        format!("Saved {n_tracks} transcript file(s); last: {transcript_path}")
    };
    emit_progress(&app, &job_id, "done", &summary);

    Ok(ProcessQueueItemResult {
        transcript_path: transcript_path.clone(),
        summary,
    })
}
