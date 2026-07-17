use serde::{Deserialize, Serialize};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// One factual transcription segment on its source track's timeline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimedSegment {
    #[serde(alias = "start_ms")]
    pub start_ms: u64,
    #[serde(alias = "end_ms")]
    pub end_ms: u64,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speaker: Option<String>,
}

/// One factual word-level timing used to rebuild WebVTT cues.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimedWord {
    #[serde(alias = "start_ms")]
    pub start_ms: u64,
    #[serde(alias = "end_ms")]
    pub end_ms: u64,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speaker: Option<String>,
}

/// Overlap between consecutive media chunks when exporting WebVTT (seconds).
pub const CHUNK_OVERLAP_SECS: f64 = 2.5;

const CUE_MAX_CHARS: usize = 50;
const CUE_MAX_DURATION_MS: u64 = 10_000;
const CUE_GAP_BREAK_MS: u64 = 500;
const OVERLAP_NEAR_DUP_START_MS: u64 = 800;

/// A plain transcript paired with factual segment-level timings when available.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimedTranscript {
    pub text: String,
    pub segments: Vec<TimedSegment>,
}

impl TimedTranscript {
    /// Build the legacy plain-text result without inventing timestamps.
    pub fn plain_text_only(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            segments: Vec::new(),
        }
    }
}

/// Reject unusable source segments. Ordering is deliberately not checked here:
/// a formatter preserves source order, while chunk merging sorts explicitly.
pub fn validate_segments(segments: &[TimedSegment]) -> Result<(), String> {
    for (index, segment) in segments.iter().enumerate() {
        if segment.end_ms <= segment.start_ms {
            return Err(format!(
                "Invalid timed segment {index}: end_ms must be greater than start_ms"
            ));
        }
        if segment.text.trim().is_empty() {
            return Err(format!(
                "Invalid timed segment {index}: text must not be empty"
            ));
        }
    }
    Ok(())
}

/// Format milliseconds as a WebVTT timestamp. Hours are never truncated.
pub fn format_timestamp(milliseconds: u64) -> String {
    let hours = milliseconds / 3_600_000;
    let minutes = (milliseconds / 60_000) % 60;
    let seconds = (milliseconds / 1_000) % 60;
    let millis = milliseconds % 1_000;
    format!("{hours:02}:{minutes:02}:{seconds:02}.{millis:03}")
}

/// Parse the strict timestamp shape emitted by [`format_timestamp`].
pub fn parse_timestamp(timestamp: &str) -> Result<u64, String> {
    let (clock, millis) = timestamp
        .split_once('.')
        .ok_or_else(|| format!("Invalid WebVTT timestamp: {timestamp}"))?;
    if millis.len() != 3 || !millis.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(format!("Invalid WebVTT timestamp: {timestamp}"));
    }

    let mut parts = clock.split(':');
    let hours = parts
        .next()
        .filter(|part| part.len() >= 2 && part.bytes().all(|byte| byte.is_ascii_digit()))
        .ok_or_else(|| format!("Invalid WebVTT timestamp: {timestamp}"))?;
    let minutes = parts
        .next()
        .filter(|part| part.len() == 2 && part.bytes().all(|byte| byte.is_ascii_digit()))
        .ok_or_else(|| format!("Invalid WebVTT timestamp: {timestamp}"))?;
    let seconds = parts
        .next()
        .filter(|part| part.len() == 2 && part.bytes().all(|byte| byte.is_ascii_digit()))
        .ok_or_else(|| format!("Invalid WebVTT timestamp: {timestamp}"))?;
    if parts.next().is_some() {
        return Err(format!("Invalid WebVTT timestamp: {timestamp}"));
    }

    let hours = hours
        .parse::<u64>()
        .map_err(|_| format!("Invalid WebVTT timestamp: {timestamp}"))?;
    let minutes = minutes
        .parse::<u64>()
        .map_err(|_| format!("Invalid WebVTT timestamp: {timestamp}"))?;
    let seconds = seconds
        .parse::<u64>()
        .map_err(|_| format!("Invalid WebVTT timestamp: {timestamp}"))?;
    let millis = millis
        .parse::<u64>()
        .map_err(|_| format!("Invalid WebVTT timestamp: {timestamp}"))?;
    if minutes >= 60 || seconds >= 60 {
        return Err(format!("Invalid WebVTT timestamp: {timestamp}"));
    }

    hours
        .checked_mul(3_600_000)
        .and_then(|value| value.checked_add(minutes * 60_000))
        .and_then(|value| value.checked_add(seconds * 1_000))
        .and_then(|value| value.checked_add(millis))
        .ok_or_else(|| format!("WebVTT timestamp overflow: {timestamp}"))
}

/// Serialize factual segments as a WebVTT document.
pub fn format_webvtt(segments: &[TimedSegment]) -> Result<String, String> {
    validate_segments(segments)?;

    let mut output = String::from("WEBVTT\n\n");
    for segment in segments {
        writeln!(
            output,
            "{} --> {}",
            format_timestamp(segment.start_ms),
            format_timestamp(segment.end_ms)
        )
        .expect("writing to String cannot fail");
        let payload = wrap_speaker_payload(segment);
        output.push_str(&escape_cue_payload(&payload));
        output.push_str("\n\n");
    }
    if !segments.is_empty() {
        output.pop();
    }
    Ok(output)
}

fn wrap_speaker_payload(segment: &TimedSegment) -> String {
    let Some(speaker) = segment.speaker.as_deref() else {
        return segment.text.clone();
    };
    if segment.text.contains("<v ") {
        return segment.text.clone();
    }
    let Some(sanitized) = sanitize_voice_name(speaker) else {
        return segment.text.clone();
    };
    format!("<v {sanitized}>{}</v>", segment.text)
}

/// Parse a leading WebVTT voice tag into `(speaker, body without the wrapper)`.
///
/// Accepts `<v Name>…</v>` (optional trailing text after `</v>`) or `<v Name>rest`.
pub fn extract_voice_speaker(text: &str) -> (Option<String>, String) {
    let trimmed = text.trim_start();
    let Some(after_v) = trimmed.strip_prefix("<v ") else {
        return (None, text.to_string());
    };
    let Some(name_end) = after_v.find('>') else {
        return (None, text.to_string());
    };
    let name = after_v[..name_end].trim();
    if name.is_empty() {
        return (None, text.to_string());
    }
    let after_open = &after_v[name_end + 1..];
    if let Some(close_at) = after_open.find("</v>") {
        let inner = &after_open[..close_at];
        let after_close = &after_open[close_at + 4..];
        let body = format!("{inner}{after_close}");
        return (Some(name.to_string()), body);
    }
    (Some(name.to_string()), after_open.to_string())
}

