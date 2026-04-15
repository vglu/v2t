use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::process::Command;
use tokio_util::sync::CancellationToken;

use tauri::AppHandle;
use tauri::Emitter;

use crate::deps;
use crate::process_kill;
use crate::session_log;
use crate::settings::DownloadedAudioFormat;

pub const JOB_CANCELLED_MSG: &str = "Job cancelled";

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrepareAudioResult {
    /// One or more normalized 16 kHz mono WAV paths (playlist → multiple).
    pub wav_paths: Vec<String>,
    pub summary: String,
    /// Pre-normalization media files, index-aligned with `wav_paths`.
    /// URL jobs: files inside the temp work dir (yt-dlp output).
    /// Local files: the caller-supplied source path itself.
    /// Not serialized to JS — kept as strings for the Tauri command shape.
    #[serde(skip_serializing)]
    pub source_media_files: Vec<PathBuf>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PipelineLogPayload {
    job_id: String,
    label: String,
    message: String,
}

const YT_DLP_TIMEOUT: Duration = Duration::from_secs(900);
const FFMPEG_TIMEOUT: Duration = Duration::from_secs(600);
pub(crate) const STDERR_TAIL: usize = 1800;

pub fn is_http_url(s: &str) -> bool {
    let t = s.trim();
    t.starts_with("http://") || t.starts_with("https://")
}

/// Append `--js-runtimes …` when non-empty (YouTube EJS; see yt-dlp wiki).
fn push_yt_dlp_js_runtimes(args: &mut Vec<String>, js_runtimes: Option<&str>) {
    let Some(raw) = js_runtimes.map(str::trim).filter(|s| !s.is_empty()) else {
        return;
    };
    args.push("--js-runtimes".into());
    args.push(raw.to_string());
}

/// Append `--cookies-from-browser <browser>` when provided.
fn push_yt_dlp_cookies(args: &mut Vec<String>, browser: Option<&str>) {
    let Some(b) = browser.map(str::trim).filter(|s| !s.is_empty()) else {
        return;
    };
    args.push("--cookies-from-browser".into());
    args.push(b.to_string());
}

/// YouTube copies `watch?v=…&list=…` links; yt-dlp then downloads the **entire** playlist (slow,
/// rate limits, failures). For watch / youtu.be URLs with `list=` we only want the `v=` video.
/// Pure `youtube.com/playlist?list=…` links are left unchanged so full playlists still work.
pub(crate) fn youtube_watch_url_should_use_no_playlist(url: &str) -> bool {
    let lower = url.trim().to_lowercase();
    let on_youtube = lower.contains("youtube.com")
        || lower.contains("youtu.be")
        || lower.contains("youtube-nocookie.com")
        || lower.contains("music.youtube.com");
    if !on_youtube {
        return false;
    }
    if lower.contains("youtube.com/playlist") {
        return false;
    }
    if !lower.contains("list=") {
        return false;
    }
    lower.contains("watch?")
        || lower.contains("youtu.be/")
        || lower.contains("/shorts/")
        || lower.contains("/live/")
        || lower.contains("/embed/")
}

/// Arguments for ffmpeg: 16 kHz mono PCM WAV (Whisper-friendly).
pub fn build_ffmpeg_normalize_args(input: &Path, output_wav: &Path) -> Vec<String> {
    vec![
        "-y".to_string(),
        "-i".to_string(),
        input.to_string_lossy().into_owned(),
        "-ar".to_string(),
        "16000".to_string(),
        "-ac".to_string(),
        "1".to_string(),
        "-c:a".to_string(),
        "pcm_s16le".to_string(),
        "-f".to_string(),
        "wav".to_string(),
        output_wav.to_string_lossy().into_owned(),
    ]
}

pub fn tail_stderr(data: &[u8]) -> String {
    let s = String::from_utf8_lossy(data);
    let t = s.trim();
    if t.len() <= STDERR_TAIL {
        t.to_string()
    } else {
        format!("…{}", &t[t.len() - STDERR_TAIL..])
    }
}

pub(crate) fn emit_pipeline_log(app: &AppHandle, job_id: &str, label: &str, stderr: &[u8]) {
    let message = tail_stderr(stderr);
    if message.is_empty() {
        return;
    }
    let payload = PipelineLogPayload {
        job_id: job_id.to_string(),
        label: label.to_string(),
        message: message.clone(),
    };
    let _ = app.emit("pipeline-log", &payload);
    session_log::try_append(app, Some(job_id), label, &message);
}

pub(crate) fn emit_pipeline_text(app: &AppHandle, job_id: &str, label: &str, text: &str) {
    let t = text.trim();
    if t.is_empty() {
        return;
    }
    let message = if t.len() <= STDERR_TAIL {
        t.to_string()
    } else {
        format!("…{}", &t[t.len() - STDERR_TAIL..])
    };
    let payload = PipelineLogPayload {
        job_id: job_id.to_string(),
        label: label.to_string(),
        message: message.clone(),
    };
    let _ = app.emit("pipeline-log", &payload);
    session_log::try_append(app, Some(job_id), label, &message);
}

fn apply_win_no_window(cmd: &mut Command) {
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    {
        let _ = cmd;
    }
}

fn is_probably_media(p: &Path) -> bool {
    let ext = p
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    matches!(
        ext.as_str(),
        "mp3" | "m4a" | "opus" | "webm" | "mp4" | "mkv" | "wav" | "flac" | "ogg" | "aac"
            | "wma" | "avi" | "mov" | "wmv" | "m4v" | "3gp"
    )
}

/// Video container extensions — whitelist for audio extraction from a local file.
/// NOTE: `webm` can carry audio-only (opus) as well; we treat it as video here since
/// we only use this to gate "extract audio from local video", and on a video-less webm
/// ffmpeg's `-vn` is a no-op (still produces a clean audio file).
pub(crate) fn is_probably_video(p: &Path) -> bool {
    let ext = p
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    matches!(
        ext.as_str(),
        "mp4" | "mkv" | "mov" | "webm" | "avi" | "flv" | "m4v" | "mpeg" | "mpg" | "wmv" | "ts" | "3gp"
    )
}

/// Media files produced by yt-dlp (playlists → many), sorted by path.
fn sorted_downloaded_media(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let mut files: Vec<PathBuf> = fs::read_dir(dir)
        .map_err(|e| format!("read_dir: {e}"))?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_file() && is_probably_media(p))
        .collect();
    files.sort();
    if files.is_empty() {
        return Err("yt-dlp produced no media files".to_string());
    }
    Ok(files)
}

