use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use tokio_util::sync::CancellationToken;

use crate::audio_save;
use crate::cancel_registry::JobCancelRegistry;
use crate::deps;
use crate::model_download;
use crate::output_template;
use crate::pipeline;
use crate::progress::{JobEvent, QueueJobProgressEvent, SinkHandle, SubtaskStatusEvent, TauriSink};
use crate::settings::{AppSettings, TranscriptionMode};
use crate::subs;
use crate::timed_transcript::{
    outputs_complete_for_resume, write_transcript_pair, TimedTranscript,
};
use crate::transcribe;
use crate::whisper_catalog;
use crate::whisper_local;
use crate::yt_dlp_metadata;
use tauri::Manager;

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
        #[serde(rename = "exportWebVtt")]
        export_webvtt: bool,
    },
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessQueueItemResult {
    pub transcript_path: String,
    pub summary: String,
}

fn emit_progress(sink: &SinkHandle, job_id: &str, phase: &str, message: &str) {
    sink.emit(JobEvent::QueueJobProgress(QueueJobProgressEvent {
        job_id: job_id.to_string(),
        phase: phase.to_string(),
        message: message.to_string(),
        subtask_index: None,
        subtask_total: None,
        subtask_percent: None,
    }));
}

/// Emit one per-subtask status transition. UI listens for these to flip the
/// row icon between ▶ running / ✓ done / ⏭ skipped / ✗ error. `subtask_index`
/// is 1-based and matches the playlist index reported by `playlist-resolved`
/// (or simply `1` for single-video URLs).
fn emit_subtask_status(
    sink: &SinkHandle,
    job_id: &str,
    subtask_index: u32,
    status: &'static str,
    reason: Option<String>,
) {
    sink.emit(JobEvent::SubtaskStatus(SubtaskStatusEvent {
        job_id: job_id.to_string(),
        subtask_index,
        status,
        reason,
    }));
}

pub fn require_output_dir(settings: &AppSettings) -> Result<PathBuf, String> {
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

fn write_browser_track_result(
    track: &BrowserTrackInfo,
    result: &TimedTranscript,
    export_webvtt: bool,
) -> Result<(), String> {
    if track.skip_transcribe {
        return Ok(());
    }
    write_transcript_pair(Path::new(&track.transcript_path), result, export_webvtt)
        .map(|_| ())
        .map_err(|e| format!("Failed to write transcript pair: {e}"))
}

fn resolve_stable_checkpoint_dir(
    app: &tauri::AppHandle,
    wav_path: &Path,
    settings: &AppSettings,
) -> Result<PathBuf, String> {
    use crate::timed_transcript::{normalize_http_provider_scope, LOCAL_WHISPER_PROVIDER_SCOPE};

    let use_tdrz = settings.transcription_mode == TranscriptionMode::LocalWhisper
        && settings.export_webvtt
        && settings.label_speakers;
    let (mode, provider_scope, model) = match settings.transcription_mode {
        TranscriptionMode::HttpApi => (
            "httpApi",
            normalize_http_provider_scope(&settings.api_base_url),
            settings.api_model.as_str(),
        ),
        TranscriptionMode::LocalWhisper => (
            "localWhisper",
            LOCAL_WHISPER_PROVIDER_SCOPE.to_string(),
            // tinydiarize overrides the selected ggml — include that in the cache key.
            if use_tdrz {
                "small.en-tdrz"
            } else {
                settings.whisper_model.as_str()
            },
        ),
        TranscriptionMode::BrowserWhisper => (
            "browserWhisper",
            "browserWhisper".to_string(),
            settings.whisper_model.as_str(),
        ),
    };
    let language_for_key = if use_tdrz {
        Some("en")
    } else {
        settings.language.as_deref()
    };
    let key = transcribe::build_chunk_checkpoint_key(
        wav_path,
        mode,
        &provider_scope,
        model,
        language_for_key,
        settings.export_webvtt,
    )?;
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("app_data_dir: {e}"))?
        .join("chunk-checkpoints")
        .join(key);
    Ok(dir)
}

