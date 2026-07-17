use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use tokio::process::Command as TokioCommand;
use tokio_util::sync::CancellationToken;

use crate::pipeline::{self, JOB_CANCELLED_MSG};
use crate::process_kill;
use crate::progress::{JobEvent, QueueJobProgressEvent, SinkHandle};
use crate::timed_transcript::{
    build_cues_from_words, merge_overlapping_chunk_transcripts, read_timed_checkpoint,
    require_timed_segments_for_export, secs_to_ms, validate_segments_against_media_duration,
    write_timed_checkpoint, TimedSegment, TimedTranscript, TimedWord, CHUNK_OVERLAP_SECS,
};
use crate::transcribe::{
    pcm_payload_bytes, TimedTranscribeOutcome, CHUNK_SECS, FFMPEG_CHUNK_TIMEOUT, MAX_UPLOAD_BYTES,
    PCM_BYTES_PER_SEC,
};
use tauri::Manager;

const WHISPER_TIMEOUT: Duration = Duration::from_secs(7200);
const WEBVTT_MAX_SEGMENT_CHARS: &str = "50";

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

/// Heuristics: identify whisper.cpp stderr lines that indicate the GPU backend failed
/// to initialize (driver mismatch, CUDA missing, Vulkan loader gone) so we can fall back
/// to a previously-installed CPU build instead of failing the whole queue item.
pub(crate) fn looks_like_gpu_init_failure(stderr: &str) -> bool {
    let s = stderr.to_ascii_lowercase();
    const NEEDLES: &[&str] = &[
        "cudagetdevicecount",
        "cuda error",
        "cuda driver",
        "cublas",
        "no cuda-capable device",
        "failed to initialize vulkan",
        "vulkan: failed",
        "vk_error",
        "vulkan loader",
    ];
    NEEDLES.iter().any(|n| s.contains(n))
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
    sink: &SinkHandle,
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

    let sink_emit = Arc::clone(sink);
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
                            sink_emit.emit(JobEvent::QueueJobProgress(QueueJobProgressEvent {
                                job_id: jid.clone(),
                                phase: "whisper".to_string(),
                                message: msg,
                                subtask_index: None,
                                subtask_total: None,
                                subtask_percent: None,
                            }));
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

/// Map a ggml model path/filename to a whisper.cpp `-dtw` preset.
/// Unknown names return `None` (caller omits `-dtw`).
pub(crate) fn dtw_preset_from_model_path(model_path: &Path) -> Option<&'static str> {
    let stem = model_path.file_stem()?.to_str()?.to_ascii_lowercase();
    let mut name = stem.strip_prefix("ggml-").unwrap_or(&stem).to_string();
    for suffix in ["-q5_0", "-q5_1", "-q8_0", "-q4_0", "-q4_1", "-tdrz"] {
        if let Some(stripped) = name.strip_suffix(suffix) {
            name = stripped.to_string();
            break;
        }
    }
    let normalized = name.replace('-', ".");
    match normalized.as_str() {
        "tiny" => Some("tiny"),
        "tiny.en" => Some("tiny.en"),
        "base" => Some("base"),
        "base.en" => Some("base.en"),
        "small" => Some("small"),
        "small.en" => Some("small.en"),
        "medium" => Some("medium"),
        "medium.en" => Some("medium.en"),
        "large.v1" => Some("large.v1"),
        "large.v2" => Some("large.v2"),
        "large.v3" => Some("large.v3"),
        "large.v3.turbo" => Some("large.v3.turbo"),
        _ => None,
    }
}

