use futures_util::StreamExt;
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

use crate::pipeline::{self, JOB_CANCELLED_MSG};
use crate::timed_transcript::{
    build_cues_from_words, chunk_checkpoint_set_key, merge_overlapping_chunk_transcripts,
    plain_text_from_segments,
    read_timed_checkpoint, require_timed_segments_for_export, secs_to_ms, sha256_file_hex,
    validate_segments_against_media_duration, write_timed_checkpoint, TimedSegment,
    TimedTranscript, TimedWord, CHUNK_OVERLAP_SECS, SEGMENT_END_DURATION_TOLERANCE_MS,
};

const HTTP_RETRY_DELAYS: [Duration; 2] = [Duration::from_secs(1), Duration::from_secs(2)];

const HTTP_TIMEOUT: Duration = Duration::from_secs(600);
pub(crate) const FFMPEG_CHUNK_TIMEOUT: Duration = Duration::from_secs(600);
/// Stay under typical 25 MB API limits (PCM 16 kHz mono ≈ 32kB/s).
pub(crate) const MAX_UPLOAD_BYTES: u64 = 22 * 1024 * 1024;
pub(crate) const CHUNK_SECS: f64 = 480.0;
const WAV_HEADER_SKIP: u64 = 64;
pub(crate) const PCM_BYTES_PER_SEC: u64 = 32000;

/// Hard cap on transcription HTTP response bodies (including chunked transfer).
/// Verbose JSON for an 8-minute chunk is far smaller; this bounds memory.
pub const MAX_TRANSCRIPTION_RESPONSE_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Deserialize)]
struct TranscriptionResponse {
    text: String,
}

#[derive(Debug, Deserialize)]
struct VerboseSegment {
    start: f64,
    end: f64,
    text: String,
}

#[derive(Debug, Deserialize)]
struct VerboseWord {
    start: f64,
    end: f64,
    #[serde(default)]
    word: Option<String>,
    #[serde(default)]
    text: Option<String>,
}

#[derive(Debug, Deserialize)]
struct VerboseTranscriptionResponse {
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    segments: Option<Vec<VerboseSegment>>,
    #[serde(default)]
    words: Option<Vec<VerboseWord>>,
}

/// Outcome of a (possibly chunked) HTTP transcription.
#[derive(Debug, Clone)]
pub struct TimedTranscribeOutcome {
    pub transcript: TimedTranscript,
    /// Stable checkpoint directory used for resume; caller cleans up after pair save.
    pub chunk_checkpoint_dir: Option<PathBuf>,
}

pub fn parse_transcription_json(body: &str) -> Result<String, String> {
    let v: TranscriptionResponse =
        serde_json::from_str(body).map_err(|e| format!("Invalid transcription JSON: {e}"))?;
    Ok(v.text)
}

/// Parse OpenAI-compatible `verbose_json` into a timed transcript.
///
/// Missing / empty / invalid `segments` for a nonempty body is a fatal compatibility error
/// (no synthetic timestamps). Empty text on any returned segment is also fatal.
/// When `media_duration_ms` is set, the latest segment end must not exceed
/// duration + [`SEGMENT_END_DURATION_TOLERANCE_MS`] (no unit guessing).
pub fn parse_verbose_transcription_json(
    body: &str,
    base_url: &str,
    model: &str,
    media_duration_ms: Option<u64>,
) -> Result<TimedTranscript, String> {
    let parsed: VerboseTranscriptionResponse = serde_json::from_str(body).map_err(|e| {
        http_timestamp_incompatibility_error(base_url, model, &format!("invalid JSON ({e})"))
    })?;

    let text_from_root = parsed.text.as_deref().unwrap_or("").trim().to_string();
    let words_raw = parsed.words.unwrap_or_default();
    let segments_raw = parsed.segments.unwrap_or_default();

    let segments =
        if let Some(from_words) = try_segments_from_verbose_words(&words_raw, base_url, model)? {
            from_words
        } else if !segments_raw.is_empty() {
            parse_verbose_segments(&segments_raw, base_url, model)?
        } else if text_from_root.is_empty() {
            return Ok(TimedTranscript::plain_text_only(String::new()));
        } else {
            return Err(http_timestamp_incompatibility_error(
                base_url,
                model,
                "response had neither usable words nor segments",
            ));
        };

    let text = if segments.is_empty() {
        text_from_root
    } else {
        plain_text_from_segments(&segments)
    };

    if !text.trim().is_empty() && segments.is_empty() {
        return Err(http_timestamp_incompatibility_error(
            base_url,
            model,
            "timestamps were present but none were usable",
        ));
    }

    if let Some(duration_ms) = media_duration_ms {
        validate_segments_against_media_duration(
            &segments,
            duration_ms,
            SEGMENT_END_DURATION_TOLERANCE_MS,
        )
        .map_err(|detail| http_timestamp_incompatibility_error(base_url, model, &detail))?;
    }

    Ok(TimedTranscript { text, segments })
}