/// Write browser/WASM results as a transcript pair when `export_webvtt` is set.
pub fn finish_browser_queue_job_timed(
    app: &tauri::AppHandle,
    registry: &JobCancelRegistry,
    job_id: &str,
    tracks: &[BrowserTrackInfo],
    results: &[TimedTranscript],
    work_dir: &str,
    delete_audio_after: bool,
    output_dir: &Path,
    export_webvtt: bool,
) -> Result<ProcessQueueItemResult, String> {
    if tracks.len() != results.len() {
        return Err("tracks and results length mismatch".to_string());
    }
    let Some(cancel) = registry.token_for(job_id) else {
        return Err("Job is not active".to_string());
    };

    validate_browser_transcript_paths(output_dir, tracks)?;

    let sink: SinkHandle = TauriSink::handle(app.clone());

    let n_tracks = tracks.len();
    for (t, result) in tracks.iter().zip(results.iter()) {
        if cancel.is_cancelled() {
            return Err(pipeline::JOB_CANCELLED_MSG.to_string());
        }
        if !t.skip_transcribe {
            emit_progress(
                &sink,
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
        }
        write_browser_track_result(t, result, export_webvtt)?;
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
    emit_progress(&sink, job_id, "done", &summary);

    Ok(ProcessQueueItemResult {
        transcript_path,
        summary,
    })
}

#[allow(clippy::too_many_arguments)]
pub async fn run_process_queue_item(
    app: tauri::AppHandle,
    sink: SinkHandle,
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

    // Vision fast-path: route image/document files before the audio pipeline.
    if crate::vision::is_vision_input(&source) {
        return crate::vision::run_vision_job(
            &app,
            &sink,
            &job_id,
            job_index,
            &source,
            &display_label,
            &settings,
            &cancel,
        )
        .await;
    }

    let ffmpeg_pb = deps::resolve_tool_path(ffmpeg_path_override.as_deref(), "ffmpeg")
        .ok_or_else(|| "ffmpeg not found (settings or folder next to app)".to_string())?;

    let date = Utc::now().format("%Y-%m-%d").to_string();

    let video_output_path: Option<PathBuf> = if settings.keep_downloaded_video
        && (source_kind == "url" || pipeline::is_http_url(&source))
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

    emit_progress(
        &sink,
        &job_id,
        "prepare",
        "Preparing audio (yt-dlp / ffmpeg)…",
    );

    // Pre-resolve playlist metadata before download so the UI can render the
    // per-video subtask list (titles + clickable links) up-front. Best-effort:
    // any failure (private playlist, single video, no internet, yt-dlp version
    // mismatch) is logged and silently ignored — pipeline continues unchanged.
    if (source_kind == "url" || pipeline::is_http_url(&source)) && !cancel.is_cancelled() {
        if let Some(yt_dlp) = deps::resolve_tool_path(yt_dlp_path_override.as_deref(), "yt-dlp") {
            match yt_dlp_metadata::resolve_playlist_metadata(
                &yt_dlp,
                &source,
                settings.cookies_from_browser.yt_dlp_arg(),
                settings.yt_dlp_js_runtimes.as_deref(),
                &cancel,
            )
            .await
            {
                Ok(info) => {
                    let subtasks = yt_dlp_metadata::entries_to_subtasks(&info);
                    if !subtasks.is_empty() {
                        let payload = yt_dlp_metadata::PlaylistResolvedPayload {
                            job_id: job_id.clone(),
                            playlist_title: info.title.clone(),
                            subtasks,
                        };
                        let title_str = info
                            .title
                            .as_deref()
                            .map(|t| format!(" \"{t}\""))
                            .unwrap_or_default();
                        let n_subtasks = payload.subtasks.len();
                        sink.emit(JobEvent::PlaylistResolved(payload));
                        emit_progress(
                            &sink,
                            &job_id,
                            "playlist",
                            &format!("Resolved playlist{title_str} ({n_subtasks} videos)",),
                        );
                    }
                }
                Err(e) => {
                    pipeline::emit_pipeline_text(
                        &sink,
                        &job_id,
                        "yt-dlp-meta",
                        &format!("Pre-resolve skipped: {e}"),
                    );
                }
            }
        }
    }

    // K (Wave 5): YouTube subtitle fast-path. When the user has opted in and the
    // video has manual subs in a priority language, fetch the SRT directly and
    // skip download + Whisper. Single-video URLs only — pure-playlist URLs would
    // need per-entry probes that are not worth the round-trips on a 100-item list.
    if (source_kind == "url" || pipeline::is_http_url(&source))
        && settings.use_subtitles_when_available
        && !subs::is_pure_playlist_url(&source)
        && !cancel.is_cancelled()
    {
        if let Some(yt_dlp) = deps::resolve_tool_path(yt_dlp_path_override.as_deref(), "yt-dlp") {
            match try_subs_fast_path(
                &sink,
                &job_id,
                job_index,
                &source,
                &display_label,
                &date,
                &settings,
                &out_dir,
                &yt_dlp,
                &cancel,
            )
            .await
            {
                Ok(Some(outcome)) => return Ok(outcome),
                Ok(None) => {
                    // No manual subs / malformed timed SRT — fall through to normal pipeline.
                }
                Err(e) => {
                    // Cancel and output-pair I/O must not fall back to Whisper.
                    return Err(e);
                }
            }
        }
    }

    let audio_format_for_yt_dlp = if settings.keep_downloaded_audio
        && (source_kind == "url" || pipeline::is_http_url(&source))
    {
        Some(settings.downloaded_audio_format)
    } else {
        None
    };

    let prep = pipeline::prepare_media_audio(
        Some((&sink, job_id.as_str())),
        source.clone(),
        source_kind.clone(),
        ffmpeg_path_override.clone(),
        yt_dlp_path_override,
        settings.yt_dlp_js_runtimes.clone(),
        settings
            .cookies_from_browser
            .yt_dlp_arg()
            .map(str::to_string),
        &cancel,
        settings.keep_downloaded_video,
        video_output_path,
        audio_format_for_yt_dlp,
    )
    .await?;

    if prep.wav_paths.is_empty() {
        return Err("No WAV paths produced".to_string());
    }

    let n_tracks = prep.wav_paths.len() as u32;

    if settings.keep_downloaded_audio {
        save_downloaded_audio(
            &sink,
            &job_id,
            &prep.source_media_files,
            prep.source_media_files_are_audio,
            &source,
            &source_kind,
            &display_label,
            &date,
            job_index,
            n_tracks,
            &settings,
            &out_dir,
            ffmpeg_path_override.as_deref(),
            &cancel,
        )
        .await;
    }

    let work_dir = PathBuf::from(&prep.wav_paths[0])
        .parent()
        .map(Path::to_path_buf)
        .ok_or("WAV path has no parent")?;

    let lang = settings.language.as_deref();

    if matches!(
        settings.transcription_mode,
        TranscriptionMode::BrowserWhisper
    ) {
        let mut tracks: Vec<BrowserTrackInfo> = Vec::new();
        for (ti, wav_s) in prep.wav_paths.iter().enumerate() {
            if cancel.is_cancelled() {
                return Err(pipeline::JOB_CANCELLED_MSG.to_string());
            }
            let track = (ti + 1) as u32;
            let filename = output_template::format_job_output_filename(
                &settings.filename_template,
                &display_label,
                &date,
                job_index,
                track,
                n_tracks,
                &source,
                "txt",
            );
            let dest_path = out_dir.join(&filename);
            if let Some(parent) = dest_path.parent() {
                fs::create_dir_all(parent).map_err(|e| format!("create_dir_all: {e}"))?;
            }

            let mut skip_transcribe = false;
            if outputs_complete_for_resume(&dest_path, settings.export_webvtt)? {
                emit_progress(
                    &sink,
                    &job_id,
                    "transcribe",
                    &format!("Track {track}/{n_tracks}: using existing transcript (resume)",),
                );
                emit_subtask_status(
                    &sink,
                    &job_id,
                    track,
                    "skipped",
                    Some("already done".to_string()),
                );
                skip_transcribe = true;
            }

            tracks.push(BrowserTrackInfo {
                wav_path: wav_s.clone(),
                transcript_path: dest_path.to_string_lossy().into_owned(),
                skip_transcribe,
            });
        }

        emit_progress(
            &sink,
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
            export_webvtt: settings.export_webvtt,
        });
    }

    let mut last_transcript: Option<PathBuf> = None;

    for (ti, wav_s) in prep.wav_paths.iter().enumerate() {
        if cancel.is_cancelled() {
            return Err(pipeline::JOB_CANCELLED_MSG.to_string());
        }
        let track = (ti + 1) as u32;
        let wav_path = Path::new(wav_s);

        let filename = output_template::format_job_output_filename(
            &settings.filename_template,
            &display_label,
            &date,
            job_index,
            track,
            n_tracks,
            &source,
            "txt",
        );
        let dest_path = out_dir.join(&filename);
        if let Some(parent) = dest_path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("create_dir_all: {e}"))?;
        }

        if outputs_complete_for_resume(&dest_path, settings.export_webvtt)? {
            emit_progress(
                &sink,
                &job_id,
                "transcribe",
                &format!("Track {track}/{n_tracks}: using existing transcript (resume)",),
            );
            emit_subtask_status(
                &sink,
                &job_id,
                track,
                "skipped",
                Some("already done".to_string()),
            );
            last_transcript = Some(dest_path);
            continue;
        }

        emit_progress(
            &sink,
            &job_id,
            "transcribe",
            &format!("Transcribing track {track}/{n_tracks} (splitting if file is large)…",),
        );
        emit_subtask_status(&sink, &job_id, track, "running", None);

        let checkpoint_dir = resolve_stable_checkpoint_dir(&app, wav_path, &settings)?;

        let transcribe_result: Result<transcribe::TimedTranscribeOutcome, String> =
            match settings.transcription_mode {
                TranscriptionMode::HttpApi => {
                    transcribe::transcribe_wav_maybe_split(
                        wav_path,
                        &ffmpeg_pb,
                        &work_dir,
                        &checkpoint_dir,
                        &settings.api_base_url,
                        &settings.api_model,
                        &settings.api_key,
                        lang,
                        settings.export_webvtt,
                        &cancel,
                    )
                    .await
                }
                TranscriptionMode::LocalWhisper => {
                    async {
                        let models_dir = model_download::resolve_models_dir(
                            &app,
                            settings.whisper_models_dir.as_deref(),
                        )?;
                        std::fs::create_dir_all(&models_dir)
                            .map_err(|e| format!("create models dir: {e}"))?;
                        let label_speakers = settings.export_webvtt && settings.label_speakers;
                        let entry = if label_speakers {
                            whisper_catalog::catalog_entry("small.en-tdrz").ok_or_else(|| {
                                "Speaker labels require catalog model 'small.en-tdrz'".to_string()
                            })?
                        } else {
                            whisper_catalog::catalog_entry(&settings.whisper_model).ok_or_else(
                                || {
                                    format!(
                                        "Unknown whisper model '{}' (pick a model in Settings)",
                                        settings.whisper_model
                                    )
                                },
                            )?
                        };
                        let model_path = models_dir.join(entry.file_name);
                        if label_speakers {
                            if !model_path.is_file() {
                                return Err(
                                    "Speaker labels require model 'small.en-tdrz' (ggml-small.en-tdrz.bin). Download it from Settings → Whisper models, then retry."
                                        .to_string(),
                                );
                            }
                            if !model_download::file_matches_sha1(&model_path, entry.sha1_hex)? {
                                return Err(format!(
                                    "Model file {} failed SHA-1 check. Delete it and download again.",
                                    model_path.display()
                                ));
                            }
                        } else if !model_path.is_file() {
                            emit_progress(
                                &sink,
                                &job_id,
                                "model",
                                &format!(
                                    "Downloading ggml model '{}' (~{} MiB)…",
                                    entry.id, entry.size_mib
                                ),
                            );
                            model_download::download_whisper_model_file(&sink, entry, &models_dir)
                                .await?;
                        } else if !model_download::file_matches_sha1(&model_path, entry.sha1_hex)? {
                            return Err(format!(
                                "Model file {} failed SHA-1 check. Delete it and download again.",
                                model_path.display()
                            ));
                        }
                        let cli =
                            deps::resolve_whisper_cli_path(settings.whisper_cli_path.as_deref())
                                .ok_or_else(|| {
                                    "whisper-cli not found. Build whisper.cpp, set path in Settings, or place whisper-cli (or main) next to the app.".to_string()
                                })?;
                        whisper_local::transcribe_wav_maybe_split_whisper(
                            wav_path,
                            &cli,
                            &model_path,
                            &ffmpeg_pb,
                            &work_dir,
                            &checkpoint_dir,
                            lang,
                            settings.export_webvtt,
                            label_speakers,
                            &cancel,
                            &sink,
                            &app,
                            &job_id,
                        )
                        .await
                    }
                    .await
                }
                TranscriptionMode::BrowserWhisper => {
                    Err("Browser Whisper handled earlier".to_string())
                }
            };

        let outcome = match transcribe_result {
            Ok(t) => t,
            Err(e) => {
                // Cancellations are not "errors" from the subtask's perspective — the
                // queue is being torn down. Don't flip the row to ✗.
                if e != pipeline::JOB_CANCELLED_MSG {
                    emit_subtask_status(&sink, &job_id, track, "error", Some(e.clone()));
                }
                return Err(e);
            }
        };

        emit_progress(
            &sink,
            &job_id,
            "save",
            &format!("Writing transcript {track}/{n_tracks}…"),
        );

        if let Err(e) =
            write_transcript_pair(&dest_path, &outcome.transcript, settings.export_webvtt)
        {
            let msg = format!("Failed to write transcript pair: {e}");
            emit_subtask_status(&sink, &job_id, track, "error", Some(msg.clone()));
            return Err(msg);
        }

        // Cleanup stable checkpoints only after the required output pair is saved.
        if let Some(dir) = outcome.chunk_checkpoint_dir.as_ref() {
            match settings.transcription_mode {
                TranscriptionMode::HttpApi => {
                    transcribe::cleanup_api_chunk_checkpoints(dir);
                }
                TranscriptionMode::LocalWhisper => {
                    whisper_local::cleanup_whisper_chunk_checkpoints(dir);
                }
                TranscriptionMode::BrowserWhisper => {}
            }
        }

        emit_subtask_status(&sink, &job_id, track, "done", None);
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
    emit_progress(&sink, &job_id, "done", &summary);

    Ok(ProcessQueueItemOutcome::Done {
        transcript_path: transcript_path.clone(),
        summary,
    })
}

