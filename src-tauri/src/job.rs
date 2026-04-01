use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use tauri::Emitter;
use tokio_util::sync::CancellationToken;

use crate::cancel_registry::JobCancelRegistry;
use crate::deps;
use crate::model_download;
use crate::output_template;
use crate::pipeline;
use crate::session_log;
use crate::settings::{AppSettings, TranscriptionMode};
use crate::transcribe;
use crate::whisper_catalog;
use crate::whisper_local;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserTrackInfo {
    pub wav_path: String,
    pub transcript_path: String,
    pub skip_transcribe: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ProcessQueueItemOutcome {
    #[serde(rename = "done")]
    Done {
        #[serde(rename = "transcriptPath")]
        transcript_path: String,
        summary: String,
    },
    #[serde(rename = "browserPrepared")]
    BrowserPrepared {
        tracks: Vec<BrowserTrackInfo>,
        #[serde(rename = "workDir")]
        work_dir: String,
        #[serde(rename = "deleteAudioAfter")]
        delete_audio_after: bool,
        language: Option<String>,
        #[serde(rename = "whisperModelId")]
        whisper_model_id: String,
    },
}

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
    session_log::try_append(app, Some(job_id), phase, message);
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

fn validate_browser_transcript_paths(
    output_dir: &Path,
    tracks: &[BrowserTrackInfo],
) -> Result<(), String> {
    fs::create_dir_all(output_dir).map_err(|e| e.to_string())?;
    let out_canon = output_dir.canonicalize().map_err(|e| e.to_string())?;
    for t in tracks {
        let p = Path::new(&t.transcript_path);
        if !p.is_absolute() {
            return Err("Transcript path must be absolute".to_string());
        }
        let parent = p
            .parent()
            .ok_or_else(|| "Transcript path has no parent".to_string())?;
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        let pc = parent.canonicalize().map_err(|e| e.to_string())?;
        if !pc.starts_with(&out_canon) {
            return Err("Transcript path escapes output folder".to_string());
        }
    }
    Ok(())
}

/// After WASM transcription in the webview: write `.txt` files, optional cleanup, unregister job.
pub fn finish_browser_queue_job(
    app: &tauri::AppHandle,
    registry: &JobCancelRegistry,
    job_id: &str,
    tracks: &[BrowserTrackInfo],
    texts: &[String],
    work_dir: &str,
    delete_audio_after: bool,
    output_dir: &Path,
) -> Result<ProcessQueueItemResult, String> {
    if tracks.len() != texts.len() {
        return Err("tracks and texts length mismatch".to_string());
    }
    let Some(cancel) = registry.token_for(job_id) else {
        return Err("Job is not active".to_string());
    };

    validate_browser_transcript_paths(output_dir, tracks)?;

    let n_tracks = tracks.len();
    for (t, text) in tracks.iter().zip(texts.iter()) {
        if cancel.is_cancelled() {
            return Err(pipeline::JOB_CANCELLED_MSG.to_string());
        }
        if !t.skip_transcribe {
            emit_progress(
                app,
                job_id,
                "save",
                &format!(
                    "Writing transcript… ({})",
                    Path::new(&t.transcript_path)
                        .file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or("file")
                ),
            );
            fs::write(&t.transcript_path, text.as_bytes())
                .map_err(|e| format!("Failed to write transcript: {e}"))?;
        }
    }

    let last_transcript = tracks
        .last()
        .ok_or_else(|| "No tracks".to_string())?
        .transcript_path
        .clone();

    let transcript_path = Path::new(&last_transcript)
        .canonicalize()
        .map_err(|e| e.to_string())?
        .to_str()
        .ok_or("Transcript path UTF-8")?
        .to_string();

    if delete_audio_after {
        let wd = Path::new(work_dir);
        if wd.is_dir() {
            let _ = fs::remove_dir_all(wd);
        }
    }

    let summary = if n_tracks == 1 {
        format!("Saved: {transcript_path}")
    } else {
        format!("Saved {n_tracks} transcript file(s); last: {transcript_path}")
    };
    emit_progress(app, job_id, "done", &summary);

    Ok(ProcessQueueItemResult {
        transcript_path,
        summary,
    })
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
) -> Result<ProcessQueueItemOutcome, String> {
    let out_dir = require_output_dir(&settings)?;

    let ffmpeg_pb = deps::resolve_tool_path(ffmpeg_path_override.as_deref(), "ffmpeg")
        .ok_or_else(|| "ffmpeg not found (settings or folder next to app)".to_string())?;

    let date = Utc::now().format("%Y-%m-%d").to_string();

    let video_output_path: Option<PathBuf> =
        if settings.keep_downloaded_video && (source_kind == "url" || pipeline::is_http_url(&source))
        {
            let name = output_template::video_filename_from_transcript_template(
                &settings.filename_template,
                &display_label,
                &date,
                job_index,
                1,
                &source,
            );
            Some(out_dir.join(name))
        } else {
            None
        };

    emit_progress(&app, &job_id, "prepare", "Preparing audio (yt-dlp / ffmpeg)…");

    let prep = pipeline::prepare_media_audio(
        Some((&app, job_id.as_str())),
        source.clone(),
        source_kind,
        ffmpeg_path_override,
        yt_dlp_path_override,
        settings.yt_dlp_js_runtimes.clone(),
        settings.cookies_from_browser.yt_dlp_arg().map(str::to_string),
        &cancel,
        settings.keep_downloaded_video,
        video_output_path,
    )
    .await?;

    if prep.wav_paths.is_empty() {
        return Err("No WAV paths produced".to_string());
    }

    let work_dir = PathBuf::from(&prep.wav_paths[0])
        .parent()
        .map(Path::to_path_buf)
        .ok_or("WAV path has no parent")?;

    let lang = settings.language.as_deref();
    let n_tracks = prep.wav_paths.len();

    if matches!(settings.transcription_mode, TranscriptionMode::BrowserWhisper) {
        let mut tracks: Vec<BrowserTrackInfo> = Vec::new();
        for (ti, wav_s) in prep.wav_paths.iter().enumerate() {
            if cancel.is_cancelled() {
                return Err(pipeline::JOB_CANCELLED_MSG.to_string());
            }
            let track = (ti + 1) as u32;
            let filename = output_template::format_output_filename(
                &settings.filename_template,
                &display_label,
                &date,
                job_index,
                track,
                &source,
                "txt",
            );
            let dest_path = out_dir.join(&filename);
            if let Some(parent) = dest_path.parent() {
                fs::create_dir_all(parent).map_err(|e| format!("create_dir_all: {e}"))?;
            }

            let mut skip_transcribe = false;
            if dest_path.is_file() {
                let nonempty = fs::metadata(&dest_path)
                    .map(|m| m.len() > 0)
                    .unwrap_or(false);
                if nonempty {
                    let existing = fs::read_to_string(&dest_path).unwrap_or_default();
                    if !existing.trim().is_empty() {
                        emit_progress(
                            &app,
                            &job_id,
                            "transcribe",
                            &format!(
                                "Track {track}/{n_tracks}: using existing transcript (resume)",
                            ),
                        );
                        skip_transcribe = true;
                    }
                }
            }

            tracks.push(BrowserTrackInfo {
                wav_path: wav_s.clone(),
                transcript_path: dest_path.to_string_lossy().into_owned(),
                skip_transcribe,
            });
        }

        emit_progress(
            &app,
            &job_id,
            "browser",
            "Prepared for in-app (WASM) transcription…",
        );

        return Ok(ProcessQueueItemOutcome::BrowserPrepared {
            tracks,
            work_dir: work_dir.to_string_lossy().into_owned(),
            delete_audio_after: settings.delete_audio_after,
            language: settings.language.clone(),
            whisper_model_id: settings.whisper_model.clone(),
        });
    }

    let mut last_transcript: Option<PathBuf> = None;

    for (ti, wav_s) in prep.wav_paths.iter().enumerate() {
        if cancel.is_cancelled() {
            return Err(pipeline::JOB_CANCELLED_MSG.to_string());
        }
        let track = (ti + 1) as u32;
        let wav_path = Path::new(wav_s);

        let filename = output_template::format_output_filename(
            &settings.filename_template,
            &display_label,
            &date,
            job_index,
            track,
            &source,
            "txt",
        );
        let dest_path = out_dir.join(&filename);
        if let Some(parent) = dest_path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("create_dir_all: {e}"))?;
        }

        if dest_path.is_file() {
            let nonempty = fs::metadata(&dest_path)
                .map(|m| m.len() > 0)
                .unwrap_or(false);
            if nonempty {
                let existing = fs::read_to_string(&dest_path).unwrap_or_default();
                if !existing.trim().is_empty() {
                    emit_progress(
                        &app,
                        &job_id,
                        "transcribe",
                        &format!(
                            "Track {track}/{n_tracks}: using existing transcript (resume)",
                        ),
                    );
                    last_transcript = Some(dest_path);
                    continue;
                }
            }
        }

        emit_progress(
            &app,
            &job_id,
            "transcribe",
            &format!(
                "Transcribing track {track}/{n_tracks} (splitting if file is large)…",
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
                    &app,
                    &job_id,
                )
                .await?
            }
            TranscriptionMode::BrowserWhisper => {
                return Err("Browser Whisper handled earlier".to_string());
            }
        };

        emit_progress(&app, &job_id, "save", &format!("Writing transcript {track}/{n_tracks}…"));

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

    Ok(ProcessQueueItemOutcome::Done {
        transcript_path: transcript_path.clone(),
        summary,
    })
}
