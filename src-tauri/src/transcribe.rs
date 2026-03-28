use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};
use tokio_util::sync::CancellationToken;

use crate::pipeline::{self, JOB_CANCELLED_MSG};

const HTTP_RETRY_DELAYS: [Duration; 2] = [Duration::from_secs(1), Duration::from_secs(2)];

const HTTP_TIMEOUT: Duration = Duration::from_secs(600);
pub(crate) const FFMPEG_CHUNK_TIMEOUT: Duration = Duration::from_secs(600);
/// Stay under typical 25 MB API limits (PCM 16 kHz mono ≈ 32kB/s).
pub(crate) const MAX_UPLOAD_BYTES: u64 = 22 * 1024 * 1024;
pub(crate) const CHUNK_SECS: f64 = 480.0;
const WAV_HEADER_SKIP: u64 = 64;
pub(crate) const PCM_BYTES_PER_SEC: u64 = 32000;

// OpenAI whisper-1: 25 MB per request (documented in README / PLAN).

#[derive(Debug, Deserialize)]
struct TranscriptionResponse {
    text: String,
}

pub fn parse_transcription_json(body: &str) -> Result<String, String> {
    let v: TranscriptionResponse =
        serde_json::from_str(body).map_err(|e| format!("Invalid transcription JSON: {e}"))?;
    Ok(v.text)
}

/// OpenAI-compatible `POST {base}/audio/transcriptions` (multipart).
pub async fn transcribe_wav_file(
    wav_path: &Path,
    base_url: &str,
    model: &str,
    api_key: &str,
    language: Option<&str>,
    cancel: &CancellationToken,
) -> Result<String, String> {
    tokio::select! {
        biased;
        _ = cancel.cancelled() => Err(JOB_CANCELLED_MSG.to_string()),
        r = transcribe_wav_file_inner(wav_path, base_url, model, api_key, language, cancel) => r,
    }
}

#[derive(Debug)]
enum TranscribeAttempt {
    Ok(String),
    Fatal(String),
    Retry(String),
}

pub(crate) fn http_status_is_retryable(status: u16) -> bool {
    status == 429 || (500..=599).contains(&status)
}

fn reqwest_error_is_retryable(e: &reqwest::Error) -> bool {
    e.is_timeout() || e.is_connect() || e.is_request()
}

async fn sleep_or_cancel(duration: Duration, cancel: &CancellationToken) -> Result<(), String> {
    tokio::select! {
        biased;
        _ = cancel.cancelled() => Err(JOB_CANCELLED_MSG.to_string()),
        _ = tokio::time::sleep(duration) => Ok(()),
    }
}

async fn transcribe_wav_http_once(
    wav_path: &Path,
    base_url: &str,
    model: &str,
    api_key: &str,
    language: Option<&str>,
) -> TranscribeAttempt {
    let key = api_key.trim();
    if key.is_empty() {
        return TranscribeAttempt::Fatal("API key is empty (Settings)".to_string());
    }

    let base = base_url.trim_end_matches('/');
    let url = format!("{base}/audio/transcriptions");

    let file_bytes = match fs::read(wav_path) {
        Ok(b) => b,
        Err(e) => return TranscribeAttempt::Fatal(format!("Failed to read WAV: {e}")),
    };

    let part = match reqwest::multipart::Part::bytes(file_bytes)
        .file_name("audio.wav")
        .mime_str("audio/wav")
    {
        Ok(p) => p,
        Err(e) => return TranscribeAttempt::Fatal(format!("multipart: {e}")),
    };

    let mut form = reqwest::multipart::Form::new()
        .part("file", part)
        .text("model", model.to_string());

    if let Some(lang) = language {
        let t = lang.trim();
        if !t.is_empty() {
            form = form.text("language", t.to_string());
        }
    }

    let client = match reqwest::Client::builder()
        .timeout(HTTP_TIMEOUT)
        .build()
    {
        Ok(c) => c,
        Err(e) => return TranscribeAttempt::Fatal(format!("HTTP client: {e}")),
    };

    let resp = match client
        .post(&url)
        .header("Authorization", format!("Bearer {key}"))
        .multipart(form)
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            let msg = format!("Transcription request failed: {e}");
            if reqwest_error_is_retryable(&e) {
                return TranscribeAttempt::Retry(msg);
            }
            return TranscribeAttempt::Fatal(msg);
        }
    };

    let status = resp.status();
    let body = match resp.text().await {
        Ok(b) => b,
        Err(e) => {
            let msg = format!("Failed to read response body: {e}");
            if status.is_server_error() || status.as_u16() == 429 {
                return TranscribeAttempt::Retry(msg);
            }
            return TranscribeAttempt::Fatal(msg);
        }
    };

    if !status.is_success() {
        let tail: String = body.chars().take(800).collect();
        let msg = format!("Transcription API error {}: {}", status.as_u16(), tail);
        if http_status_is_retryable(status.as_u16()) {
            return TranscribeAttempt::Retry(msg);
        }
        return TranscribeAttempt::Fatal(msg);
    }

    match parse_transcription_json(&body) {
        Ok(t) => TranscribeAttempt::Ok(t),
        Err(e) => TranscribeAttempt::Fatal(e),
    }
}