pub(crate) async fn run_cmd(
    program: &Path,
    args: &[String],
    timeout: Duration,
    cancel: &CancellationToken,
) -> Result<std::process::Output, String> {
    let mut cmd = Command::new(program);
    cmd.args(args);
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    // Prepend the program's parent dir to PATH so sibling binaries (e.g. deno next to yt-dlp)
    // are discoverable by child processes.
    if let Some(parent) = program.parent() {
        let cur_path = std::env::var("PATH").unwrap_or_default();
        let sep = if cfg!(windows) { ";" } else { ":" };
        let new_path = format!("{}{sep}{cur_path}", parent.display());
        cmd.env("PATH", new_path);
    }

    apply_win_no_window(&mut cmd);

    let child = cmd
        .spawn()
        .map_err(|e| format!("Failed to spawn process: {e}"))?;
    let pid = child.id();

    let wait = tokio::spawn(async move {
        match tokio::time::timeout(timeout, child.wait_with_output()).await {
            Ok(Ok(output)) => Ok(output),
            Ok(Err(e)) => Err(format!("Process wait error: {e}")),
            Err(_) => {
                if let Some(p) = pid {
                    process_kill::kill_process_tree(p);
                }
                Err(format!("Process timed out after {:?}", timeout))
            }
        }
    });

    tokio::select! {
        _ = cancel.cancelled() => {
            if let Some(p) = pid {
                process_kill::kill_process_tree(p);
            }
            Err(JOB_CANCELLED_MSG.to_string())
        }
        joined = wait => match joined {
            Ok(Ok(output)) => Ok(output),
            Ok(Err(e)) => Err(e),
            Err(e) => Err(format!("Process task join: {e}")),
        },
    }
}