/// Locate a Silero/VAD ggml model under `{app_data}/models`.
/// Prefers current upstream `ggml-silero-v6.2.0.bin`, then older pinned names,
/// else any `*silero*.bin` / `*vad*.bin`.
fn locate_vad_model(app: &tauri::AppHandle) -> Option<PathBuf> {
    let models = app.path().app_data_dir().ok()?.join("models");
    if !models.is_dir() {
        return None;
    }
    const PREFERRED: &[&str] = &[
        "ggml-silero-v6.2.0.bin",
        "ggml-silero-v5.1.2.bin",
        "silero-v6.2.0-ggml.bin",
    ];
    for name in PREFERRED {
        let candidate = models.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    let mut matches: Vec<PathBuf> = std::fs::read_dir(&models)
        .ok()?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file()
                && path
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("bin"))
                && path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| {
                        let lower = name.to_ascii_lowercase();
                        lower.contains("silero") || lower.contains("vad")
                    })
        })
        .collect();
    matches.sort();
    matches.into_iter().next()
}

/// Run whisper.cpp `whisper-cli` (or compatible `main`) on one WAV.
///
/// When `export_webvtt` is true, requests full JSON (`-ojf`) and parses factual
/// word/segment offsets. When false, keeps the legacy `-otxt` path.
///
/// When `label_speakers` is true (with WebVTT), adds `-tdrz` and forces `-l en`
/// for the English tinydiarize model.
#[allow(clippy::too_many_arguments)]
async fn transcribe_one_wav(
    wav_path: &Path,
    cli: &Path,
    model_path: &Path,
    language: Option<&str>,
    out_base: &Path,
    export_webvtt: bool,
    label_speakers: bool,
    cancel: &CancellationToken,
    sink: &SinkHandle,
    app: &tauri::AppHandle,
    job_id: &str,
) -> Result<TimedTranscript, String> {
    let use_tdrz = export_webvtt && label_speakers;
    let lang = if use_tdrz {
        "en".to_string()
    } else {
        match language {
            Some(l) if !l.trim().is_empty() => l.trim().to_string(),
            _ => "auto".to_string(),
        }
    };

    let mut args: Vec<String> = vec![
        "-m".into(),
        model_path.to_string_lossy().into_owned(),
        "-f".into(),
        wav_path.to_string_lossy().into_owned(),
        "-of".into(),
        out_base.to_string_lossy().into_owned(),
        "-nt".into(),
        "-l".into(),
        lang,
    ];
    if use_tdrz {
        args.push("-tdrz".into());
    }
    if export_webvtt {
        args.push("-ojf".into());
        args.push("-ml".into());
        args.push(WEBVTT_MAX_SEGMENT_CHARS.into());
        args.push("-sow".into());
        if let Some(preset) = dtw_preset_from_model_path(model_path) {
            args.push("-dtw".into());
            args.push(preset.into());
        }
        if let Some(vad_model) = locate_vad_model(app) {
            args.push("--vad".into());
            args.push("--vad-model".into());
            args.push(vad_model.to_string_lossy().into_owned());
            args.push("-vsd".into());
            args.push("300".into());
        }
    } else {
        args.push("-otxt".into());
    }

    let mut out =
        run_whisper_cli_with_progress(cli, &args, WHISPER_TIMEOUT, cancel, sink, job_id).await?;
    if !out.status.success() {
        let stderr_text = String::from_utf8_lossy(&out.stderr).to_string();
        if looks_like_gpu_init_failure(&stderr_text) {
            if let Some(cpu_cli) = crate::tool_download::locate_installed_cpu_whisper_cli(app) {
                if cpu_cli != cli {
                    let msg = format!(
                        "[whisper] GPU init failed (driver/SDK mismatch?), falling back to CPU build at {}",
                        cpu_cli.display()
                    );
                    sink.emit(JobEvent::QueueJobProgress(QueueJobProgressEvent {
                        job_id: job_id.to_string(),
                        phase: "whisper".to_string(),
                        message: msg,
                        subtask_index: None,
                        subtask_total: None,
                        subtask_percent: None,
                    }));
                    out = run_whisper_cli_with_progress(
                        &cpu_cli,
                        &args,
                        WHISPER_TIMEOUT,
                        cancel,
                        sink,
                        job_id,
                    )
                    .await?;
                }
            }
        }
        if !out.status.success() {
            return Err(format!(
                "whisper-cli failed (exit {}): {}",
                out.status.code().unwrap_or(-1),
                pipeline::tail_stderr(&out.stderr)
            ));
        }
    }

    if export_webvtt {
        let json_path = out_base.with_extension("json");
        let json = match std::fs::read_to_string(&json_path) {
            Ok(body) => body,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(format!(
                    "Local Whisper WebVTT export failed: whisper-cli did not write expected {} (check whisper-cli version / -oj support)",
                    json_path.display()
                ));
            }
            Err(e) => return Err(format!("read whisper json: {e}")),
        };
        let transcript = parse_whisper_json(&json, label_speakers)?;
        require_timed_segments_for_export(&transcript, true, "Local Whisper")?;
        let _ = std::fs::remove_file(&json_path);
        Ok(transcript)
    } else {
        let read_path = out_base.with_extension("txt");
        let text = match std::fs::read_to_string(&read_path) {
            Ok(body) => body,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(format!(
                    "whisper-cli did not write expected {} (check whisper-cli version / flags)",
                    read_path.display()
                ));
            }
            Err(e) => return Err(format!("read whisper txt: {e}")),
        };
        let transcript = TimedTranscript::plain_text_only(text.trim().to_string());
        let _ = std::fs::remove_file(&read_path);
        Ok(transcript)
    }
}

