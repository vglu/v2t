use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use serde::Serialize;
use tauri::Emitter;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use tokio::process::Command as TokioCommand;
use tokio_util::sync::CancellationToken;

use crate::pipeline::{self, JOB_CANCELLED_MSG};
use crate::process_kill;
use crate::session_log;
use crate::transcribe::{
    pcm_payload_bytes, wav_source_fingerprint, CHUNK_SECS, FFMPEG_CHUNK_TIMEOUT, MAX_UPLOAD_BYTES,
    PCM_BYTES_PER_SEC,
};

const WHISPER_TIMEOUT: Duration = Duration::from_secs(7200);

fn safe_job_token(job_id: &str) -> String {
    job_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Parse a line like `whisper_print_progress_callback: 10% done` or `45%`.
fn parse_whisper_progress_pct(line: &str) -> Option<u8> {
    let pos = line.find('%')?;
    let mut start = pos;
    while start > 0 && line.as_bytes()[start - 1].is_ascii_digit() {
        start -= 1;
    }
    if start == pos {
        return None;
    }
    let num: u32 = line[start..pos].parse().ok()?;
    if num <= 100 {
        Some(num as u8)
    } else {
        None
    }
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct QueueJobProgressEmit {
    job_id: String,
    phase: String,
    message: String,
}

fn apply_win_no_window(cmd: &mut TokioCommand) {
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

async fn run_whisper_cli_with_progress(
    cli: &Path,
    args: &[String],
    timeout: Duration,
    cancel: &CancellationToken,
    app: &tauri::AppHandle,
    job_id: &str,
) -> Result<std::process::Output, String> {
    let mut cmd = TokioCommand::new(cli);
    cmd.args(args);
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    apply_win_no_window(&mut cmd);

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Failed to spawn whisper-cli: {e}"))?;
    let pid = child.id();

    let stderr = child.stderr.take().ok_or("whisper-cli: no stderr pipe")?;
    let stdout = child.stdout.take().ok_or("whisper-cli: no stdout pipe")?;

    let app_emit = app.clone();
    let jid = job_id.to_string();
    let stderr_task = tokio::spawn(async move {
        let mut reader = BufReader::new(stderr);
        let mut line = String::new();
        let mut last_pct: Option<u8> = None;
        let mut full = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line).await {
                Ok(0) => break,
                Ok(_) => {
                    full.push_str(&line);
                    if let Some(p) = parse_whisper_progress_pct(line.trim_end()) {
                        if last_pct != Some(p) {
                            last_pct = Some(p);
                            let msg = format!("Local Whisper: {p}%");
                            let _ = app_emit.emit(
                                "queue-job-progress",
                                &QueueJobProgressEmit {
                                    job_id: jid.clone(),
                                    phase: "whisper".to_string(),
                                    message: msg.clone(),
                                },
                            );
                            session_log::try_append(&app_emit, Some(jid.as_str()), "whisper", &msg);
                        }
                    }
                }
                Err(_) => break,
            }
        }
        full
    });

    let stdout_task = tokio::spawn(async move {
        let mut buf = Vec::new();
        let mut r = BufReader::new(stdout);
        let _ = r.read_to_end(&mut buf).await;
        buf
    });

    let status = tokio::select! {
        _ = cancel.cancelled() => {
            if let Some(p) = pid {
                process_kill::kill_process_tree(p);
            }
            stderr_task.abort();
            stdout_task.abort();
            return Err(JOB_CANCELLED_MSG.to_string());
        }
        r = tokio::time::timeout(timeout, async { child.wait().await.map_err(|e| e.to_string()) }) => {
            match r {
                Ok(Ok(st)) => st,
                Ok(Err(e)) => return Err(e),
                Err(_) => {
                    if let Some(p) = pid {
                        process_kill::kill_process_tree(p);
                    }
                    stderr_task.abort();
                    stdout_task.abort();
                    return Err(format!("whisper-cli timed out after {:?}", timeout));
                }
            }
        }
    };

    let stderr_text = stderr_task
        .await
        .map_err(|e| format!("whisper stderr task: {e}"))?;
    let stdout_bytes = stdout_task
        .await
        .map_err(|e| format!("whisper stdout task: {e}"))?;

    Ok(std::process::Output {
        status,
        stdout: stdout_bytes,
        stderr: stderr_text.into_bytes(),
    })
}

/// Run whisper.cpp `whisper-cli` (or compatible `main`) on one WAV; read `-otxt` output.
async fn transcribe_one_wav(
    wav_path: &Path,
    cli: &Path,
    model_path: &Path,
    language: Option<&str>,
    _work_dir: &Path,
    out_base: &Path,
    cancel: &CancellationToken,
    app: &tauri::AppHandle,
    job_id: &str,
) -> Result<String, String> {
    let lang = match language {
        Some(l) if !l.trim().is_empty() => l.trim().to_string(),
        _ => "auto".to_string(),
    };

    let args: Vec<String> = vec![
        "-m".into(),
        model_path.to_string_lossy().into_owned(),
        "-f".into(),
        wav_path.to_string_lossy().into_owned(),
        "-of".into(),
        out_base.to_string_lossy().into_owned(),
        "-otxt".into(),
        "-nt".into(),
        "-l".into(),
        lang,
    ];

    let out = run_whisper_cli_with_progress(cli, &args, WHISPER_TIMEOUT, cancel, app, job_id).await?;
    if !out.status.success() {
        return Err(format!(
            "whisper-cli failed (exit {}): {}",
            out.status.code().unwrap_or(-1),
            pipeline::tail_stderr(&out.stderr)
        ));
    }

    let read_path = out_base.with_extension("txt");
    if !read_path.is_file() {
        return Err(format!(
            "whisper-cli did not write expected {} (check whisper-cli version / flags)",
            read_path.display()
        ));
    }

    let text = std::fs::read_to_string(&read_path).map_err(|e| format!("read whisper txt: {e}"))?;
    let _ = std::fs::remove_file(&read_path);
    Ok(text.trim().to_string())
}

/// Same chunking strategy as HTTP path when PCM payload exceeds `MAX_UPLOAD_BYTES`.
pub async fn transcribe_wav_maybe_split_whisper(
    wav_path: &Path,
    cli: &Path,
    model_path: &Path,
    ffmpeg: &Path,
    work_dir: &Path,
    language: Option<&str>,
    cancel: &CancellationToken,
    app: &tauri::AppHandle,
    job_id: &str,
) -> Result<String, String> {
    let token = safe_job_token(job_id);
    let payload = pcm_payload_bytes(wav_path)?;
    if payload <= MAX_UPLOAD_BYTES {
        let out_base = work_dir.join(format!("v2t-whisper-{token}-0"));
        return transcribe_one_wav(
            wav_path,
            cli,
            model_path,
            language,
            work_dir,
            &out_base,
            cancel,
            app,
            job_id,
        )
        .await;
    }

    let fp = wav_source_fingerprint(wav_path)?;
    let duration_sec = (payload as f64 / PCM_BYTES_PER_SEC as f64).max(1.0);
    let mut start = 0.0f64;
    let mut i = 0u32;
    let mut parts: Vec<String> = Vec::new();
    let max_chunks = ((duration_sec / CHUNK_SECS).ceil() as u32).saturating_add(4);

    while start < duration_sec - 0.05 {
        if cancel.is_cancelled() {
            return Err(JOB_CANCELLED_MSG.to_string());
        }
        if i >= max_chunks {
            return Err("Whisper chunk split safety limit exceeded".to_string());
        }

        let checkpoint = whisper_chunk_checkpoint_path(work_dir, &fp, i);
        if checkpoint.is_file() {
            let saved =
                std::fs::read_to_string(&checkpoint).map_err(|e| format!("read checkpoint: {e}"))?;
            if !saved.trim().is_empty() {
                parts.push(saved);
                start += CHUNK_SECS;
                i += 1;
                continue;
            }
        }

        let chunk_path: PathBuf = work_dir.join(format!("v2t-whisper-{token}-chunk-{i}.wav"));
        let args: Vec<String> = vec![
            "-y".into(),
            "-ss".into(),
            format!("{start:.3}"),
            "-i".into(),
            wav_path.to_string_lossy().into_owned(),
            "-t".into(),
            format!("{CHUNK_SECS:.1}"),
            "-ar".into(),
            "16000".into(),
            "-ac".into(),
            "1".into(),
            "-c:a".into(),
            "pcm_s16le".into(),
            "-f".into(),
            "wav".into(),
            chunk_path.to_string_lossy().into_owned(),
        ];

        let out = pipeline::run_cmd(ffmpeg, &args, FFMPEG_CHUNK_TIMEOUT, cancel).await?;
        if !out.status.success() {
            return Err(format!(
                "ffmpeg whisper chunk failed: {}",
                pipeline::tail_stderr(&out.stderr)
            ));
        }

        let out_base = work_dir.join(format!("v2t-whisper-{token}-out-{i}"));
        let piece = transcribe_one_wav(
            &chunk_path,
            cli,
            model_path,
            language,
            work_dir,
            &out_base,
            cancel,
            app,
            job_id,
        )
        .await?;
        std::fs::write(&checkpoint, piece.as_bytes())
            .map_err(|e| format!("write checkpoint: {e}"))?;
        parts.push(piece);
        let _ = std::fs::remove_file(&chunk_path);
        start += CHUNK_SECS;
        i += 1;
    }

    let out = parts.join("\n\n");
    cleanup_whisper_chunk_checkpoints(work_dir, &fp);
    Ok(out)
}

fn whisper_chunk_checkpoint_path(work_dir: &Path, fp: &str, i: u32) -> PathBuf {
    work_dir.join(format!("v2t-whisper-{fp}-chunk-{i}.txt"))
}

fn cleanup_whisper_chunk_checkpoints(work_dir: &Path, fp: &str) {
    let prefix = format!("v2t-whisper-{fp}-chunk-");
    let Ok(rd) = std::fs::read_dir(work_dir) else {
        return;
    };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with(&prefix) && name.ends_with(".txt") {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_percent_from_whisper_style_line() {
        assert_eq!(
            parse_whisper_progress_pct("whisper_print_progress_callback: 10% done"),
            Some(10)
        );
        assert_eq!(parse_whisper_progress_pct("progress 45%"), Some(45));
        assert_eq!(parse_whisper_progress_pct("no percent here"), None);
    }
}