/// Second yt-dlp pass: best video+audio merged to `mp4` (URL jobs only; used when user opts in).
pub async fn download_best_video_mp4(
    maybe_log: Option<(&AppHandle, &str)>,
    yt_dlp: &Path,
    url: &str,
    dest_mp4: &Path,
    cancel: &CancellationToken,
    yt_dlp_js_runtimes: Option<&str>,
    cookies_from_browser: Option<&str>,
) -> Result<(), String> {
    if let Some(parent) = dest_mp4.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("create video dir: {e}"))?;
    }
    let dest = dest_mp4
        .to_str()
        .ok_or("Video output path must be UTF-8")?
        .replace('\\', "/");

    let mut args: Vec<String> = Vec::new();
    push_yt_dlp_js_runtimes(&mut args, yt_dlp_js_runtimes);
    push_yt_dlp_cookies(&mut args, cookies_from_browser);
    if youtube_watch_url_should_use_no_playlist(url) {
        args.push("--no-playlist".into());
    }
    args.extend([
        "-f".into(),
        "bv*+ba/b".into(),
        "--merge-output-format".into(),
        "mp4".into(),
        "-o".into(),
        dest,
        url.trim().to_string(),
    ]);

    let out = run_cmd(yt_dlp, &args, YT_DLP_TIMEOUT, cancel).await?;
    if !out.status.success() {
        return Err(format!(
            "yt-dlp video download failed (exit {}): {}",
            out.status.code().unwrap_or(-1),
            tail_stderr(&out.stderr)
        ));
    }
    if let Some((app, jid)) = maybe_log.as_ref() {
        emit_pipeline_log(app, jid, "yt-dlp-video", &out.stderr);
    }
    Ok(())
}