/// Save extracted audio for each source track into `out_dir`. URL jobs usually copy
/// the first-pass yt-dlp audio output, but may also receive raw media when yt-dlp
/// had to skip `-x` postprocessing; those cases are extracted via ffmpeg here.
/// Local video jobs also invoke ffmpeg. Local audio sources are skipped. Errors are
/// logged and swallowed so transcription keeps going.
#[allow(clippy::too_many_arguments)]
async fn save_downloaded_audio(
    sink: &SinkHandle,
    job_id: &str,
    source_media_files: &[PathBuf],
    source_media_files_are_audio: bool,
    source: &str,
    source_kind: &str,
    display_label: &str,
    date: &str,
    job_index: u32,
    n_tracks: u32,
    settings: &AppSettings,
    out_dir: &Path,
    ffmpeg_override: Option<&str>,
    cancel: &CancellationToken,
) {
    let is_url = source_kind == "url" || pipeline::is_http_url(source);

    for (ti, src_media) in source_media_files.iter().enumerate() {
        if cancel.is_cancelled() {
            return;
        }
        let track = (ti + 1) as u32;

        let save_result: Result<PathBuf, String> = if is_url && source_media_files_are_audio {
            // yt-dlp already produced the exact format we want (or the original
            // container when format == Original). Just copy with the real extension.
            let ext = src_media
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("m4a")
                .to_string();
            let name = output_template::audio_filename_for_job(
                &settings.filename_template,
                display_label,
                date,
                job_index,
                track,
                n_tracks,
                source,
                &ext,
            );
            let dest = out_dir.join(&name);
            audio_save::copy_downloaded_audio(src_media, &dest).map(|_| dest)
        } else if pipeline::is_probably_video(src_media) {
            let Some(ffmpeg) = deps::resolve_tool_path(ffmpeg_override, "ffmpeg") else {
                pipeline::emit_pipeline_text(
                    sink,
                    job_id,
                    "audio-save",
                    "Could not save audio: ffmpeg not found",
                );
                return;
            };
            let ffprobe = deps::resolve_tool_path(ffmpeg_override, "ffprobe");
            let template = settings.filename_template.clone();
            let display = display_label.to_string();
            let date_s = date.to_string();
            let source_s = source.to_string();
            audio_save::extract_audio_from_local_video(
                &ffmpeg,
                ffprobe.as_deref(),
                src_media,
                out_dir,
                settings.downloaded_audio_format,
                move |ext: &str| {
                    output_template::audio_filename_for_job(
                        &template, &display, &date_s, job_index, track, n_tracks, &source_s, ext,
                    )
                },
                cancel,
            )
            .await
        } else if is_url {
            let ext = src_media
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("m4a")
                .to_string();
            let name = output_template::audio_filename_for_job(
                &settings.filename_template,
                display_label,
                date,
                job_index,
                track,
                n_tracks,
                source,
                &ext,
            );
            let dest = out_dir.join(&name);
            audio_save::copy_downloaded_audio(src_media, &dest).map(|_| dest)
        } else {
            // Local audio source — file is already audio, no point copying. Emit a note once.
            if ti == 0 {
                pipeline::emit_pipeline_text(
                    sink,
                    job_id,
                    "audio-save",
                    "Source is already an audio file — skipping audio save.",
                );
            }
            continue;
        };

        match save_result {
            Ok(dest) => {
                pipeline::emit_pipeline_text(
                    sink,
                    job_id,
                    "audio-save",
                    &format!("Saved audio: {}", dest.display()),
                );
            }
            Err(e) => {
                pipeline::emit_pipeline_text(
                    sink,
                    job_id,
                    "audio-save",
                    &format!("Could not save audio (continuing with transcript): {e}"),
                );
            }
        }
    }
}