/// Rebuild cue-sized segments from factual word timings (no invented timestamps).
pub fn build_cues_from_words(words: &[TimedWord]) -> Result<Vec<TimedSegment>, String> {
    let mut usable: Vec<TimedWord> = Vec::with_capacity(words.len());
    for (index, word) in words.iter().enumerate() {
        let text = word.text.trim();
        if text.is_empty() {
            continue;
        }
        if word.end_ms <= word.start_ms {
            return Err(format!(
                "Invalid timed word {index}: end_ms must be greater than start_ms"
            ));
        }
        usable.push(TimedWord {
            start_ms: word.start_ms,
            end_ms: word.end_ms,
            text: text.to_string(),
            speaker: word.speaker.clone(),
        });
    }
    if usable.is_empty() {
        return Ok(Vec::new());
    }

    let mut segments = Vec::new();
    let mut cue_start = 0usize;
    let mut index = 0usize;
    while index < usable.len() {
        let ends_sentence = word_ends_sentence(&usable[index].text);
        let is_last = index + 1 >= usable.len();
        let break_after = if is_last {
            true
        } else {
            let next = index + 1;
            let gap = usable[next].start_ms.saturating_sub(usable[index].end_ms);
            let chars_with_next = cue_char_len(&usable[cue_start..=next]);
            let duration_with_next = usable[next]
                .end_ms
                .saturating_sub(usable[cue_start].start_ms);
            ends_sentence
                || gap >= CUE_GAP_BREAK_MS
                || chars_with_next > CUE_MAX_CHARS
                || duration_with_next > CUE_MAX_DURATION_MS
        };

        if break_after {
            segments.push(cue_from_words(&usable[cue_start..=index]));
            cue_start = index + 1;
        }
        index += 1;
    }

    validate_segments(&segments)?;
    Ok(segments)
}

fn word_ends_sentence(text: &str) -> bool {
    text.chars()
        .last()
        .is_some_and(|ch| matches!(ch, '.' | '?' | '!' | '。'))
}

fn cue_char_len(words: &[TimedWord]) -> usize {
    let mut len = 0usize;
    for (i, word) in words.iter().enumerate() {
        if i > 0 {
            len += 1;
        }
        len += word.text.chars().count();
    }
    len
}

fn cue_from_words(words: &[TimedWord]) -> TimedSegment {
    let start_ms = words[0].start_ms;
    let end_ms = words[words.len() - 1].end_ms;
    let text = words
        .iter()
        .map(|word| word.text.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    let speaker = {
        let first = &words[0].speaker;
        if words.iter().all(|word| &word.speaker == first) {
            first.clone()
        } else {
            None
        }
    };
    TimedSegment {
        start_ms,
        end_ms,
        text,
        speaker,
    }
}

fn escape_cue_payload(text: &str) -> String {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut output = String::with_capacity(normalized.len());
    let mut remainder = normalized.as_str();

    while let Some(position) = remainder.find('<') {
        escape_plain_payload(&remainder[..position], &mut output);
        remainder = &remainder[position..];
        if let Some(close) = remainder.find('>') {
            let candidate = &remainder[..=close];
            if let Some(preserved) = preserve_safe_webvtt_tag(candidate) {
                output.push_str(&preserved);
                remainder = &remainder[close + 1..];
                continue;
            }
        }
        output.push_str("&lt;");
        remainder = &remainder['<'.len_utf8()..];
    }
    escape_plain_payload(remainder, &mut output);
    output
}

fn escape_plain_payload(text: &str, output: &mut String) {
    for character in text.chars() {
        match character {
            '&' => output.push_str("&amp;"),
            '>' => output.push_str("&gt;"),
            '\0' => output.push('\u{fffd}'),
            _ => output.push(character),
        }
    }
}

/// Preserve only exact safe basic tags (`<i>`, `</b>`, …) and spec-like voice tags
/// (`<v Alice>`, `</v>`) with a sanitized voice name. Attributes / unknown tags escape.
fn preserve_safe_webvtt_tag(candidate: &str) -> Option<String> {
    let inner = candidate.strip_prefix('<')?.strip_suffix('>')?;
    if inner.is_empty() || inner.contains(['<', '>', '\0', '\n', '\r']) {
        return None;
    }

    const BASIC: &[&str] = &[
        "i", "/i", "b", "/b", "u", "/u", "c", "/c", "ruby", "/ruby", "rt", "/rt", "lang", "/lang",
        "/v",
    ];
    if BASIC.iter().any(|tag| inner == *tag) {
        return Some(candidate.to_string());
    }

    // Spec-like voice open tag: `<v Voice Name>` (no attributes / classes).
    let voice_name = inner.strip_prefix("v ")?;
    let sanitized = sanitize_voice_name(voice_name)?;
    Some(format!("<v {sanitized}>"))
}

fn sanitize_voice_name(voice_name: &str) -> Option<String> {
    if voice_name.is_empty() {
        return None;
    }
    let sanitized: String = voice_name
        .chars()
        .map(|ch| match ch {
            c if c.is_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.' | '\'') => c,
            _ => '\u{fffd}',
        })
        .collect();
    if sanitized
        .chars()
        .all(|c| c == '\u{fffd}' || c.is_whitespace())
    {
        return None;
    }
    Some(sanitized)
}

/// Shift chunk-relative segments onto a track timeline with checked arithmetic.
pub fn offset_segments(
    segments: &[TimedSegment],
    offset_ms: u64,
) -> Result<Vec<TimedSegment>, String> {
    validate_segments(segments)?;
    segments
        .iter()
        .enumerate()
        .map(|(index, segment)| {
            let start_ms = segment.start_ms.checked_add(offset_ms).ok_or_else(|| {
                format!("Timed segment {index} start millisecond offset overflow")
            })?;
            let end_ms = segment
                .end_ms
                .checked_add(offset_ms)
                .ok_or_else(|| format!("Timed segment {index} end millisecond offset overflow"))?;
            Ok(TimedSegment {
                start_ms,
                end_ms,
                text: segment.text.clone(),
                speaker: segment.speaker.clone(),
            })
        })
        .collect()
}

/// Merge chunk transcripts by chunk offset and stable chronological cue order.
pub fn merge_chunk_transcripts(
    chunks: Vec<(u64, TimedTranscript)>,
) -> Result<TimedTranscript, String> {
    merge_overlapping_chunk_transcripts(chunks, 0)
}

