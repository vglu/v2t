use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

use crate::pipeline::{self, JOB_CANCELLED_MSG};
use crate::transcribe::{
    pcm_payload_bytes, CHUNK_SECS, FFMPEG_CHUNK_TIMEOUT, MAX_UPLOAD_BYTES, PCM_BYTES_PER_SEC,
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

/// Run whisper.cpp `whisper-cli` (or compatible `main`) on one WAV; read `-otxt` output.
async fn transcribe_one_wav(
    wav_path: &Path,
    cli: &Path,
    model_path: &Path,
    language: Option<&str>,
    _work_dir: &Path,
    out_base: &Path,
    cancel: &CancellationToken,
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

    let out = pipeline::run_cmd(cli, &args, WHISPER_TIMEOUT, cancel).await?;
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

    let text = fs::read_to_string(&read_path).map_err(|e| format!("read whisper txt: {e}"))?;
    let _ = fs::remove_file(&read_path);
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
        )
        .await;
    }

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
        )
        .await?;
        parts.push(piece);
        let _ = fs::remove_file(&chunk_path);
        start += CHUNK_SECS;
        i += 1;
    }

    Ok(parts.join("\n\n"))
}