#[derive(Debug, Deserialize)]
struct WhisperJsonOffsets {
    from: i64,
    to: i64,
}

#[derive(Debug, Deserialize)]
struct WhisperJsonTimestamps {
    #[serde(default)]
    from: Option<String>,
    #[serde(default)]
    to: Option<String>,
}

#[derive(Debug, Deserialize)]
struct WhisperJsonToken {
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    offsets: Option<WhisperJsonOffsets>,
    #[serde(default)]
    t_dtw: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct WhisperJsonSegment {
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    offsets: Option<WhisperJsonOffsets>,
    #[serde(default)]
    timestamps: Option<WhisperJsonTimestamps>,
    #[serde(default)]
    start: Option<f64>,
    #[serde(default)]
    end: Option<f64>,
    #[serde(default)]
    tokens: Option<Vec<WhisperJsonToken>>,
    /// tinydiarize: when true, the next segment belongs to the other speaker.
    #[serde(default)]
    speaker_turn_next: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct WhisperJsonRoot {
    #[serde(default)]
    transcription: Option<Vec<WhisperJsonSegment>>,
    #[serde(default)]
    segments: Option<Vec<WhisperJsonSegment>>,
    #[serde(default)]
    text: Option<String>,
}

/// Parse whisper.cpp `-oj` / `-ojf` JSON into a timed transcript.
///
/// Prefers word cues rebuilt from full-JSON tokens when present; otherwise:
/// - `{ "transcription": [ { "offsets": {"from","to"}, "text" } ] }`
/// - `{ "segments": [ { "start","end","text" } ] }` (seconds)
/// - timestamp strings `00:00:00,000` / `00:00:00.000` as a fallback when offsets are absent
///
/// When `label_speakers` is true, uses segment-level fallback (so tinydiarize
/// `speaker_turn_next` turns are preserved) and assigns Person 1 / Person 2.
pub fn parse_whisper_json(body: &str, label_speakers: bool) -> Result<TimedTranscript, String> {
    let root: WhisperJsonRoot = serde_json::from_str(body)
        .map_err(|e| format!("Local Whisper WebVTT export failed: invalid JSON ({e})"))?;

    let raw_segments: &[WhisperJsonSegment] = root
        .transcription
        .as_deref()
        .or(root.segments.as_deref())
        .unwrap_or(&[]);

    let segments = if label_speakers {
        // Prefer segment fallback so speaker turns are not lost in word rebuild.
        parse_whisper_segments_fallback(raw_segments, true)?
    } else {
        let words = words_from_whisper_segments(raw_segments);
        if !words.is_empty() {
            build_cues_from_words(&words).map_err(|e| {
                format!("Local Whisper WebVTT export failed: word cue rebuild ({e})")
            })?
        } else {
            parse_whisper_segments_fallback(raw_segments, false)?
        }
    };

    let text = root
        .text
        .as_deref()
        .map(|value| {
            if label_speakers {
                strip_speaker_turn_markers(value)
            } else {
                value.trim().to_string()
            }
        })
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| {
            segments
                .iter()
                .map(|segment| segment.text.as_str())
                .collect::<Vec<_>>()
                .join(" ")
        });

    Ok(TimedTranscript { text, segments })
}

fn strip_speaker_turn_markers(text: &str) -> String {
    let mut cleaned = text
        .replace("[SPEAKER_TURN]", "")
        .replace("(SPEAKER_TURN)", "");
    while cleaned.contains("  ") {
        cleaned = cleaned.replace("  ", " ");
    }
    cleaned.trim().to_string()
}

fn words_from_whisper_segments(raw_segments: &[WhisperJsonSegment]) -> Vec<TimedWord> {
    let mut token_timings: Vec<(String, u64, u64)> = Vec::new();
    for segment in raw_segments {
        let Some(tokens) = segment.tokens.as_ref() else {
            continue;
        };
        for token in tokens {
            let raw_text = token.text.as_deref().unwrap_or("");
            if raw_text.is_empty() || is_whisper_special_token(raw_text) {
                continue;
            }
            let Some((start_ms, end_ms)) = token_bounds_ms(token) else {
                continue;
            };
            if end_ms <= start_ms {
                continue;
            }
            token_timings.push((raw_text.to_string(), start_ms, end_ms));
        }
    }
    merge_whisper_tokens_into_words(&token_timings)
}

fn is_whisper_special_token(text: &str) -> bool {
    let trimmed = text.trim();
    (trimmed.starts_with('[') && trimmed.ends_with(']'))
        || (trimmed.starts_with("<|") && trimmed.ends_with("|>"))
}

fn token_bounds_ms(token: &WhisperJsonToken) -> Option<(u64, u64)> {
    if let Some(offsets) = &token.offsets {
        if offsets.to > offsets.from {
            return Some((offsets.from.max(0) as u64, offsets.to.max(0) as u64));
        }
    }
    // t_dtw is in centiseconds; usable only when offsets are missing.
    let t_dtw = token
        .t_dtw
        .filter(|value| value.is_finite() && *value >= 0.0)?;
    let start_ms = (t_dtw * 10.0).round() as u64;
    Some((start_ms, start_ms.saturating_add(1)))
}

/// Tokens that begin with a space start a new word; otherwise append to the current word.
fn merge_whisper_tokens_into_words(tokens: &[(String, u64, u64)]) -> Vec<TimedWord> {
    let mut words: Vec<TimedWord> = Vec::new();
    for (raw_text, start_ms, end_ms) in tokens {
        let starts_new = raw_text.starts_with(' ') || words.is_empty();
        let piece = raw_text.trim();
        if piece.is_empty() {
            continue;
        }
        if starts_new {
            words.push(TimedWord {
                start_ms: *start_ms,
                end_ms: *end_ms,
                text: piece.to_string(),
                speaker: None,
            });
        } else if let Some(current) = words.last_mut() {
            current.text.push_str(piece);
            current.end_ms = (*end_ms).max(current.end_ms);
        }
    }
    words
}

fn parse_whisper_segments_fallback(
    raw_segments: &[WhisperJsonSegment],
    label_speakers: bool,
) -> Result<Vec<TimedSegment>, String> {
    let mut segments = Vec::with_capacity(raw_segments.len());
    let mut person: u8 = 1;
    for (index, segment) in raw_segments.iter().enumerate() {
        let raw_text = segment.text.as_deref().unwrap_or("");
        let text = if label_speakers {
            strip_speaker_turn_markers(raw_text)
        } else {
            raw_text.trim().to_string()
        };
        if text.is_empty() {
            if label_speakers && segment.speaker_turn_next == Some(true) {
                person = if person == 1 { 2 } else { 1 };
            }
            continue;
        }

        let (start_ms, end_ms) = if let Some(offsets) = &segment.offsets {
            if offsets.to <= offsets.from {
                return Err(format!(
                    "Local Whisper WebVTT export failed: segment {index} has end <= start"
                ));
            }
            (offsets.from.max(0) as u64, offsets.to.max(0) as u64)
        } else if let (Some(start), Some(end)) = (segment.start, segment.end) {
            let start_ms = secs_to_ms(start);
            let end_ms = secs_to_ms(end);
            if end_ms <= start_ms {
                return Err(format!(
                    "Local Whisper WebVTT export failed: segment {index} has end <= start"
                ));
            }
            (start_ms, end_ms)
        } else if let Some(timestamps) = &segment.timestamps {
            let from = timestamps.from.as_deref().ok_or_else(|| {
                format!(
                    "Local Whisper WebVTT export failed: segment {index} missing timestamp from"
                )
            })?;
            let to = timestamps.to.as_deref().ok_or_else(|| {
                format!("Local Whisper WebVTT export failed: segment {index} missing timestamp to")
            })?;
            let start_ms = parse_whisper_clock_to_ms(from)?;
            let end_ms = parse_whisper_clock_to_ms(to)?;
            if end_ms <= start_ms {
                return Err(format!(
                    "Local Whisper WebVTT export failed: segment {index} has end <= start"
                ));
            }
            (start_ms, end_ms)
        } else {
            return Err(format!(
                "Local Whisper WebVTT export failed: segment {index} has no offsets/timestamps"
            ));
        };

        let speaker = if label_speakers {
            Some(format!("Person {person}"))
        } else {
            None
        };

        segments.push(TimedSegment {
            start_ms,
            end_ms,
            text,
            speaker,
        });

        if label_speakers && segment.speaker_turn_next == Some(true) {
            person = if person == 1 { 2 } else { 1 };
        }
    }
    Ok(segments)
}

fn constrain_whisper_transcript_to_duration(
    mut transcript: TimedTranscript,
    duration_ms: u64,
) -> Result<TimedTranscript, String> {
    let original_segment_count = transcript.segments.len();
    let mut segments = Vec::with_capacity(original_segment_count);

    for mut segment in transcript.segments {
        if segment.start_ms >= duration_ms {
            continue;
        }
        segment.end_ms = segment.end_ms.min(duration_ms);
        if segment.end_ms > segment.start_ms {
            segments.push(segment);
        }
    }

    if segments.len() != original_segment_count {
        transcript.text = segments
            .iter()
            .map(|segment| segment.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
    }
    transcript.segments = segments;

    require_timed_segments_for_export(&transcript, true, "Local Whisper")?;
    validate_segments_against_media_duration(&transcript.segments, duration_ms, 0)?;
    Ok(transcript)
}

fn parse_whisper_clock_to_ms(value: &str) -> Result<u64, String> {
    let normalized = value.trim().replace(',', ".");
    // Accept HH:MM:SS.mmm or MM:SS.mmm
    let parts: Vec<&str> = normalized.split(':').collect();
    let (hours, minutes, seconds_part) = match parts.as_slice() {
        [h, m, s] => (
            h.parse::<u64>()
                .map_err(|_| format!("Invalid whisper timestamp: {value}"))?,
            m.parse::<u64>()
                .map_err(|_| format!("Invalid whisper timestamp: {value}"))?,
            *s,
        ),
        [m, s] => (
            0,
            m.parse::<u64>()
                .map_err(|_| format!("Invalid whisper timestamp: {value}"))?,
            *s,
        ),
        _ => return Err(format!("Invalid whisper timestamp: {value}")),
    };
    let (secs, millis) = if let Some((s, ms)) = seconds_part.split_once('.') {
        let millis = match ms.len() {
            0 => 0,
            1 => ms.parse::<u64>().unwrap_or(0) * 100,
            2 => ms.parse::<u64>().unwrap_or(0) * 10,
            _ => ms[..3]
                .parse::<u64>()
                .map_err(|_| format!("Invalid whisper timestamp: {value}"))?,
        };
        (
            s.parse::<u64>()
                .map_err(|_| format!("Invalid whisper timestamp: {value}"))?,
            millis,
        )
    } else {
        (
            seconds_part
                .parse::<u64>()
                .map_err(|_| format!("Invalid whisper timestamp: {value}"))?,
            0,
        )
    };
    Ok(hours * 3_600_000 + minutes * 60_000 + secs * 1_000 + millis)
}

/// Same chunking strategy as HTTP path when PCM payload exceeds `MAX_UPLOAD_BYTES`.
///
/// `work_dir` holds temporary chunk WAV / whisper outputs. `checkpoint_dir` is a
/// stable app-data cache for resume across unique work dirs.
#[allow(clippy::too_many_arguments)]
pub async fn transcribe_wav_maybe_split_whisper(
    wav_path: &Path,
    cli: &Path,
    model_path: &Path,
    ffmpeg: &Path,
    work_dir: &Path,
    checkpoint_dir: &Path,
    language: Option<&str>,
    export_webvtt: bool,
    label_speakers: bool,
    cancel: &CancellationToken,
    sink: &SinkHandle,
    app: &tauri::AppHandle,
    job_id: &str,
) -> Result<TimedTranscribeOutcome, String> {
    let token = safe_job_token(job_id);
    let payload = pcm_payload_bytes(wav_path)?;
    let duration_sec = (payload as f64 / PCM_BYTES_PER_SEC as f64).max(1.0);
    if payload <= MAX_UPLOAD_BYTES {
        let out_base = work_dir.join(format!("v2t-whisper-{token}-0"));
        let transcript = transcribe_one_wav(
            wav_path,
            cli,
            model_path,
            language,
            &out_base,
            export_webvtt,
            label_speakers,
            cancel,
            sink,
            app,
            job_id,
        )
        .await?;
        let transcript = if export_webvtt {
            constrain_whisper_transcript_to_duration(transcript, secs_to_ms(duration_sec))?
        } else {
            transcript
        };
        return Ok(TimedTranscribeOutcome {
            transcript,
            chunk_checkpoint_dir: None,
        });
    }

    std::fs::create_dir_all(checkpoint_dir).map_err(|e| format!("create checkpoint dir: {e}"))?;
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
            return Err("Whisper chunk split safety limit exceeded".to_string());
        }

        let chunk_start_ms = secs_to_ms(start);
        let chunk_duration_ms = secs_to_ms((duration_sec - start).min(CHUNK_SECS));
        let checkpoint = whisper_chunk_checkpoint_path(checkpoint_dir, i, export_webvtt);

        if export_webvtt {
            if let Some((stored_start, saved)) = read_timed_checkpoint(&checkpoint)? {
                let offset = if stored_start > 0 {
                    stored_start
                } else {
                    chunk_start_ms
                };
                let saved = constrain_whisper_transcript_to_duration(saved, chunk_duration_ms)?;
                chunks.push((offset, saved));
                start += chunk_step;
                i += 1;
                continue;
            }
        } else if let Some(saved) = read_plain_whisper_checkpoint(&checkpoint)? {
            chunks.push((chunk_start_ms, TimedTranscript::plain_text_only(saved)));
            start += chunk_step;
            i += 1;
            continue;
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
            &out_base,
            export_webvtt,
            label_speakers,
            cancel,
            sink,
            app,
            job_id,
        )
        .await?;
        let piece = if export_webvtt {
            constrain_whisper_transcript_to_duration(piece, chunk_duration_ms)?
        } else {
            piece
        };

        if export_webvtt {
            write_timed_checkpoint(&checkpoint, &piece, chunk_start_ms)?;
        } else {
            std::fs::write(&checkpoint, piece.text.as_bytes())
                .map_err(|e| format!("write checkpoint: {e}"))?;
        }
        chunks.push((chunk_start_ms, piece));
        let _ = std::fs::remove_file(&chunk_path);
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

fn whisper_chunk_checkpoint_path(checkpoint_dir: &Path, i: u32, timed: bool) -> PathBuf {
    let ext = if timed { "json" } else { "txt" };
    checkpoint_dir.join(format!("chunk-{i}.{ext}"))
}

fn read_plain_whisper_checkpoint(path: &Path) -> Result<Option<String>, String> {
    match std::fs::read_to_string(path) {
        Ok(saved) if !saved.trim().is_empty() => Ok(Some(saved)),
        Ok(_) => Ok(None),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("read checkpoint: {e}")),
    }
}

/// Remove whisper chunk checkpoints from a stable checkpoint directory.
pub fn cleanup_whisper_chunk_checkpoints(checkpoint_dir: &Path) {
    let Ok(rd) = std::fs::read_dir(checkpoint_dir) else {
        return;
    };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with("chunk-") && (name.ends_with(".txt") || name.ends_with(".json")) {
            let _ = std::fs::remove_file(e.path());
        }
    }
    let _ = std::fs::remove_dir(checkpoint_dir);
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