/// Merge chunk transcripts, optionally dropping overlap-region duplicates.
///
/// When `overlap_ms == 0`, behaves like a plain offset+sort merge.
/// When `overlap_ms > 0`, later-chunk segments that start before
/// `later_offset + overlap_ms/2`, or that near-duplicate an earlier segment
/// (normalized text, start within 800 ms), are dropped.
pub fn merge_overlapping_chunk_transcripts(
    mut chunks: Vec<(u64, TimedTranscript)>,
    overlap_ms: u64,
) -> Result<TimedTranscript, String> {
    chunks.sort_by_key(|(offset_ms, _)| *offset_ms);

    let mut texts = Vec::with_capacity(chunks.len());
    let mut segments = Vec::new();
    for (offset_ms, transcript) in chunks {
        texts.push(transcript.text);
        let mut offset = offset_segments(&transcript.segments, offset_ms)?;
        if overlap_ms > 0 && !segments.is_empty() {
            let cutoff = offset_ms.saturating_add(overlap_ms / 2);
            offset.retain(|segment| {
                if segment.start_ms < cutoff {
                    return false;
                }
                !segments.iter().any(|earlier| {
                    near_duplicate_segment(earlier, segment, OVERLAP_NEAR_DUP_START_MS)
                })
            });
        }
        segments.extend(offset);
    }
    segments.sort_by_key(|segment| (segment.start_ms, segment.end_ms));
    validate_segments(&segments)?;

    Ok(TimedTranscript {
        text: texts.join("\n\n"),
        segments,
    })
}

fn near_duplicate_segment(a: &TimedSegment, b: &TimedSegment, start_tol_ms: u64) -> bool {
    let start_diff = a.start_ms.abs_diff(b.start_ms);
    if start_diff >= start_tol_ms {
        return false;
    }
    normalize_cue_text(&a.text) == normalize_cue_text(&b.text)
}

fn normalize_cue_text(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// Convert a floating-point second offset (ffmpeg `-ss`) into whole milliseconds.
pub fn secs_to_ms(secs: f64) -> u64 {
    if !secs.is_finite() || secs <= 0.0 {
        0
    } else {
        (secs * 1000.0).round() as u64
    }
}

/// Conventional chunk start for index `i` using the shared `CHUNK_SECS` constant.
pub fn chunk_start_ms_for_index(chunk_index: u32, chunk_secs: f64) -> u64 {
    secs_to_ms(chunk_index as f64 * chunk_secs)
}

/// On-disk timed checkpoint payload. Segment times are chunk-relative.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimedCheckpoint {
    pub text: String,
    pub segments: Vec<TimedSegment>,
    /// Actual ffmpeg chunk start in milliseconds on the source track timeline.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chunk_start_ms: Option<u64>,
}

impl TimedCheckpoint {
    pub fn from_transcript(transcript: &TimedTranscript, chunk_start_ms: u64) -> Self {
        Self {
            text: transcript.text.clone(),
            segments: transcript.segments.clone(),
            chunk_start_ms: Some(chunk_start_ms),
        }
    }

    pub fn into_transcript(self) -> TimedTranscript {
        TimedTranscript {
            text: self.text,
            segments: self.segments,
        }
    }
}

/// Write a JSON timed checkpoint. Times in `segments` must be chunk-relative.
pub fn write_timed_checkpoint(
    path: &Path,
    transcript: &TimedTranscript,
    chunk_start_ms: u64,
) -> Result<(), String> {
    if !transcript.text.trim().is_empty() && transcript.segments.is_empty() {
        return Err(
            "Timed checkpoint requires factual segments for a nonempty transcript".to_string(),
        );
    }
    validate_segments(&transcript.segments)?;
    let payload = TimedCheckpoint::from_transcript(transcript, chunk_start_ms);
    let json = serde_json::to_string(&payload)
        .map_err(|error| format!("serialize timed checkpoint: {error}"))?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            format!(
                "Failed to create checkpoint dir {}: {error}",
                parent.display()
            )
        })?;
    }
    std::fs::write(path, json).map_err(|error| {
        format!(
            "Failed to write timed checkpoint {}: {error}",
            path.display()
        )
    })
}

/// Read a timed JSON checkpoint. Legacy plain-text files are treated as a miss.
///
/// Returns `Ok(None)` when the file is missing, is legacy text, or lacks usable
/// segments for a nonempty transcript (caller should re-transcribe).
pub fn read_timed_checkpoint(path: &Path) -> Result<Option<(u64, TimedTranscript)>, String> {
    let raw = match std::fs::read_to_string(path) {
        Ok(value) => value,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(format!(
                "Failed to read timed checkpoint {}: {error}",
                path.display()
            ));
        }
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    // Legacy plain-text checkpoints are not valid for timed export.
    if !trimmed.starts_with('{') {
        return Ok(None);
    }
    let parsed: TimedCheckpoint = match serde_json::from_str(trimmed) {
        Ok(value) => value,
        Err(_) => return Ok(None),
    };
    if !parsed.text.trim().is_empty() && parsed.segments.is_empty() {
        return Ok(None);
    }
    if validate_segments(&parsed.segments).is_err() {
        return Ok(None);
    }
    let chunk_start_ms = parsed.chunk_start_ms.unwrap_or(0);
    Ok(Some((chunk_start_ms, parsed.into_transcript())))
}

/// True when a nonempty `.txt` exists, and — when WebVTT is required — a sibling
/// `.vtt` exists, is nonempty, and contains at least one cue (or both reflect an
/// empty transcript consistently).
pub fn outputs_complete_for_resume(output_txt: &Path, export_webvtt: bool) -> Result<bool, String> {
    let txt = match std::fs::read_to_string(output_txt) {
        Ok(value) => value,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => {
            return Err(format!(
                "Failed to read transcript {}: {error}",
                output_txt.display()
            ));
        }
    };
    let txt_empty = txt.trim().is_empty();
    if !export_webvtt {
        return Ok(!txt_empty);
    }

    let output_vtt = output_txt.with_extension("vtt");
    let vtt = match std::fs::read_to_string(&output_vtt) {
        Ok(value) => value,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => {
            return Err(format!(
                "Failed to read WebVTT {}: {error}",
                output_vtt.display()
            ));
        }
    };
    if txt_empty {
        // Empty transcript pair: minimal valid WebVTT header is enough.
        return Ok(vtt.trim_start().starts_with("WEBVTT"));
    }
    Ok(webvtt_has_cue(&vtt))
}

fn webvtt_has_cue(contents: &str) -> bool {
    let mut saw_header = false;
    for line in contents.lines() {
        let trimmed = line.trim();
        if !saw_header {
            if trimmed == "WEBVTT" || trimmed.starts_with("WEBVTT ") {
                saw_header = true;
            }
            continue;
        }
        if trimmed.contains("-->") {
            return true;
        }
    }
    false
}