fn try_segments_from_verbose_words(
    words_raw: &[VerboseWord],
    base_url: &str,
    model: &str,
) -> Result<Option<Vec<TimedSegment>>, String> {
    if words_raw.is_empty() {
        return Ok(None);
    }
    let mut words = Vec::with_capacity(words_raw.len());
    for (index, word) in words_raw.iter().enumerate() {
        if !word.start.is_finite() || !word.end.is_finite() {
            return Err(http_timestamp_incompatibility_error(
                base_url,
                model,
                &format!("word {index} has non-finite start/end"),
            ));
        }
        let start_ms = secs_to_ms(word.start);
        let end_ms = secs_to_ms(word.end);
        if end_ms <= start_ms {
            return Err(http_timestamp_incompatibility_error(
                base_url,
                model,
                &format!("word {index} has end <= start"),
            ));
        }
        let text = word
            .word
            .as_deref()
            .or(word.text.as_deref())
            .unwrap_or("")
            .trim();
        if text.is_empty() {
            continue;
        }
        words.push(TimedWord {
            start_ms,
            end_ms,
            text: text.to_string(),
            speaker: None,
        });
    }
    if words.is_empty() {
        return Ok(None);
    }
    let segments = build_cues_from_words(&words)
        .map_err(|detail| http_timestamp_incompatibility_error(base_url, model, &detail))?;
    if segments.is_empty() {
        return Ok(None);
    }
    Ok(Some(segments))
}

fn parse_verbose_segments(
    segments_raw: &[VerboseSegment],
    base_url: &str,
    model: &str,
) -> Result<Vec<TimedSegment>, String> {
    let mut segments = Vec::with_capacity(segments_raw.len());
    for (index, segment) in segments_raw.iter().enumerate() {
        if !segment.start.is_finite() || !segment.end.is_finite() {
            return Err(http_timestamp_incompatibility_error(
                base_url,
                model,
                &format!("segment {index} has non-finite start/end"),
            ));
        }
        let start_ms = secs_to_ms(segment.start);
        let end_ms = secs_to_ms(segment.end);
        if end_ms <= start_ms {
            return Err(http_timestamp_incompatibility_error(
                base_url,
                model,
                &format!("segment {index} has end <= start"),
            ));
        }
        let text = segment.text.trim();
        if text.is_empty() {
            return Err(http_timestamp_incompatibility_error(
                base_url,
                model,
                &format!("segment {index} has empty text"),
            ));
        }
        segments.push(TimedSegment {
            start_ms,
            end_ms,
            text: text.to_string(),
            speaker: None,
        });
    }
    Ok(segments)
}

pub(crate) fn http_timestamp_incompatibility_error(
    base_url: &str,
    model: &str,
    detail: &str,
) -> String {
    format!(
        "HTTP API WebVTT export failed: provider at {base_url} (model {model}) did not return segment timestamps ({detail}). Disable \"Export WebVTT\" or use a timestamp-capable endpoint / Local Whisper / Browser mode."
    )
}