/// Attempt the subtitle fast-path for a single-video URL.
/// `Ok(Some(Done))` — fast-path completed and wrote the transcript.
/// `Ok(None)` — no usable manual subs / malformed timed SRT; caller falls through.
/// `Err(_)` — cancel or output I/O failure that must bubble (no Whisper fallback).
#[allow(clippy::too_many_arguments)]
async fn try_subs_fast_path(
    sink: &SinkHandle,
    job_id: &str,
    job_index: u32,
    source: &str,
    display_label: &str,
    date: &str,
    settings: &AppSettings,
    out_dir: &Path,
    yt_dlp: &Path,
    cancel: &CancellationToken,
) -> Result<Option<ProcessQueueItemOutcome>, String> {
    let cookies = settings.cookies_from_browser.yt_dlp_arg();
    let js = settings.yt_dlp_js_runtimes.as_deref();

    emit_progress(sink, job_id, "subs", "Probing subtitles…");
    let probe = match subs::probe_subs(yt_dlp, source, cookies, js, cancel).await {
        Ok(p) => p,
        Err(e) => {
            if e == pipeline::JOB_CANCELLED_MSG {
                return Err(e);
            }
            pipeline::emit_pipeline_text(
                sink,
                job_id,
                "subs",
                &format!("Subtitle probe skipped: {e}"),
            );
            return Ok(None);
        }
    };

    let Some(lang) = subs::pick_priority_lang(&settings.subtitle_priority_langs, &probe) else {
        pipeline::emit_pipeline_text(
            sink,
            job_id,
            "subs",
            "No manual subtitles in priority languages — falling back to download + Whisper.",
        );
        return Ok(None);
    };

    if cancel.is_cancelled() {
        return Err(pipeline::JOB_CANCELLED_MSG.to_string());
    }

    emit_progress(
        sink,
        job_id,
        "subs",
        &format!("Found manual subtitles ({lang}) — fetching…"),
    );

    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    let work_dir = std::env::temp_dir().join(format!("v2t-subs-{nanos}"));

    let srt_path =
        match subs::download_srt(yt_dlp, source, &lang, &work_dir, cookies, js, cancel).await {
            Ok(p) => p,
            Err(e) => {
                let _ = fs::remove_dir_all(&work_dir);
                if e == pipeline::JOB_CANCELLED_MSG {
                    return Err(e);
                }
                pipeline::emit_pipeline_text(
                    sink,
                    job_id,
                    "subs",
                    &format!("Subtitle fetch skipped: {e}"),
                );
                return Ok(None);
            }
        };

    let srt_text = match fs::read_to_string(&srt_path) {
        Ok(t) => t,
        Err(e) => {
            let _ = fs::remove_dir_all(&work_dir);
            return Err(format!("Failed to read .srt: {e}"));
        }
    };
    let timed = match subs::srt_to_timed_transcript(&srt_text, settings.export_webvtt) {
        Ok(t) => t,
        Err(e) => {
            let _ = fs::remove_dir_all(&work_dir);
            pipeline::emit_pipeline_text(
                sink,
                job_id,
                "subs",
                &format!("Subtitle timing unusable ({e}) — falling back to download + Whisper."),
            );
            return Ok(None);
        }
    };
    if timed.text.trim().is_empty() {
        let _ = fs::remove_dir_all(&work_dir);
        pipeline::emit_pipeline_text(
            sink,
            job_id,
            "subs",
            "Subtitle file converted to empty text — falling back to download + Whisper.",
        );
        return Ok(None);
    }

    let track = 1u32;
    let filename = output_template::format_output_filename(
        &settings.filename_template,
        display_label,
        date,
        job_index,
        track,
        source,
        "txt",
    );
    let dest_path = out_dir.join(&filename);
    if let Some(parent) = dest_path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("create_dir_all: {e}"))?;
    }
    if let Err(e) = write_transcript_pair(&dest_path, &timed, settings.export_webvtt) {
        let _ = fs::remove_dir_all(&work_dir);
        return Err(format!("Failed to write transcript pair: {e}"));
    }

    if settings.keep_srt {
        let srt_filename = output_template::format_output_filename(
            &settings.filename_template,
            display_label,
            date,
            job_index,
            track,
            source,
            "srt",
        );
        let srt_dest = out_dir.join(&srt_filename);
        if let Err(e) = fs::copy(&srt_path, &srt_dest) {
            pipeline::emit_pipeline_text(
                sink,
                job_id,
                "subs",
                &format!("Could not save .srt next to transcript: {e}"),
            );
        }
    }

    let _ = fs::remove_dir_all(&work_dir);

    emit_subtask_status(
        sink,
        job_id,
        track,
        "done",
        Some(format!("from subs ({lang})")),
    );

    let transcript_path = dest_path
        .canonicalize()
        .map_err(|e| e.to_string())?
        .to_str()
        .ok_or("Transcript path UTF-8")?
        .to_string();

    let summary = format!("Saved (subs {lang}): {transcript_path}");
    emit_progress(sink, job_id, "done", &summary);

    Ok(Some(ProcessQueueItemOutcome::Done {
        transcript_path,
        summary,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timed_transcript::{format_webvtt, TimedSegment};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn test_dir(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("v2t-job-{}-{nonce}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn resume_requires_vtt_pair_when_export_enabled() {
        let dir = test_dir("resume");
        let txt = dir.join("track.txt");
        fs::write(&txt, "hello").unwrap();
        assert!(!outputs_complete_for_resume(&txt, true).unwrap());
        assert!(outputs_complete_for_resume(&txt, false).unwrap());

        let vtt = txt.with_extension("vtt");
        fs::write(&vtt, "WEBVTT\n\n00:00:00.000 --> 00:00:01.000\nhello\n").unwrap();
        assert!(outputs_complete_for_resume(&txt, true).unwrap());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn multi_track_pair_naming_uses_same_stem() {
        let dir = test_dir("names");
        let t1 = dir.join("show_t1.txt");
        let t2 = dir.join("show_t2.txt");
        write_transcript_pair(
            &t1,
            &TimedTranscript {
                text: "one".into(),
                segments: vec![TimedSegment {
                    start_ms: 0,
                    end_ms: 100,
                    text: "one".into(),
                    speaker: None,
                }],
            },
            true,
        )
        .unwrap();
        write_transcript_pair(
            &t2,
            &TimedTranscript {
                text: "two".into(),
                segments: vec![TimedSegment {
                    start_ms: 0,
                    end_ms: 200,
                    text: "two".into(),
                    speaker: None,
                }],
            },
            true,
        )
        .unwrap();
        assert!(dir.join("show_t1.vtt").is_file());
        assert!(dir.join("show_t2.vtt").is_file());
        assert_ne!(
            fs::read_to_string(dir.join("show_t1.vtt")).unwrap(),
            fs::read_to_string(dir.join("show_t2.vtt")).unwrap()
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn missing_vtt_after_txt_only_is_incomplete() {
        let dir = test_dir("incomplete");
        let txt = dir.join("only.txt");
        fs::write(&txt, "body").unwrap();
        assert!(!outputs_complete_for_resume(&txt, true).unwrap());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn format_webvtt_from_segments_is_valid() {
        let vtt = format_webvtt(&[TimedSegment {
            start_ms: 1_500,
            end_ms: 2_000,
            text: "cue".into(),
            speaker: None,
        }])
        .unwrap();
        assert!(vtt.starts_with("WEBVTT"));
        assert!(vtt.contains("00:00:01.500 --> 00:00:02.000"));
    }

    #[test]
    fn browser_prepared_serializes_job_specific_export_flag() {
        let outcome = ProcessQueueItemOutcome::BrowserPrepared {
            tracks: vec![],
            work_dir: "C:\\work".to_string(),
            delete_audio_after: false,
            language: None,
            whisper_model_id: "base".to_string(),
            export_webvtt: true,
        };
        let json = serde_json::to_value(outcome).unwrap();
        assert_eq!(json["kind"], "browserPrepared");
        assert_eq!(json["exportWebVtt"], true);
    }

    #[test]
    fn browser_track_writer_honors_enabled_and_disabled_export_flags() {
        let dir = test_dir("browser-finish");
        let timed_path = dir.join("timed.txt");
        let timed_track = BrowserTrackInfo {
            wav_path: dir.join("timed.wav").to_string_lossy().into_owned(),
            transcript_path: timed_path.to_string_lossy().into_owned(),
            skip_transcribe: false,
        };
        let timed = TimedTranscript {
            text: "hello".to_string(),
            segments: vec![TimedSegment {
                start_ms: 0,
                end_ms: 1_000,
                text: "hello".to_string(),
                speaker: None,
            }],
        };
        write_browser_track_result(&timed_track, &timed, true).unwrap();
        assert_eq!(fs::read_to_string(&timed_path).unwrap(), "hello");
        assert!(timed_path.with_extension("vtt").is_file());

        let plain_path = dir.join("plain.txt");
        let plain_track = BrowserTrackInfo {
            wav_path: dir.join("plain.wav").to_string_lossy().into_owned(),
            transcript_path: plain_path.to_string_lossy().into_owned(),
            skip_transcribe: false,
        };
        write_browser_track_result(
            &plain_track,
            &TimedTranscript::plain_text_only("legacy"),
            false,
        )
        .unwrap();
        assert_eq!(fs::read_to_string(&plain_path).unwrap(), "legacy");
        assert!(!plain_path.with_extension("vtt").exists());
        let _ = fs::remove_dir_all(dir);
    }
}