/// Require factual segments when WebVTT export is enabled for a nonempty body.
pub fn require_timed_segments_for_export(
    transcript: &TimedTranscript,
    export_webvtt: bool,
    mode_label: &str,
) -> Result<(), String> {
    if !export_webvtt {
        return Ok(());
    }
    if transcript.text.trim().is_empty() {
        return Ok(());
    }
    if transcript.segments.is_empty() {
        return Err(format!(
            "{mode_label} WebVTT export failed: no factual segment timestamps were produced. Disable \"Export WebVTT\" or use a timestamp-capable provider."
        ));
    }
    validate_segments(&transcript.segments)
        .map_err(|error| format!("{mode_label} WebVTT export failed: {error}"))
}

/// Explicit tolerance when comparing provider segment ends to measured WAV/chunk duration.
/// Providers that return milliseconds-as-seconds will exceed this by orders of magnitude.
pub const SEGMENT_END_DURATION_TOLERANCE_MS: u64 = 500;

/// Reject transcripts whose latest segment end exceeds `duration_ms + tolerance`.
pub fn validate_segments_against_media_duration(
    segments: &[TimedSegment],
    duration_ms: u64,
    tolerance_ms: u64,
) -> Result<(), String> {
    let limit = duration_ms.saturating_add(tolerance_ms);
    for (index, segment) in segments.iter().enumerate() {
        if segment.end_ms > limit {
            return Err(format!(
                "Timed segment {index} end_ms {} exceeds media duration {} ms (tolerance {} ms)",
                segment.end_ms, duration_ms, tolerance_ms
            ));
        }
    }
    Ok(())
}

/// Version of the on-disk chunk checkpoint contract. Bump when key inputs change.
pub const CHECKPOINT_SCHEMA_VERSION: u32 = 3;

/// Provider scope for local whisper.cpp (not an HTTP endpoint).
pub const LOCAL_WHISPER_PROVIDER_SCOPE: &str = "localWhisper";

/// Normalize an HTTP API base URL for checkpoint keys (trim + strip trailing `/`).
pub fn normalize_http_provider_scope(base_url: &str) -> String {
    base_url.trim().trim_end_matches('/').to_string()
}

/// Deterministic cache-set id: SHA-256 of WAV content digest plus transcription contract.
///
/// Includes schema version, chunk duration contract, and provider scope
/// (normalized `api_base_url` for HTTP, [`LOCAL_WHISPER_PROVIDER_SCOPE`] for local).
/// Does not include API keys or other secrets.
pub fn chunk_checkpoint_set_key(
    wav_content_sha256_hex: &str,
    mode: &str,
    provider_scope: &str,
    model: &str,
    language: Option<&str>,
    export_webvtt: bool,
    schema_version: u32,
    chunk_secs: f64,
) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(wav_content_sha256_hex.as_bytes());
    hasher.update(b"\0");
    hasher.update(mode.as_bytes());
    hasher.update(b"\0");
    hasher.update(provider_scope.as_bytes());
    hasher.update(b"\0");
    hasher.update(model.as_bytes());
    hasher.update(b"\0");
    hasher.update(language.unwrap_or("").as_bytes());
    hasher.update(b"\0");
    hasher.update(if export_webvtt { b"1" } else { b"0" });
    hasher.update(b"\0");
    hasher.update(schema_version.to_string().as_bytes());
    hasher.update(b"\0");
    // Fixed decimal form so f64 contract is stable across platforms.
    hasher.update(format!("{chunk_secs:.3}").as_bytes());
    hex::encode(hasher.finalize())
}

/// SHA-256 hex digest of file bytes (normalized WAV as produced by the pipeline).
/// Streams the file in fixed-size chunks — never loads the whole WAV into memory.
pub fn sha256_file_hex(path: &Path) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;

    let mut file = std::fs::File::open(path).map_err(|error| {
        format!(
            "Failed to open {} for checkpoint key: {error}",
            path.display()
        )
    })?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|error| {
            format!(
                "Failed to read {} for checkpoint key: {error}",
                path.display()
            )
        })?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex::encode(hasher.finalize()))
}

#[cfg(test)]
thread_local! {
    static FAIL_PAIR_PUBLISH_AFTER_VTT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Test-only hook: fail after VTT destination publish, before TXT publish.
#[cfg(test)]
pub fn test_set_fail_pair_publish_after_vtt(fail: bool) {
    FAIL_PAIR_PUBLISH_AFTER_VTT.with(|cell| cell.set(fail));
}

#[cfg(test)]
fn test_should_fail_pair_publish_after_vtt() -> bool {
    FAIL_PAIR_PUBLISH_AFTER_VTT.with(|cell| cell.get())
}

fn unique_temp_path(destination: &Path, label: &str) -> PathBuf {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let file_name = format!(
        ".{}.{label}.{}.{nonce}.tmp",
        destination
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("out"),
        std::process::id()
    );
    match destination.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.join(file_name),
        _ => PathBuf::from(file_name),
    }
}

fn replace_file(temp: &Path, destination: &Path) -> Result<(), String> {
    match std::fs::rename(temp, destination) {
        Ok(()) => Ok(()),
        Err(_) => {
            // Windows cannot rename over an existing file; remove then rename.
            let _ = std::fs::remove_file(destination);
            std::fs::rename(temp, destination).map_err(|error| {
                format!(
                    "Failed to publish {} from {}: {error}",
                    destination.display(),
                    temp.display()
                )
            })
        }
    }
}

fn remove_file_if_exists(path: &Path) -> Result<(), String> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("Failed to remove {}: {error}", path.display())),
    }
}