/// OpenAI-compatible `POST {base}/audio/transcriptions` (multipart).
pub async fn transcribe_wav_file(
    wav_path: &Path,
    base_url: &str,
    model: &str,
    api_key: &str,
    language: Option<&str>,
    export_webvtt: bool,
    cancel: &CancellationToken,
) -> Result<TimedTranscript, String> {
    tokio::select! {
        biased;
        _ = cancel.cancelled() => Err(JOB_CANCELLED_MSG.to_string()),
        r = transcribe_wav_file_inner(
            wav_path,
            base_url,
            model,
            api_key,
            language,
            export_webvtt,
            cancel,
        ) => r,
    }
}

#[derive(Debug)]
enum TranscribeAttempt {
    Ok(TimedTranscript),
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

async fn read_response_body_capped(resp: reqwest::Response) -> Result<String, TranscribeAttempt> {
    if let Some(len) = resp.content_length() {
        if len as usize > MAX_TRANSCRIPTION_RESPONSE_BYTES {
            return Err(TranscribeAttempt::Fatal(format!(
                "Transcription response Content-Length {len} exceeds cap of {MAX_TRANSCRIPTION_RESPONSE_BYTES} bytes"
            )));
        }
    }

    let mut body = Vec::new();
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk
            .map_err(|e| TranscribeAttempt::Retry(format!("Failed to read response body: {e}")))?;
        if body.len().saturating_add(chunk.len()) > MAX_TRANSCRIPTION_RESPONSE_BYTES {
            return Err(TranscribeAttempt::Fatal(format!(
                "Transcription response body exceeds cap of {MAX_TRANSCRIPTION_RESPONSE_BYTES} bytes"
            )));
        }
        body.extend_from_slice(&chunk);
    }
    String::from_utf8(body).map_err(|e| {
        TranscribeAttempt::Fatal(format!("Transcription response body is not UTF-8: {e}"))
    })
}

fn wav_duration_ms(wav_path: &Path) -> Result<u64, String> {
    let payload = pcm_payload_bytes(wav_path)?;
    let secs = payload as f64 / PCM_BYTES_PER_SEC as f64;
    Ok(secs_to_ms(secs))
}