    #[test]
    fn detects_cuda_init_failure() {
        assert!(looks_like_gpu_init_failure(
            "ggml_cuda_init: cudaGetDeviceCount returned 35"
        ));
        assert!(looks_like_gpu_init_failure(
            "CUDA error: no CUDA-capable device is detected"
        ));
        assert!(looks_like_gpu_init_failure("cuBLAS error 7"));
    }

    #[test]
    fn detects_vulkan_init_failure() {
        assert!(looks_like_gpu_init_failure(
            "ggml_vulkan: failed to initialize Vulkan"
        ));
        assert!(looks_like_gpu_init_failure("vulkan loader missing"));
    }

    #[test]
    fn ignores_unrelated_errors() {
        assert!(!looks_like_gpu_init_failure("no model file"));
        assert!(!looks_like_gpu_init_failure(
            "error: input audio file not found"
        ));
    }

    #[test]
    fn parses_whisper_cpp_transcription_offsets_json() {
        let json = r#"{
            "transcription": [
                {
                    "timestamps": {"from": "00:00:00,000", "to": "00:00:02,000"},
                    "offsets": {"from": 0, "to": 2000},
                    "text": " Hello everyone."
                },
                {
                    "timestamps": {"from": "00:00:02,000", "to": "00:00:08,500"},
                    "offsets": {"from": 2000, "to": 8500},
                    "text": " <v Alice>Here we are</v>"
                }
            ]
        }"#;
        let timed = parse_whisper_json(json, false).unwrap();
        assert_eq!(timed.segments.len(), 2);
        assert_eq!(timed.segments[0].start_ms, 0);
        assert_eq!(timed.segments[0].end_ms, 2_000);
        assert_eq!(timed.segments[0].text, "Hello everyone.");
        assert_eq!(timed.segments[1].start_ms, 2_000);
        assert_eq!(timed.segments[1].end_ms, 8_500);
        assert!(timed.segments[1].text.contains("<v Alice>"));
        assert!(timed.text.contains("Hello everyone."));
    }

    #[test]
    fn parses_whisper_json_with_seconds_fields() {
        let json = r#"{
            "text": "Hi there",
            "segments": [
                {"start": 0.0, "end": 1.25, "text": "Hi"},
                {"start": 1.25, "end": 2.0, "text": "there"}
            ]
        }"#;
        let timed = parse_whisper_json(json, false).unwrap();
        assert_eq!(timed.text, "Hi there");
        assert_eq!(timed.segments[0].end_ms, 1_250);
        assert_eq!(timed.segments[1].start_ms, 1_250);
    }

    #[test]
    fn whisper_json_without_segments_yields_empty_segments() {
        let timed = parse_whisper_json(r#"{"text":"only text"}"#, false).unwrap();
        assert_eq!(timed.text, "only text");
        assert!(timed.segments.is_empty());
    }

    #[test]
    fn constrains_whisper_padding_to_actual_media_duration() {
        let transcript = TimedTranscript {
            text: "kept clipped dropped".to_string(),
            segments: vec![
                TimedSegment {
                    start_ms: 0,
                    end_ms: 1_000,
                    text: "kept".to_string(),
                    speaker: None,
                },
                TimedSegment {
                    start_ms: 1_000,
                    end_ms: 30_000,
                    text: "clipped".to_string(),
                    speaker: None,
                },
                TimedSegment {
                    start_ms: 30_000,
                    end_ms: 60_000,
                    text: "dropped".to_string(),
                    speaker: None,
                },
            ],
        };

        let constrained = constrain_whisper_transcript_to_duration(transcript, 1_500).unwrap();
        assert_eq!(constrained.text, "kept clipped");
        assert_eq!(constrained.segments.len(), 2);
        assert_eq!(constrained.segments[1].end_ms, 1_500);
    }

    #[test]
    fn parse_whisper_clock_accepts_comma_millis() {
        assert_eq!(parse_whisper_clock_to_ms("00:00:01,250").unwrap(), 1_250);
        assert_eq!(
            parse_whisper_clock_to_ms("01:02:03.004").unwrap(),
            3_723_004
        );
    }

    #[test]
    fn dtw_preset_maps_common_ggml_filenames() {
        assert_eq!(
            dtw_preset_from_model_path(Path::new("ggml-medium.bin")),
            Some("medium")
        );
        assert_eq!(
            dtw_preset_from_model_path(Path::new("ggml-large-v3-turbo.bin")),
            Some("large.v3.turbo")
        );
        assert_eq!(
            dtw_preset_from_model_path(Path::new("ggml-tiny.en.bin")),
            Some("tiny.en")
        );
        assert_eq!(
            dtw_preset_from_model_path(Path::new("ggml-large-v3-turbo-q5_0.bin")),
            Some("large.v3.turbo")
        );
        assert_eq!(dtw_preset_from_model_path(Path::new("custom.bin")), None);
    }

    #[test]
    fn parses_ojf_tokens_into_word_cues() {
        let json = r#"{
            "transcription": [
                {
                    "offsets": {"from": 0, "to": 2000},
                    "text": " Hello world.",
                    "tokens": [
                        {"text": " Hello", "offsets": {"from": 0, "to": 800}},
                        {"text": " world", "offsets": {"from": 800, "to": 1600}},
                        {"text": ".", "offsets": {"from": 1600, "to": 2000}}
                    ]
                }
            ]
        }"#;
        let timed = parse_whisper_json(json, false).unwrap();
        assert_eq!(timed.segments.len(), 1);
        assert_eq!(timed.segments[0].text, "Hello world.");
        assert_eq!(timed.segments[0].start_ms, 0);
        assert_eq!(timed.segments[0].end_ms, 2_000);
    }

    #[test]
    fn parses_tinydiarize_speaker_turn_next_into_person_labels() {
        let json = r#"{
            "text": "Hello [SPEAKER_TURN] How are you? Fine.",
            "transcription": [
                {
                    "offsets": {"from": 0, "to": 1500},
                    "text": " Hello [SPEAKER_TURN]",
                    "speaker_turn_next": true,
                    "tokens": [
                        {"text": " Hello", "offsets": {"from": 0, "to": 1000}},
                        {"text": " [SPEAKER_TURN]", "offsets": {"from": 1000, "to": 1500}}
                    ]
                },
                {
                    "offsets": {"from": 1500, "to": 3000},
                    "text": " How are you?",
                    "speaker_turn_next": true
                },
                {
                    "offsets": {"from": 3000, "to": 4000},
                    "text": " Fine (SPEAKER_TURN)",
                    "speaker_turn_next": false
                }
            ]
        }"#;
        let timed = parse_whisper_json(json, true).unwrap();
        assert_eq!(timed.segments.len(), 3);
        assert_eq!(timed.segments[0].speaker.as_deref(), Some("Person 1"));
        assert_eq!(timed.segments[0].text, "Hello");
        assert_eq!(timed.segments[1].speaker.as_deref(), Some("Person 2"));
        assert_eq!(timed.segments[1].text, "How are you?");
        assert_eq!(timed.segments[2].speaker.as_deref(), Some("Person 1"));
        assert_eq!(timed.segments[2].text, "Fine");
        assert!(!timed.text.contains("SPEAKER_TURN"));
    }
}
