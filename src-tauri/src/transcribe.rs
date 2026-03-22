use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

use crate::pipeline::{self, JOB_CANCELLED_MSG};

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
        r = transcribe_wav_file_inner(wav_path, base_url, model, api_key, language) => r,
    }
}

async fn transcribe_wav_file_inner(
    wav_path: &Path,
    base_url: &str,
    model: &str,
    api_key: &str,
    language: Option<&str>,
) -> Result<String, String> {
    let key = api_key.trim();
    if key.is_empty() {
        return Err("API key is empty (Settings)".to_string());
    }

    let base = base_url.trim_end_matches('/');
    let url = format!("{base}/audio/transcriptions");

    let file_bytes = fs::read(wav_path).map_err(|e| format!("Failed to read WAV: {e}"))?;

    let part = reqwest::multipart::Part::bytes(file_bytes)
        .file_name("audio.wav")
        .mime_str("audio/wav")
        .map_err(|e| format!("multipart: {e}"))?;

    let mut form = reqwest::multipart::Form::new()
        .part("file", part)
        .text("model", model.to_string());

    if let Some(lang) = language {
        let t = lang.trim();
        if !t.is_empty() {
            form = form.text("language", t.to_string());
        }
    }

    let client = reqwest::Client::builder()
        .timeout(HTTP_TIMEOUT)
        .build()
        .map_err(|e| format!("HTTP client: {e}"))?;

    let resp = client
        .post(&url)
        .header("Authorization", format!("Bearer {key}"))
        .multipart(form)
        .send()
        .await
        .map_err(|e| format!("Transcription request failed: {e}"))?;

    let status = resp.status();
    let body = resp
        .text()
        .await
        .map_err(|e| format!("Failed to read response body: {e}"))?;

    if !status.is_success() {
        let tail: String = body.chars().take(800).collect();
        return Err(format!(
            "Transcription API error {}: {}",
            status.as_u16(),
            tail
        ));
    }

    parse_transcription_json(&body)
}

pub(crate) fn pcm_payload_bytes(path: &Path) -> Result<u64, String> {
    let len = fs::metadata(path)
        .map_err(|e| format!("metadata: {e}"))?
        .len();
    Ok(len.saturating_sub(WAV_HEADER_SKIP))
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
        parts.push(piece);
        let _ = fs::remove_file(&chunk_path);
        start += CHUNK_SECS;
        i += 1;
    }

    Ok(parts.join("\n\n"))
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
    fn pcm_payload_skips_header() {
        let dir = std::env::temp_dir().join("v2t-pcm-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("x.wav");
        std::fs::write(&p, vec![0u8; 64 + 32000]).unwrap();
        let pl = pcm_payload_bytes(&p).unwrap();
        assert_eq!(pl, 32000);
    }
}