async fn transcribe_wav_file_inner(
    wav_path: &Path,
    base_url: &str,
    model: &str,
    api_key: &str,
    language: Option<&str>,
    cancel: &CancellationToken,
) -> Result<String, String> {
    let mut last_err = String::new();
    for attempt in 0..3 {
        if attempt > 0 {
            sleep_or_cancel(HTTP_RETRY_DELAYS[attempt - 1], cancel).await?;
        }
        match transcribe_wav_http_once(wav_path, base_url, model, api_key, language).await {
            TranscribeAttempt::Ok(s) => return Ok(s),
            TranscribeAttempt::Fatal(e) => return Err(e),
            TranscribeAttempt::Retry(msg) => last_err = msg,
        }
    }
    Err(last_err)
}

pub(crate) fn pcm_payload_bytes(path: &Path) -> Result<u64, String> {
    let len = fs::metadata(path)
        .map_err(|e| format!("metadata: {e}"))?
        .len();
    Ok(len.saturating_sub(WAV_HEADER_SKIP))
}

/// Stable id for resume checkpoints: source mtime (seconds) + file size.
pub(crate) fn wav_source_fingerprint(path: &Path) -> Result<String, String> {
    let meta = fs::metadata(path).map_err(|e| format!("metadata: {e}"))?;
    let len = meta.len();
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    Ok(format!("{mtime:x}-{len:x}"))
}

fn api_chunk_checkpoint_path(work_dir: &Path, fp: &str, i: u32) -> PathBuf {
    work_dir.join(format!("v2t-api-{fp}-chunk-{i}.txt"))
}

fn cleanup_api_chunk_checkpoints(work_dir: &Path, fp: &str) {
    let prefix = format!("v2t-api-{fp}-chunk-");
    let Ok(rd) = fs::read_dir(work_dir) else {
        return;
    };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with(&prefix) && name.ends_with(".txt") {
            let _ = fs::remove_file(e.path());
        }
    }
}

/// If WAV is large, split with ffmpeg into ~8 min PCM chunks, transcribe each, join text.
pub async fn transcribe_wav_maybe_split(
    wav_path: &Path,
    ffmpeg: &Path,
    work_dir: &Path,
    base_url: &str,
    model: &str,
    api_key: &str,
    language: Option<&str>,
    cancel: &CancellationToken,
) -> Result<String, String> {
    let payload = pcm_payload_bytes(wav_path)?;
    if payload <= MAX_UPLOAD_BYTES {
        return transcribe_wav_file(wav_path, base_url, model, api_key, language, cancel).await;
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
            return Err("Chunk split safety limit exceeded".to_string());
        }

        let checkpoint = api_chunk_checkpoint_path(work_dir, &fp, i);
        if checkpoint.is_file() {
            let saved = fs::read_to_string(&checkpoint).map_err(|e| format!("read checkpoint: {e}"))?;
            if !saved.trim().is_empty() {
                parts.push(saved);
                start += CHUNK_SECS;
                i += 1;
                continue;
            }
        }

        let chunk_path: PathBuf = work_dir.join(format!("v2t-api-chunk-{i}.wav"));
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
                "ffmpeg chunk failed: {}",
                pipeline::tail_stderr(&out.stderr)
            ));
        }

        let piece =
            transcribe_wav_file(&chunk_path, base_url, model, api_key, language, cancel).await?;
        fs::write(&checkpoint, piece.as_bytes()).map_err(|e| format!("write checkpoint: {e}"))?;
        parts.push(piece);
        let _ = fs::remove_file(&chunk_path);
        start += CHUNK_SECS;
        i += 1;
    }

    let out = parts.join("\n\n");
    cleanup_api_chunk_checkpoints(work_dir, &fp);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_openai_shape() {
        let t = parse_transcription_json(r#"{"text":"hello world"}"#).unwrap();
        assert_eq!(t, "hello world");
    }

    #[test]
    fn parse_invalid_json_errors() {
        assert!(parse_transcription_json("not json").is_err());
    }

    #[test]
    fn http_retry_codes() {
        assert!(http_status_is_retryable(429));
        assert!(http_status_is_retryable(503));
        assert!(http_status_is_retryable(500));
        assert!(!http_status_is_retryable(401));
        assert!(!http_status_is_retryable(400));
    }

    #[test]
    fn pcm_payload_skips_header() {
        let dir = std::env::temp_dir().join("v2t-pcm-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("x.wav");
        std::fs::write(&p, vec![0u8; 64 + 32000]).unwrap();
        let pl = pcm_payload_bytes(&p).unwrap();
        assert_eq!(pl, 32000);
    }

    #[test]
    fn wav_fingerprint_hex_pair() {
        let dir = std::env::temp_dir().join("v2t-fp-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("a.wav");
        std::fs::write(&p, b"x").unwrap();
        let fp = wav_source_fingerprint(&p).unwrap();
        assert!(fp.contains('-'));
        assert_eq!(wav_source_fingerprint(&p).unwrap(), fp);
    }
}