/// Write the plain transcript and, when requested, its WebVTT sibling.
///
/// Crash-safe pair publish:
/// - validate + format first
/// - write unique temps in the same directory
/// - when `export_webvtt`: publish VTT before TXT after clearing the TXT destination so
///   an intermediate crash cannot leave a complete mismatched txt+vtt pair; rollback
///   partial VTT on later failure
/// - when `export_webvtt` is false: remove any stale sibling `.vtt`, then replace TXT
pub fn write_transcript_pair(
    output_txt: &Path,
    transcript: &TimedTranscript,
    export_webvtt: bool,
) -> Result<Option<PathBuf>, String> {
    let text = transcript.text.as_str();
    let segments = transcript.segments.as_slice();

    let vtt_contents = if export_webvtt {
        if !text.trim().is_empty() && segments.is_empty() {
            return Err(
                "WebVTT export requires factual timed segments for a nonempty transcript"
                    .to_string(),
            );
        }
        Some(format_webvtt(segments)?)
    } else {
        None
    };

    if let Some(parent) = output_txt.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|error| {
                format!("Failed to create output dir {}: {error}", parent.display())
            })?;
        }
    }

    let output_vtt = output_txt.with_extension("vtt");

    let Some(vtt_contents) = vtt_contents else {
        let temp_txt = unique_temp_path(output_txt, "txt");
        let write_plain = (|| -> Result<(), String> {
            std::fs::write(&temp_txt, text).map_err(|error| {
                format!(
                    "Failed to write temp transcript {}: {error}",
                    temp_txt.display()
                )
            })?;
            // Remove stale VTT first so a crash cannot leave new txt + old vtt as a pair.
            remove_file_if_exists(&output_vtt)?;
            replace_file(&temp_txt, output_txt)?;
            Ok(())
        })();
        if write_plain.is_err() {
            let _ = std::fs::remove_file(&temp_txt);
        }
        write_plain?;
        return Ok(None);
    };

    let temp_vtt = unique_temp_path(&output_vtt, "vtt");
    let temp_txt = unique_temp_path(output_txt, "txt");
    let publish = (|| -> Result<(), String> {
        std::fs::write(&temp_vtt, &vtt_contents).map_err(|error| {
            format!(
                "Failed to write temp WebVTT {}: {error}",
                temp_vtt.display()
            )
        })?;
        std::fs::write(&temp_txt, text).map_err(|error| {
            format!(
                "Failed to write temp transcript {}: {error}",
                temp_txt.display()
            )
        })?;

        // Drop TXT first so resume never sees a complete mismatched pair.
        remove_file_if_exists(output_txt)?;
        replace_file(&temp_vtt, &output_vtt).map_err(|error| {
            let _ = remove_file_if_exists(&output_vtt);
            error
        })?;

        #[cfg(test)]
        if test_should_fail_pair_publish_after_vtt() {
            remove_file_if_exists(&output_vtt)?;
            return Err("injected pair publish failure after vtt".to_string());
        }

        replace_file(&temp_txt, output_txt).map_err(|error| {
            let _ = remove_file_if_exists(&output_vtt);
            error
        })?;
        Ok(())
    })();

    let _ = std::fs::remove_file(&temp_vtt);
    let _ = std::fs::remove_file(&temp_txt);
    publish?;
    Ok(Some(output_vtt))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn segment(start_ms: u64, end_ms: u64, text: &str) -> TimedSegment {
        TimedSegment {
            start_ms,
            end_ms,
            text: text.to_string(),
            speaker: None,
        }
    }

    fn word(start_ms: u64, end_ms: u64, text: &str) -> TimedWord {
        TimedWord {
            start_ms,
            end_ms,
            text: text.to_string(),
            speaker: None,
        }
    }

    fn test_path(name: &str) -> std::path::PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock must be after Unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "v2t-timed-transcript-{}-{nonce}-{name}",
            std::process::id()
        ))
    }

    #[test]
    fn plain_text_only_has_no_synthetic_segments() {
        assert_eq!(
            TimedTranscript::plain_text_only("Hello"),
            TimedTranscript {
                text: "Hello".to_string(),
                segments: Vec::new(),
            }
        );
    }

    #[test]
    fn timed_transcript_uses_camel_case_command_wire_shape() {
        let transcript = TimedTranscript {
            text: "hello".to_string(),
            segments: vec![segment(25, 1_250, "hello")],
        };
        let json = serde_json::to_value(&transcript).unwrap();
        assert_eq!(json["segments"][0]["startMs"], 25);
        assert_eq!(json["segments"][0]["endMs"], 1_250);
        assert!(json["segments"][0].get("start_ms").is_none());

        let restored: TimedTranscript = serde_json::from_value(json).unwrap();
        assert_eq!(restored, transcript);
    }

    #[test]
    fn validates_happy_path_and_boundaries() {
        let segments = vec![
            segment(0, 1, "smallest interval"),
            segment(3_599_999, 3_600_000, "hour boundary"),
        ];
        assert_eq!(validate_segments(&segments), Ok(()));
    }

    #[test]
    fn rejects_invalid_intervals_and_empty_payloads() {
        let zero_length = vec![segment(1_000, 1_000, "bad")];
        let reversed = vec![segment(2_000, 1_000, "bad")];
        let empty = vec![segment(0, 1, " \r\n ")];

        assert!(validate_segments(&zero_length)
            .unwrap_err()
            .contains("end_ms must be greater"));
        assert!(validate_segments(&reversed)
            .unwrap_err()
            .contains("end_ms must be greater"));
        assert!(validate_segments(&empty)
            .unwrap_err()
            .contains("text must not be empty"));
    }

    #[test]
    fn timestamp_round_trip_covers_hour_and_large_values() {
        for ms in [
            0, 1, 999, 1_000, 59_999, 60_000, 3_599_999, 3_600_000, 99_999_999,
        ] {
            let rendered = format_timestamp(ms);
            assert_eq!(parse_timestamp(&rendered), Ok(ms), "{rendered}");
        }
    }

    #[test]
    fn rejects_malformed_timestamps() {
        for invalid in [
            "00:00:00",
            "0:00:00.000",
            "00:60:00.000",
            "00:00:60.000",
            "00:00:00,000",
            "aa:00:00.000",
        ] {
            assert!(parse_timestamp(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn formats_valid_webvtt_with_milliseconds_in_given_order() {
        let segments = vec![
            segment(3_600_001, 3_602_345, "First"),
            segment(10, 20, "Second"),
        ];
        let output = format_webvtt(&segments).unwrap();

        assert_eq!(
            output,
            "WEBVTT\n\n01:00:00.001 --> 01:00:02.345\nFirst\n\n00:00:00.010 --> 00:00:00.020\nSecond\n"
        );
    }

    #[test]
    fn escapes_payload_without_flattening_lines_or_removing_speaker_tags() {
        let segments = vec![segment(
            0,
            1_000,
            "<v Alice>Hello & welcome</v>\n[SPEAKER]: 2 < 3 --> yes",
        )];
        let output = format_webvtt(&segments).unwrap();

        assert!(output.contains("<v Alice>Hello &amp; welcome</v>\n[SPEAKER]: 2 &lt; 3 --&gt; yes"));
    }

    #[test]
    fn formatter_rejects_invalid_segments() {
        let error = format_webvtt(&[segment(5, 5, "bad")]).unwrap_err();
        assert!(error.contains("end_ms must be greater"));
    }

    #[test]
    fn offsets_and_merges_chunks_in_stable_chronological_order() {
        let chunks = vec![
            (
                480_000,
                TimedTranscript {
                    text: "later".to_string(),
                    segments: vec![
                        segment(250, 500, "same start, later end"),
                        segment(0, 100, "chunk two"),
                    ],
                },
            ),
            (
                0,
                TimedTranscript {
                    text: "earlier".to_string(),
                    segments: vec![
                        segment(480_250, 480_400, "same start, earlier end"),
                        segment(0, 10, "chunk one"),
                    ],
                },
            ),
        ];

        let merged = merge_chunk_transcripts(chunks).unwrap();
        assert_eq!(merged.text, "earlier\n\nlater");
        assert_eq!(
            merged.segments,
            vec![
                segment(0, 10, "chunk one"),
                segment(480_000, 480_100, "chunk two"),
                segment(480_250, 480_400, "same start, earlier end"),
                segment(480_250, 480_500, "same start, later end"),
            ]
        );
    }

    #[test]
    fn offset_rejects_millisecond_overflow() {
        let error = offset_segments(&[segment(u64::MAX - 1, u64::MAX, "bad")], 1).unwrap_err();
        assert!(error.contains("overflow"));
    }

    #[test]
    fn writes_txt_only_when_export_is_disabled() {
        let txt = test_path("plain.txt");
        let vtt = txt.with_extension("vtt");
        let result =
            write_transcript_pair(&txt, &TimedTranscript::plain_text_only("plain"), false).unwrap();

        assert_eq!(result, None);
        assert_eq!(fs::read_to_string(&txt).unwrap(), "plain");
        assert!(!vtt.exists());
        fs::remove_file(txt).unwrap();
    }

    #[test]
    fn writes_nonempty_transcript_pair_with_same_stem() {
        let txt = test_path("timed.txt");
        let vtt = txt.with_extension("vtt");
        let transcript = TimedTranscript {
            text: "spoken text".to_string(),
            segments: vec![segment(25, 1_250, "spoken text")],
        };
        let result = write_transcript_pair(&txt, &transcript, true).unwrap();

        assert_eq!(result.as_deref(), Some(vtt.as_path()));
        assert_eq!(fs::read_to_string(&txt).unwrap(), "spoken text");
        assert_eq!(
            fs::read_to_string(&vtt).unwrap(),
            "WEBVTT\n\n00:00:00.025 --> 00:00:01.250\nspoken text\n"
        );
        fs::remove_file(txt).unwrap();
        fs::remove_file(vtt).unwrap();
    }

    #[test]
    fn writes_pair_and_supports_empty_transcript() {
        let txt = test_path("empty.txt");
        let vtt = txt.with_extension("vtt");
        let result =
            write_transcript_pair(&txt, &TimedTranscript::plain_text_only(""), true).unwrap();

        assert_eq!(result.as_deref(), Some(vtt.as_path()));
        assert_eq!(fs::read_to_string(&txt).unwrap(), "");
        assert_eq!(fs::read_to_string(&vtt).unwrap(), "WEBVTT\n\n");
        fs::remove_file(txt).unwrap();
        fs::remove_file(vtt).unwrap();
    }

    #[test]
    fn refuses_nonempty_vtt_export_without_factual_segments() {
        let txt = test_path("missing-segments.txt");
        let error =
            write_transcript_pair(&txt, &TimedTranscript::plain_text_only("nonempty"), true)
                .unwrap_err();

        assert!(error.contains("factual timed segments"));
        assert!(!txt.exists());
        assert!(!txt.with_extension("vtt").exists());
    }

    #[test]
    fn export_false_removes_stale_sibling_vtt() {
        let txt = test_path("stale-plain.txt");
        let vtt = txt.with_extension("vtt");
        fs::write(&txt, "old").unwrap();
        fs::write(&vtt, "WEBVTT\n\n00:00:00.000 --> 00:00:01.000\nold\n").unwrap();

        write_transcript_pair(&txt, &TimedTranscript::plain_text_only("fresh"), false).unwrap();

        assert_eq!(fs::read_to_string(&txt).unwrap(), "fresh");
        assert!(!vtt.exists());
        fs::remove_file(txt).unwrap();
    }

    #[test]
    fn replaces_stale_mismatched_pair_atomically() {
        let txt = test_path("stale-pair.txt");
        let vtt = txt.with_extension("vtt");
        fs::write(&txt, "old text").unwrap();
        fs::write(&vtt, "WEBVTT\n\n00:00:00.000 --> 00:00:01.000\nold cue\n").unwrap();

        let transcript = TimedTranscript {
            text: "new text".to_string(),
            segments: vec![segment(10, 20, "new text")],
        };
        write_transcript_pair(&txt, &transcript, true).unwrap();

        assert_eq!(fs::read_to_string(&txt).unwrap(), "new text");
        let vtt_body = fs::read_to_string(&vtt).unwrap();
        assert!(vtt_body.contains("new text"));
        assert!(!vtt_body.contains("old cue"));
        fs::remove_file(txt).unwrap();
        fs::remove_file(vtt).unwrap();
    }

    #[test]
    fn injected_second_publish_failure_does_not_leave_complete_mismatched_pair() {
        let txt = test_path("inject-fail.txt");
        let vtt = txt.with_extension("vtt");
        fs::write(&txt, "old text").unwrap();
        fs::write(&vtt, "WEBVTT\n\n00:00:00.000 --> 00:00:01.000\nold cue\n").unwrap();

        test_set_fail_pair_publish_after_vtt(true);
        let transcript = TimedTranscript {
            text: "new text".to_string(),
            segments: vec![segment(10, 20, "new text")],
        };
        let err = write_transcript_pair(&txt, &transcript, true).unwrap_err();
        test_set_fail_pair_publish_after_vtt(false);

        assert!(err.contains("injected"));
        // Must not look like a successful mismatched pair.
        assert!(!outputs_complete_for_resume(&txt, true).unwrap());
        let _ = fs::remove_file(&txt);
        let _ = fs::remove_file(&vtt);
    }

    #[test]
    fn escapes_tags_with_attributes_but_preserves_safe_voice() {
        let segments = vec![
            segment(0, 100, "<i onclick=alert(1)>evil</i>"),
            segment(100, 200, "<v Alice>hello</v>"),
        ];
        let output = format_webvtt(&segments).unwrap();
        // Attribute form must be escaped at the open tag; exact </i> stays.
        assert!(output.contains("&lt;i onclick=alert(1)&gt;evil</i>"));
        assert!(output.contains("<v Alice>hello</v>"));
        assert!(!output.contains("<i onclick"));
    }

    #[test]
    fn checkpoint_set_key_stable_and_changes_with_contract() {
        let a = chunk_checkpoint_set_key(
            "abc",
            "httpApi",
            "https://api.example/v1",
            "whisper-1",
            Some("en"),
            true,
            CHECKPOINT_SCHEMA_VERSION,
            480.0,
        );
        let b = chunk_checkpoint_set_key(
            "abc",
            "httpApi",
            "https://api.example/v1",
            "whisper-1",
            Some("en"),
            true,
            CHECKPOINT_SCHEMA_VERSION,
            480.0,
        );
        let c = chunk_checkpoint_set_key(
            "abc",
            "httpApi",
            "https://api.example/v1",
            "whisper-1",
            Some("en"),
            false,
            CHECKPOINT_SCHEMA_VERSION,
            480.0,
        );
        let d = chunk_checkpoint_set_key(
            "abd",
            "httpApi",
            "https://api.example/v1",
            "whisper-1",
            Some("en"),
            true,
            CHECKPOINT_SCHEMA_VERSION,
            480.0,
        );
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_ne!(a, d);
        assert!(!a.contains("whisper-1"));
        assert!(!a.contains("en"));
    }

    #[test]
    fn endpoint_changes_checkpoint_key() {
        let base = |scope: &str| {
            chunk_checkpoint_set_key(
                "abc",
                "httpApi",
                scope,
                "whisper-1",
                Some("en"),
                true,
                CHECKPOINT_SCHEMA_VERSION,
                480.0,
            )
        };
        assert_ne!(
            base("https://api.openai.com/v1"),
            base("https://api.other.com/v1")
        );
        assert_ne!(
            base("https://api.openai.com/v1"),
            base(LOCAL_WHISPER_PROVIDER_SCOPE)
        );
    }

    #[test]
    fn trailing_slash_normalization_same_checkpoint_key() {
        let a = normalize_http_provider_scope("https://api.example/v1/");
        let b = normalize_http_provider_scope("https://api.example/v1");
        let c = normalize_http_provider_scope("  https://api.example/v1///  ");
        assert_eq!(a, b);
        assert_eq!(a, c);
        let key_a = chunk_checkpoint_set_key(
            "abc",
            "httpApi",
            &a,
            "m",
            None,
            true,
            CHECKPOINT_SCHEMA_VERSION,
            480.0,
        );
        let key_b = chunk_checkpoint_set_key(
            "abc",
            "httpApi",
            &b,
            "m",
            None,
            true,
            CHECKPOINT_SCHEMA_VERSION,
            480.0,
        );
        assert_eq!(key_a, key_b);
    }

    #[test]
    fn schema_and_chunk_secs_contract_change_key() {
        let base = |schema: u32, chunk: f64| {
            chunk_checkpoint_set_key(
                "abc",
                "httpApi",
                "https://api.example/v1",
                "m",
                None,
                true,
                schema,
                chunk,
            )
        };
        assert_ne!(
            base(CHECKPOINT_SCHEMA_VERSION, 480.0),
            base(CHECKPOINT_SCHEMA_VERSION + 1, 480.0)
        );
        assert_ne!(
            base(CHECKPOINT_SCHEMA_VERSION, 480.0),
            base(CHECKPOINT_SCHEMA_VERSION, 240.0)
        );
    }

    #[test]
    fn streaming_sha256_matches_known_digest() {
        use sha2::{Digest, Sha256};
        let path = test_path("stream-hash.bin");
        // Larger than one 64 KiB buffer to exercise the streaming loop.
        let mut bytes = vec![0u8; 70_000];
        for (i, b) in bytes.iter_mut().enumerate() {
            *b = (i % 251) as u8;
        }
        fs::write(&path, &bytes).unwrap();
        let mut expected = Sha256::new();
        expected.update(&bytes);
        let expect_hex = hex::encode(expected.finalize());
        assert_eq!(sha256_file_hex(&path).unwrap(), expect_hex);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn validates_segment_end_against_media_duration() {
        let ok = vec![segment(0, 1_000, "a")];
        assert!(validate_segments_against_media_duration(
            &ok,
            1_000,
            SEGMENT_END_DURATION_TOLERANCE_MS
        )
        .is_ok());
        let bad = vec![segment(0, 50_000, "ms-as-secs")];
        let err = validate_segments_against_media_duration(&bad, 2_000, 500).unwrap_err();
        assert!(err.contains("exceeds media duration"));
    }

    #[test]
    fn compact_checkpoint_json_is_single_line_object() {
        let path = test_path("compact.json");
        let transcript = TimedTranscript {
            text: "hello".to_string(),
            segments: vec![segment(0, 250, "hello")],
        };
        write_timed_checkpoint(&path, &transcript, 100).unwrap();
        let raw = fs::read_to_string(&path).unwrap();
        assert!(raw.starts_with('{'));
        assert!(!raw.contains('\n'));
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn secs_to_ms_and_chunk_index_helpers_round_stably() {
        assert_eq!(secs_to_ms(480.0), 480_000);
        assert_eq!(chunk_start_ms_for_index(1, 480.0), 480_000);
        assert_eq!(secs_to_ms(0.001), 1);
        assert_eq!(secs_to_ms(-1.0), 0);
    }

    #[test]
    fn timed_checkpoint_round_trip_preserves_chunk_relative_segments() {
        let path = test_path("chunk.json");
        let transcript = TimedTranscript {
            text: "hello".to_string(),
            segments: vec![segment(0, 250, "hello")],
        };
        write_timed_checkpoint(&path, &transcript, 480_000).unwrap();
        let (offset, restored) = read_timed_checkpoint(&path).unwrap().unwrap();
        assert_eq!(offset, 480_000);
        assert_eq!(restored, transcript);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn legacy_text_checkpoint_is_a_miss_for_timed_resume() {
        let path = test_path("legacy.txt");
        fs::write(&path, "legacy plain checkpoint").unwrap();
        assert_eq!(read_timed_checkpoint(&path).unwrap(), None);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn json_checkpoint_without_segments_is_a_miss() {
        let path = test_path("no-segments.json");
        fs::write(
            &path,
            r#"{"text":"spoken","segments":[],"chunk_start_ms":0}"#,
        )
        .unwrap();
        assert_eq!(read_timed_checkpoint(&path).unwrap(), None);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn outputs_complete_for_resume_matrix() {
        let txt = test_path("resume.txt");
        let vtt = txt.with_extension("vtt");

        assert!(!outputs_complete_for_resume(&txt, false).unwrap());

        fs::write(&txt, "body").unwrap();
        assert!(outputs_complete_for_resume(&txt, false).unwrap());
        assert!(!outputs_complete_for_resume(&txt, true).unwrap());

        fs::write(&vtt, "WEBVTT\n\n00:00:00.000 --> 00:00:01.000\nbody\n").unwrap();
        assert!(outputs_complete_for_resume(&txt, true).unwrap());

        fs::write(&txt, "").unwrap();
        fs::write(&vtt, "WEBVTT\n\n").unwrap();
        assert!(outputs_complete_for_resume(&txt, true).unwrap());

        fs::write(&txt, "body").unwrap();
        fs::write(&vtt, "WEBVTT\n\n").unwrap();
        assert!(!outputs_complete_for_resume(&txt, true).unwrap());

        fs::remove_file(txt).unwrap();
        fs::remove_file(vtt).unwrap();
    }

    #[test]
    fn merge_uses_actual_chunk_start_not_only_index_times_480() {
        // Resume after a non-default start (e.g. first chunk was shorter / re-cut).
        let chunks = vec![
            (
                0,
                TimedTranscript {
                    text: "a".to_string(),
                    segments: vec![segment(0, 50, "a")],
                },
            ),
            (
                123_456,
                TimedTranscript {
                    text: "b".to_string(),
                    segments: vec![segment(10, 40, "b")],
                },
            ),
        ];
        let merged = merge_chunk_transcripts(chunks).unwrap();
        assert_eq!(merged.segments[1].start_ms, 123_466);
        assert_eq!(merged.segments[1].end_ms, 123_496);
    }

    #[test]
    fn build_cues_from_words_splits_on_gap_chars_duration_and_punct() {
        let words = vec![
            word(0, 200, "Hello"),
            word(250, 500, "world."),
            // gap >= 500 ms
            word(1_200, 1_400, "Next"),
            word(1_450, 1_600, "cue"),
            // char budget: long word alone, then another that would exceed 50
            word(2_200, 2_400, &"x".repeat(48)),
            word(2_450, 2_600, &"y".repeat(10)),
            // duration budget across a long span (gap separates from prior cue)
            word(3_500, 3_600, "start"),
            word(13_700, 13_800, "too-long"),
        ];
        let cues = build_cues_from_words(&words).unwrap();
        assert_eq!(cues.len(), 6);
        assert_eq!(cues[0].text, "Hello world.");
        assert_eq!(cues[0].end_ms, 500);
        assert_eq!(cues[1].text, "Next cue");
        assert_eq!(cues[1].start_ms, 1_200);
        assert_eq!(cues[2].text, "x".repeat(48));
        assert_eq!(cues[3].text, "y".repeat(10));
        assert_eq!(cues[4].text, "start");
        assert_eq!(cues[5].text, "too-long");
    }

    #[test]
    fn build_cues_propagates_shared_speaker_only() {
        let mut a = word(0, 100, "Hi");
        a.speaker = Some("Alice".into());
        let mut b = word(120, 200, "there.");
        b.speaker = Some("Alice".into());
        let mut c = word(900, 1_000, "Bye");
        c.speaker = Some("Bob".into());
        let cues = build_cues_from_words(&[a, b, c]).unwrap();
        assert_eq!(cues.len(), 2);
        assert_eq!(cues[0].speaker.as_deref(), Some("Alice"));
        assert_eq!(cues[0].text, "Hi there.");
        assert_eq!(cues[1].speaker.as_deref(), Some("Bob"));
    }

    #[test]
    fn build_cues_mixed_speakers_clear_speaker_field() {
        let mut a = word(0, 100, "Hi");
        a.speaker = Some("Alice".into());
        let mut b = word(120, 200, "Bob");
        b.speaker = Some("Bob".into());
        let cues = build_cues_from_words(&[a, b]).unwrap();
        assert_eq!(cues.len(), 1);
        assert_eq!(cues[0].speaker, None);
        assert_eq!(cues[0].text, "Hi Bob");
        assert!(!cues[0].text.contains("<v "));
    }

    #[test]
    fn format_webvtt_wraps_speaker_field_before_escape() {
        let segments = vec![TimedSegment {
            start_ms: 0,
            end_ms: 1_000,
            text: "Hello & welcome".into(),
            speaker: Some("Alice".into()),
        }];
        let output = format_webvtt(&segments).unwrap();
        assert!(output.contains("<v Alice>Hello &amp; welcome</v>"));
    }

    #[test]
    fn format_webvtt_skips_wrap_when_text_already_has_voice_tag() {
        let segments = vec![TimedSegment {
            start_ms: 0,
            end_ms: 1_000,
            text: "<v Alice>Hello</v>".into(),
            speaker: Some("Bob".into()),
        }];
        let output = format_webvtt(&segments).unwrap();
        assert!(output.contains("<v Alice>Hello</v>"));
        assert!(!output.contains("<v Bob>"));
    }

    #[test]
    fn extract_voice_speaker_closed_and_open_forms() {
        let (sp, body) = extract_voice_speaker("<v Alice>Hello</v> world");
        assert_eq!(sp.as_deref(), Some("Alice"));
        assert_eq!(body, "Hello world");
        let (sp2, body2) = extract_voice_speaker("<v Bob>open rest");
        assert_eq!(sp2.as_deref(), Some("Bob"));
        assert_eq!(body2, "open rest");
        let (sp3, body3) = extract_voice_speaker("plain");
        assert_eq!(sp3, None);
        assert_eq!(body3, "plain");
    }

    #[test]
    fn merge_overlapping_drops_early_and_near_duplicate_segments() {
        let chunks = vec![
            (
                0,
                TimedTranscript {
                    text: "a".into(),
                    segments: vec![
                        segment(0, 1_000, "kept early"),
                        segment(4_000, 4_500, "overlap edge"),
                    ],
                },
            ),
            (
                3_000,
                TimedTranscript {
                    text: "b".into(),
                    segments: vec![
                        // Absolute start 3_200 < 3_000 + 1_250 → dropped by cutoff
                        segment(200, 400, "too early"),
                        // Absolute start 4_050 near-dup of "overlap edge" at 4_000
                        segment(1_050, 1_400, "Overlap Edge"),
                        segment(2_000, 2_500, "kept later"),
                    ],
                },
            ),
        ];
        let merged = merge_overlapping_chunk_transcripts(chunks, 2_500).unwrap();
        let texts: Vec<_> = merged.segments.iter().map(|s| s.text.as_str()).collect();
        assert!(texts.contains(&"kept early"));
        assert!(texts.contains(&"overlap edge"));
        assert!(texts.contains(&"kept later"));
        assert!(!texts.iter().any(|t| t.eq_ignore_ascii_case("too early")));
        assert_eq!(
            texts
                .iter()
                .filter(|t| t.eq_ignore_ascii_case("overlap edge"))
                .count(),
            1
        );
    }

    #[test]
    fn merge_overlapping_zero_matches_plain_merge() {
        let chunks = vec![
            (
                0,
                TimedTranscript {
                    text: "a".into(),
                    segments: vec![segment(0, 50, "a")],
                },
            ),
            (
                100,
                TimedTranscript {
                    text: "b".into(),
                    segments: vec![segment(0, 50, "b")],
                },
            ),
        ];
        let plain = merge_chunk_transcripts(chunks.clone()).unwrap();
        let overlap0 = merge_overlapping_chunk_transcripts(chunks, 0).unwrap();
        assert_eq!(plain, overlap0);
    }
}