pub async fn prepare_media_audio(
    maybe_log: Option<(&AppHandle, &str)>,
    source: String,
    source_kind: String,
    ffmpeg_override: Option<String>,
    yt_dlp_override: Option<String>,
    yt_dlp_js_runtimes: Option<String>,
    cookies_from_browser: Option<String>,
    cancel: &CancellationToken,
    keep_downloaded_video: bool,
    video_output_path: Option<PathBuf>,
    // When Some, tells yt-dlp's first pass to convert to this format via --audio-format
    // (mp3|m4a). None keeps bestaudio. Only used for URL sources.
    audio_format_for_yt_dlp: Option<DownloadedAudioFormat>,
) -> Result<PrepareAudioResult, String> {
    let source = source.trim().to_string();
    if source.is_empty() {
        return Err("Empty source".to_string());
    }

    let ffmpeg = deps::resolve_tool_path(ffmpeg_override.as_deref(), "ffmpeg")
        .ok_or_else(|| "ffmpeg not found (settings or folder next to app)".to_string())?;

    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    let work_dir = std::env::temp_dir().join(format!("v2t-work-{nanos}"));
    fs::create_dir_all(&work_dir).map_err(|e| format!("create work dir: {e}"))?;

    let input_files: Vec<PathBuf> = if source_kind == "url" || (source_kind != "file" && is_http_url(&source))
    {
        let yt_dlp = deps::resolve_tool_path(yt_dlp_override.as_deref(), "yt-dlp")
            .ok_or_else(|| "yt-dlp not found (needed for URLs)".to_string())?;

        // %(id)s avoids collisions when multiple tracks are downloaded (real playlists).
        let template_path = work_dir.join("v2t-%(id)s.%(ext)s");
        let template = template_path
            .to_str()
            .ok_or("Work path is not valid UTF-8")?
            .replace('\\', "/");

        let js = yt_dlp_js_runtimes.as_deref();
        let cookies = cookies_from_browser.as_deref();
        let mut args: Vec<String> = Vec::new();
        push_yt_dlp_js_runtimes(&mut args, js);
        push_yt_dlp_cookies(&mut args, cookies);
        args.extend(["-x".into(), "--no-mtime".into()]);
        if let Some(fmt) = audio_format_for_yt_dlp.and_then(|f| f.yt_dlp_arg()) {
            args.push("--audio-format".into());
            args.push(fmt.into());
            args.push("--audio-quality".into());
            args.push("0".into());
        }
        if youtube_watch_url_should_use_no_playlist(&source) {
            args.push("--no-playlist".into());
        }
        args.push("-o".into());
        args.push(template);
        args.push(source.clone());

        let out = run_cmd(&yt_dlp, &args, YT_DLP_TIMEOUT, cancel).await?;
        if !out.status.success() {
            let _ = fs::remove_dir_all(&work_dir);
            return Err(format!(
                "yt-dlp failed (exit {}): {}",
                out.status.code().unwrap_or(-1),
                tail_stderr(&out.stderr)
            ));
        }
        if let Some((app, jid)) = maybe_log.as_ref() {
            emit_pipeline_log(app, jid, "yt-dlp", &out.stderr);
        }

        if cancel.is_cancelled() {
            let _ = fs::remove_dir_all(&work_dir);
            return Err(JOB_CANCELLED_MSG.to_string());
        }

        let media = sorted_downloaded_media(&work_dir)?;

        if keep_downloaded_video {
            if let Some(ref vp) = video_output_path {
                if is_http_url(&source) {
                    match download_best_video_mp4(
                        maybe_log,
                        &yt_dlp,
                        source.as_str(),
                        vp,
                        cancel,
                        js,
                        cookies,
                    )
                    .await
                    {
                        Ok(()) => {}
                        Err(e) => {
                            if let Some((app, jid)) = maybe_log.as_ref() {
                                emit_pipeline_text(
                                    app,
                                    jid,
                                    "yt-dlp-video",
                                    &format!("Could not save video (continuing with transcript): {e}"),
                                );
                            }
                        }
                    }
                }
            }
        }

        media
    } else {
        let p = PathBuf::from(&source);
        if !p.is_file() {
            let _ = fs::remove_dir_all(&work_dir);
            return Err(format!("File not found: {}", source));
        }
        vec![p]
    };

    if cancel.is_cancelled() {
        let _ = fs::remove_dir_all(&work_dir);
        return Err(JOB_CANCELLED_MSG.to_string());
    }

    let mut wav_paths: Vec<String> = Vec::new();
    for (i, input_media) in input_files.iter().enumerate() {
        if cancel.is_cancelled() {
            let _ = fs::remove_dir_all(&work_dir);
            return Err(JOB_CANCELLED_MSG.to_string());
        }
        let normalized = work_dir.join(format!("normalized_{i}.wav"));
        let ff_args = build_ffmpeg_normalize_args(input_media, &normalized);
        let out = run_cmd(&ffmpeg, &ff_args, FFMPEG_TIMEOUT, cancel).await?;
        if !out.status.success() {
            let _ = fs::remove_dir_all(&work_dir);
            return Err(format!(
                "ffmpeg failed (exit {}): {}",
                out.status.code().unwrap_or(-1),
                tail_stderr(&out.stderr)
            ));
        }
        if let Some((app, jid)) = maybe_log.as_ref() {
            emit_pipeline_log(app, jid, "ffmpeg", &out.stderr);
        }
        if !normalized.is_file() {
            let _ = fs::remove_dir_all(&work_dir);
            return Err(format!("ffmpeg did not create {}", normalized.display()));
        }
        let wav_path = normalized
            .canonicalize()
            .map_err(|e| e.to_string())?
            .to_str()
            .ok_or("WAV path UTF-8")?
            .to_string();
        wav_paths.push(wav_path);
    }

    let summary = if wav_paths.len() == 1 {
        format!(
            "16 kHz mono WAV ready: {}",
            wav_paths.first().unwrap()
        )
    } else {
        format!(
            "16 kHz mono WAV ready ({} tracks from playlist)",
            wav_paths.len()
        )
    };

    Ok(PrepareAudioResult {
        wav_paths,
        summary,
        source_media_files: input_files,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_http_url_detects() {
        assert!(is_http_url("https://a.com/x"));
        assert!(is_http_url(" http://x "));
        assert!(!is_http_url("/tmp/a.mp4"));
        assert!(!is_http_url(""));
    }

    #[test]
    fn youtube_no_playlist_flag_matches_browser_style_links() {
        assert!(youtube_watch_url_should_use_no_playlist(
            "https://www.youtube.com/watch?v=CwzZuMhk_SI&list=PLkoMD"
        ));
        assert!(youtube_watch_url_should_use_no_playlist(
            "https://youtu.be/CwzZuMhk_SI?list=PLkoMD"
        ));
        assert!(!youtube_watch_url_should_use_no_playlist(
            "https://www.youtube.com/watch?v=CwzZuMhk_SI"
        ));
        assert!(!youtube_watch_url_should_use_no_playlist(
            "https://www.youtube.com/playlist?list=PLkoMD"
        ));
        assert!(!youtube_watch_url_should_use_no_playlist("https://example.com/x?list=1"));
    }

    #[test]
    fn ffmpeg_args_contain_rate_and_channels() {
        let args = build_ffmpeg_normalize_args(Path::new("/in.mp4"), Path::new("/out.wav"));
        let joined = args.join(" ");
        assert!(joined.contains("16000"));
        assert!(joined.contains("pcm_s16le"));
        assert!(args.iter().any(|a| a == "-i"));
    }
}