async fn transcribe_wav_http_once(
    wav_path: &Path,
    base_url: &str,
    model: &str,
    api_key: &str,
    language: Option<&str>,
    export_webvtt: bool,
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

    if export_webvtt {
        form = form
            .text("response_format", "verbose_json".to_string())
            .text("timestamp_granularities[]", "segment".to_string())
            .text("timestamp_granularities[]", "word".to_string());
    }

    if let Some(lang) = language {
        let t = lang.trim();
        if !t.is_empty() {
            form = form.text("language", t.to_string());
        }
    }

    let client = match reqwest::Client::builder().timeout(HTTP_TIMEOUT).build() {
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
    let body = match read_response_body_capped(resp).await {
        Ok(b) => b,
        Err(attempt) => return attempt,
    };

    if !status.is_success() {
        let tail: String = body.chars().take(800).collect();
        let msg = format!("Transcription API error {}: {}", status.as_u16(), tail);
        if http_status_is_retryable(status.as_u16()) {
            return TranscribeAttempt::Retry(msg);
        }
        return TranscribeAttempt::Fatal(msg);
    }

    if export_webvtt {
        let duration_ms = match wav_duration_ms(wav_path) {
            Ok(ms) => Some(ms),
            Err(e) => return TranscribeAttempt::Fatal(e),
        };
        match parse_verbose_transcription_json(&body, base, model, duration_ms) {
            Ok(t) => TranscribeAttempt::Ok(t),
            // Compatibility miss on HTTP 200 is fatal — do not retry the same plain/no-segment shape.
            Err(e) => TranscribeAttempt::Fatal(e),
        }
    } else {
        match parse_transcription_json(&body) {
            Ok(t) => TranscribeAttempt::Ok(TimedTranscript::plain_text_only(t)),
            Err(e) => TranscribeAttempt::Fatal(e),
        }
    }
}

async fn transcribe_wav_file_inner(
    wav_path: &Path,
    base_url: &str,
    model: &str,
    api_key: &str,
    language: Option<&str>,
    export_webvtt: bool,
    cancel: &CancellationToken,
) -> Result<TimedTranscript, String> {
    let mut last_err = String::new();
    for attempt in 0..3 {
        if attempt > 0 {
            sleep_or_cancel(HTTP_RETRY_DELAYS[attempt - 1], cancel).await?;
        }
        match transcribe_wav_http_once(wav_path, base_url, model, api_key, language, export_webvtt)
            .await
        {
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

/// Build a stable checkpoint-set key from WAV content + transcription contract.
///
/// `provider_scope` is a normalized HTTP `api_base_url` or [`LOCAL_WHISPER_PROVIDER_SCOPE`].
pub fn build_chunk_checkpoint_key(
    wav_path: &Path,
    mode: &str,
    provider_scope: &str,
    model: &str,
    language: Option<&str>,
    export_webvtt: bool,
) -> Result<String, String> {
    use crate::timed_transcript::CHECKPOINT_SCHEMA_VERSION;
    let content = sha256_file_hex(wav_path)?;
    Ok(chunk_checkpoint_set_key(
        &content,
        mode,
        provider_scope,
        model,
        language,
        export_webvtt,
        CHECKPOINT_SCHEMA_VERSION,
        CHUNK_SECS,
    ))
}

fn api_chunk_checkpoint_path(checkpoint_dir: &Path, i: u32, timed: bool) -> PathBuf {
    let ext = if timed { "json" } else { "txt" };
    checkpoint_dir.join(format!("chunk-{i}.{ext}"))
}

/// Remove API chunk checkpoints in a stable checkpoint directory.
pub fn cleanup_api_chunk_checkpoints(checkpoint_dir: &Path) {
    let Ok(rd) = fs::read_dir(checkpoint_dir) else {
        return;
    };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with("chunk-") && (name.ends_with(".txt") || name.ends_with(".json")) {
            let _ = fs::remove_file(e.path());
        }
    }
    let _ = fs::remove_dir(checkpoint_dir);
}

fn read_plain_checkpoint(path: &Path) -> Result<Option<String>, String> {
    match fs::read_to_string(path) {
        Ok(saved) if !saved.trim().is_empty() => Ok(Some(saved)),
        Ok(_) => Ok(None),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("read checkpoint: {e}")),
    }
}

/// If WAV is large, split with ffmpeg into ~8 min PCM chunks, transcribe each, join text.
///
/// `work_dir` holds temporary chunk WAV files. `checkpoint_dir` is a stable app-data
/// cache keyed by content+settings so retries across unique work dirs can resume.
#[allow(clippy::too_many_arguments)]
pub async fn transcribe_wav_maybe_split(
    wav_path: &Path,
    ffmpeg: &Path,
    work_dir: &Path,
    checkpoint_dir: &Path,
    base_url: &str,
    model: &str,
    api_key: &str,
    language: Option<&str>,
    export_webvtt: bool,
    cancel: &CancellationToken,
) -> Result<TimedTranscribeOutcome, String> {
    let payload = pcm_payload_bytes(wav_path)?;
    if payload <= MAX_UPLOAD_BYTES {
        let transcript = transcribe_wav_file(
            wav_path,
            base_url,
            model,
            api_key,
            language,
            export_webvtt,
            cancel,
        )
        .await?;
        require_timed_segments_for_export(&transcript, export_webvtt, "HTTP API")?;
        return Ok(TimedTranscribeOutcome {
            transcript,
            chunk_checkpoint_dir: None,
        });
    }

    fs::create_dir_all(checkpoint_dir).map_err(|e| format!("create checkpoint dir: {e}"))?;
    let duration_sec = (payload as f64 / PCM_BYTES_PER_SEC as f64).max(1.0);
    let mut start = 0.0f64;
    let mut i = 0u32;
    let mut chunks: Vec<(u64, TimedTranscript)> = Vec::new();
    let chunk_step = if export_webvtt {
        CHUNK_SECS - CHUNK_OVERLAP_SECS
    } else {
        CHUNK_SECS
    };
    let max_chunks = ((duration_sec / chunk_step).ceil() as u32).saturating_add(4);

    while start < duration_sec - 0.05 {
        if cancel.is_cancelled() {
            return Err(JOB_CANCELLED_MSG.to_string());
        }
        if i >= max_chunks {
            return Err("Chunk split safety limit exceeded".to_string());
        }

        let chunk_start_ms = secs_to_ms(start);
        let checkpoint = api_chunk_checkpoint_path(checkpoint_dir, i, export_webvtt);

        if export_webvtt {
            if let Some((stored_start, saved)) = read_timed_checkpoint(&checkpoint)? {
                let offset = if stored_start > 0 {
                    stored_start
                } else {
                    chunk_start_ms
                };
                chunks.push((offset, saved));
                start += chunk_step;
                i += 1;
                continue;
            }
        } else if let Some(saved) = read_plain_checkpoint(&checkpoint)? {
            chunks.push((chunk_start_ms, TimedTranscript::plain_text_only(saved)));
            start += chunk_step;
            i += 1;
            continue;
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

        let piece = transcribe_wav_file(
            &chunk_path,
            base_url,
            model,
            api_key,
            language,
            export_webvtt,
            cancel,
        )
        .await?;
        require_timed_segments_for_export(&piece, export_webvtt, "HTTP API")?;

        if export_webvtt {
            write_timed_checkpoint(&checkpoint, &piece, chunk_start_ms)?;
        } else {
            fs::write(&checkpoint, piece.text.as_bytes())
                .map_err(|e| format!("write checkpoint: {e}"))?;
        }
        chunks.push((chunk_start_ms, piece));
        let _ = fs::remove_file(&chunk_path);
        start += chunk_step;
        i += 1;
    }

    let transcript = if export_webvtt {
        let overlap_ms = secs_to_ms(CHUNK_OVERLAP_SECS);
        merge_overlapping_chunk_transcripts(chunks, overlap_ms)?
    } else {
        TimedTranscript::plain_text_only(
            chunks
                .into_iter()
                .map(|(_, t)| t.text)
                .collect::<Vec<_>>()
                .join("\n\n"),
        )
    };

    Ok(TimedTranscribeOutcome {
        transcript,
        chunk_checkpoint_dir: Some(checkpoint_dir.to_path_buf()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timed_transcript::{merge_chunk_transcripts, write_timed_checkpoint};

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
    fn parses_verbose_json_segments_to_ms() {
        let body = r#"{
            "text": "Hello world",
            "segments": [
                {"start": 0.0, "end": 1.5, "text": " Hello"},
                {"start": 1.5, "end": 2.25, "text": " world"}
            ]
        }"#;
        let timed = parse_verbose_transcription_json(
            body,
            "https://api.example/v1",
            "whisper-1",
            Some(3_000),
        )
        .unwrap();
        assert_eq!(timed.text, "Hello world");
        assert_eq!(timed.segments.len(), 2);
        assert_eq!(timed.segments[0].start_ms, 0);
        assert_eq!(timed.segments[0].end_ms, 1_500);
        assert_eq!(timed.segments[1].start_ms, 1_500);
        assert_eq!(timed.segments[1].end_ms, 2_250);
        assert_eq!(timed.segments[0].text, "Hello");
    }

    #[test]
    fn verbose_json_without_segments_is_fatal_compatibility_error() {
        let err = parse_verbose_transcription_json(
            r#"{"text":"only plain"}"#,
            "https://api.example/v1",
            "whisper-1",
            None,
        )
        .unwrap_err();
        assert!(err.contains("HTTP API"));
        assert!(err.contains("https://api.example/v1"));
        assert!(err.contains("whisper-1"));
        assert!(err.contains("WebVTT"));
        assert!(err.contains("neither usable words nor segments"));
        assert!(!err.to_lowercase().contains("falling back to local"));
    }

    #[test]
    fn verbose_json_prefers_words_over_segments() {
        let body = r#"{
            "text": "Hello world.",
            "segments": [
                {"start": 0.0, "end": 5.0, "text": " ignored long segment"}
            ],
            "words": [
                {"start": 0.0, "end": 0.4, "word": "Hello"},
                {"start": 0.45, "end": 0.9, "word": "world."}
            ]
        }"#;
        let timed = parse_verbose_transcription_json(
            body,
            "https://api.example/v1",
            "whisper-1",
            Some(2_000),
        )
        .unwrap();
        assert_eq!(timed.segments.len(), 1);
        assert_eq!(timed.segments[0].text, "Hello world.");
        assert_eq!(timed.segments[0].end_ms, 900);
    }

    #[test]
    fn verbose_json_invalid_segment_bounds_are_fatal() {
        let err = parse_verbose_transcription_json(
            r#"{"text":"x","segments":[{"start":2.0,"end":1.0,"text":"bad"}]}"#,
            "https://api.example/v1",
            "m",
            None,
        )
        .unwrap_err();
        assert!(err.contains("HTTP API"));
        assert!(err.contains("end <= start"));
    }

    #[test]
    fn verbose_json_empty_segment_text_is_fatal() {
        let err = parse_verbose_transcription_json(
            r#"{"text":"x","segments":[{"start":0.0,"end":1.0,"text":"  "}]}"#,
            "https://api.example/v1",
            "m",
            None,
        )
        .unwrap_err();
        assert!(err.contains("empty text"));
    }

    #[test]
    fn verbose_json_rejects_millisecond_scale_vs_wav_duration() {
        // Provider returned ms as if they were seconds → end far beyond a 2s WAV.
        let err = parse_verbose_transcription_json(
            r#"{"text":"x","segments":[{"start":0.0,"end":5000.0,"text":"bad units"}]}"#,
            "https://api.example/v1",
            "m",
            Some(2_000),
        )
        .unwrap_err();
        assert!(err.contains("exceeds media duration"));
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
    fn checkpoint_key_stable_for_same_wav_and_contract() {
        let dir = std::env::temp_dir().join("v2t-fp-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("a.wav");
        std::fs::write(&p, b"normalized-wav-bytes").unwrap();
        let scope = "https://api.example/v1";
        let a = build_chunk_checkpoint_key(&p, "httpApi", scope, "whisper-1", Some("en"), true)
            .unwrap();
        let b = build_chunk_checkpoint_key(&p, "httpApi", scope, "whisper-1", Some("en"), true)
            .unwrap();
        let c = build_chunk_checkpoint_key(&p, "httpApi", scope, "whisper-1", Some("en"), false)
            .unwrap();
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_eq!(a.len(), 64);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn checkpoint_key_changes_with_http_endpoint() {
        let dir = std::env::temp_dir().join("v2t-fp-endpoint");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("a.wav");
        std::fs::write(&p, b"wav").unwrap();
        let a = build_chunk_checkpoint_key(
            &p,
            "httpApi",
            "https://api.openai.com/v1",
            "whisper-1",
            None,
            true,
        )
        .unwrap();
        let b = build_chunk_checkpoint_key(
            &p,
            "httpApi",
            "https://api.other.com/v1",
            "whisper-1",
            None,
            true,
        )
        .unwrap();
        let slash = build_chunk_checkpoint_key(
            &p,
            "httpApi",
            &crate::timed_transcript::normalize_http_provider_scope("https://api.openai.com/v1/"),
            "whisper-1",
            None,
            true,
        )
        .unwrap();
        assert_ne!(a, b);
        assert_eq!(a, slash);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn checkpoint_key_encodes_schema_and_chunk_secs() {
        use crate::timed_transcript::{chunk_checkpoint_set_key, CHECKPOINT_SCHEMA_VERSION};
        let with_current = chunk_checkpoint_set_key(
            "deadbeef",
            "httpApi",
            "https://api.example/v1",
            "m",
            None,
            true,
            CHECKPOINT_SCHEMA_VERSION,
            CHUNK_SECS,
        );
        let other_schema = chunk_checkpoint_set_key(
            "deadbeef",
            "httpApi",
            "https://api.example/v1",
            "m",
            None,
            true,
            CHECKPOINT_SCHEMA_VERSION + 1,
            CHUNK_SECS,
        );
        let other_chunk = chunk_checkpoint_set_key(
            "deadbeef",
            "httpApi",
            "https://api.example/v1",
            "m",
            None,
            true,
            CHECKPOINT_SCHEMA_VERSION,
            CHUNK_SECS / 2.0,
        );
        assert_ne!(with_current, other_schema);
        assert_ne!(with_current, other_chunk);
        // build_chunk_checkpoint_key must wire the same schema + CHUNK_SECS.
        let dir = std::env::temp_dir().join("v2t-fp-schema");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("a.wav");
        std::fs::write(&p, b"x").unwrap();
        let from_builder =
            build_chunk_checkpoint_key(&p, "httpApi", "https://api.example/v1", "m", None, true)
                .unwrap();
        let content = sha256_file_hex(&p).unwrap();
        let expected = chunk_checkpoint_set_key(
            &content,
            "httpApi",
            "https://api.example/v1",
            "m",
            None,
            true,
            CHECKPOINT_SCHEMA_VERSION,
            CHUNK_SECS,
        );
        assert_eq!(from_builder, expected);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn timed_checkpoint_resume_uses_stored_chunk_start() {
        let dir = std::env::temp_dir().join("v2t-api-ckpt-test");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("chunk-1.json");
        let transcript = TimedTranscript {
            text: "later".into(),
            segments: vec![TimedSegment {
                start_ms: 0,
                end_ms: 100,
                text: "later".into(),
                speaker: None,
            }],
        };
        write_timed_checkpoint(&path, &transcript, 480_000).unwrap();
        let (offset, restored) = read_timed_checkpoint(&path).unwrap().unwrap();
        assert_eq!(offset, 480_000);
        assert_eq!(restored.text, "later");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn legacy_txt_checkpoint_ignored_when_reading_timed_path() {
        let dir = std::env::temp_dir().join("v2t-api-legacy-ckpt");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("chunk-0.txt");
        fs::write(&path, "legacy").unwrap();
        assert!(read_timed_checkpoint(&path).unwrap().is_none());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn cleanup_removes_checkpoint_dir_after_pair() {
        let dir = std::env::temp_dir().join("v2t-api-cleanup");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("chunk-0.txt"), "a").unwrap();
        fs::write(dir.join("chunk-1.json"), "{}").unwrap();
        cleanup_api_chunk_checkpoints(&dir);
        assert!(!dir.exists());
    }

    #[test]
    fn resume_saved_chunk_then_remaining_merge_equivalence() {
        let saved = TimedTranscript {
            text: "first".into(),
            segments: vec![TimedSegment {
                start_ms: 0,
                end_ms: 100,
                text: "first".into(),
                speaker: None,
            }],
        };
        let second = TimedTranscript {
            text: "second".into(),
            segments: vec![TimedSegment {
                start_ms: 0,
                end_ms: 50,
                text: "second".into(),
                speaker: None,
            }],
        };
        let resumed =
            merge_chunk_transcripts(vec![(0, saved.clone()), (480_000, second.clone())]).unwrap();
        let fresh = merge_chunk_transcripts(vec![(0, saved), (480_000, second)]).unwrap();
        assert_eq!(resumed, fresh);
        assert_eq!(resumed.segments[1].start_ms, 480_000);
    }

    #[test]
    fn response_body_cap_constant_is_documented_bound() {
        assert_eq!(MAX_TRANSCRIPTION_RESPONSE_BYTES, 16 * 1024 * 1024);
    }
}
