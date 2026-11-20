//! Brief pipeline: orchestrates anchor detection → frame extraction → doc generation → PDF.
//!
//! Called from `job.rs` after a successful transcription when `settings.generate_brief` is true.
//! This is a soft-fail stage: errors are reported but do not invalidate the transcript.

use std::path::{Path, PathBuf};

use tokio_util::sync::CancellationToken;

use crate::brief_ado;
use crate::brief_frames;
use crate::brief_gate;
use crate::brief_md;
use crate::brief_pdf;
use crate::brief_provider::{BriefAnchor, BriefFrame, BriefProvider, GeminiProvider};
use crate::deps;
use crate::gemini_key_store;
use crate::pipeline;
use crate::progress::{QueueJobProgressEvent, SinkHandle};
use crate::settings::{AppSettings, BriefDocMode};

/// Maximum transcript length (chars) sent to the LLM in a single call.
/// Approx 50k chars ≈ 1.5h of dense speech. Above this we truncate with a note.
const MAX_TRANSCRIPT_CHARS: usize = 50_000;

/// Minimum anchors to keep after scene selection / LLM snap (Brief path).
const MIN_ANCHORS: usize = 5;
/// FDD targets video-to-spec density (~10–20 product screenshots).
const FDD_MIN_ANCHORS: usize = 10;
const FDD_MAX_ANCHORS: usize = 20;
/// Grab the frame a couple of seconds after the spoken cue (screen has settled).
const FDD_ANCHOR_OFFSET_SECS: f64 = 2.0;
/// Prefer a nearby scene-cut when one exists; otherwise keep the LLM timestamp.
const FDD_SOFT_SNAP_SECS: f64 = 3.0;
/// Minimum spacing between FDD screenshots.
const FDD_MIN_GAP_SECS: f64 = 8.0;

/// Run the full Brief pipeline for one job.
///
/// Returns the path to the generated `.fdd.pdf` / `.brief.pdf` on success.
/// On error, returns `Err(message)` — the caller decides whether to fail the job
/// based on `settings.brief_fail_policy`.
pub async fn run_brief_pipeline(
    sink: &SinkHandle,
    job_id: &str,
    transcript_path: &Path,
    vtt_path: Option<&Path>,
    video_path: Option<&Path>,
    settings: &AppSettings,
    cancel: &CancellationToken,
) -> Result<PathBuf, String> {
    if cancel.is_cancelled() {
        return Err(pipeline::JOB_CANCELLED_MSG.to_string());
    }

    // Resolve Gemini API key (same keyring as Vision OCR, D10)
    let api_key = gemini_key_store::get()
        .map_err(|e| format!("Could not read Gemini API key: {e}"))?
        .or_else(|| {
            if settings.gemini_api_key.is_empty() {
                None
            } else {
                Some(settings.gemini_api_key.clone())
            }
        })
        .ok_or("Design document requires a Gemini API key — add it in Settings → Brief / FDD.")?;

    let mode_label = settings.brief_doc_mode.label();
    emit_brief_progress(
        sink,
        job_id,
        "brief-start",
        &format!("Starting {mode_label} generation…"),
    );

    // Build provider — model selected by quality profile (accurate uses full Flash for FDD)
    let model = settings.effective_brief_model();
    let provider: Box<dyn BriefProvider> =
        Box::new(GeminiProvider::new(api_key, model, settings.brief_doc_mode));

    run_pipeline_with_provider(sink, job_id, transcript_path, vtt_path, video_path, settings, cancel, provider).await
}

/// Same as run_brief_pipeline but accepts a provider parameter (testable).
pub async fn run_pipeline_with_provider(
    sink: &SinkHandle,
    job_id: &str,
    transcript_path: &Path,
    vtt_path: Option<&Path>,
    video_path: Option<&Path>,
    settings: &AppSettings,
    cancel: &CancellationToken,
    provider: Box<dyn BriefProvider>,
) -> Result<PathBuf, String> {
    // ── Read transcript ───────────────────────────────────────────────────────
    let transcript_text = std::fs::read_to_string(transcript_path)
        .map_err(|e| format!("Could not read transcript: {e}"))?;

    // Prefer VTT for richer timestamps; fall back to plain TXT.
    let vtt_text = vtt_path
        .filter(|p| p.is_file())
        .and_then(|p| std::fs::read_to_string(p).ok());
    let transcript_for_llm = vtt_text.as_deref().unwrap_or(&transcript_text);

    // Truncate very long transcripts with a note
    let (transcript_llm, was_truncated) = truncate_transcript(transcript_for_llm, MAX_TRANSCRIPT_CHARS);
    if was_truncated {
        emit_brief_progress(
            sink, job_id, "brief-trunc",
            &format!(
                "Transcript truncated to {MAX_TRANSCRIPT_CHARS} chars for Brief. The document covers the first portion."
            ),
        );
    }

    // Check for ASR hallucination gaps (long repeated phrases)
    let has_gaps = detect_transcript_gaps(&transcript_llm);
    if has_gaps {
        emit_brief_progress(
            sink, job_id, "brief-gaps",
            "Detected possible ASR gaps or repeated phrases in transcript. Brief quality may be reduced.",
        );
    }

    // ── Determine source name ──────────────────────────────────────────────────
    let source_name = video_path
        .or(Some(transcript_path))
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .unwrap_or("recording");

    // ── Create temp frames directory (sibling to transcript) ──────────────────
    let stem = transcript_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("brief");
    let out_dir = transcript_path
        .parent()
        .unwrap_or(Path::new("."));
    let frames_dir = std::env::temp_dir().join(format!("v2t-brief-frames-{stem}"));

    // ── Frames: scene-change first; LLM only selects/captions ─────────────────
    let frames: Vec<BriefFrame> = if let Some(video) = video_path.filter(|p| p.is_file()) {
        let ffmpeg_path = deps::resolve_tool_path(settings.ffmpeg_path.as_deref(), "ffmpeg")
            .ok_or("ffmpeg not found — cannot extract frames for Brief")?;

        emit_brief_progress(
            sink, job_id, "brief-scenes",
            "Detecting scene changes for screenshots…",
        );

        let scenes = brief_frames::detect_scene_changes(&ffmpeg_path, video, cancel).await?;
        emit_brief_progress(
            sink, job_id, "brief-scenes-done",
            &format!("Found {} scene-change candidate(s).", scenes.len()),
        );

        if cancel.is_cancelled() {
            return Err(pipeline::JOB_CANCELLED_MSG.to_string());
        }

        let duration_secs = brief_frames::probe_duration_secs(&ffmpeg_path, video)
            .await
            .unwrap_or(0.0);
        if duration_secs > 1.0 {
            emit_brief_progress(
                sink,
                job_id,
                "brief-duration",
                &format!("Video duration ≈ {:.0}s.", duration_secs),
            );
        }

        // FDD: LLM timestamps (validated to duration). Brief: scene-first.
        let anchors = select_scene_anchors(
            provider.as_ref(),
            &transcript_llm,
            &scenes,
            brief_frames::MAX_FRAMES,
            settings.brief_doc_mode,
            duration_secs,
            cancel,
            sink,
            job_id,
        )
        .await?;

        emit_brief_progress(
            sink, job_id, "brief-frames",
            &format!(
                "Extracting {} screenshot(s) (screen-only crop when layout looks like Teams/Meet)…",
                anchors.len()
            ),
        );

        let extracted =
            brief_frames::extract_frames(&ffmpeg_path, video, &anchors, &frames_dir, cancel).await?;
        emit_brief_progress(
            sink, job_id, "brief-frames-done",
            &format!("Extracted {} unique screenshot(s).", extracted.len()),
        );
        extracted
    } else {
        // No video — Brief without screenshots + warning (D5)
        emit_brief_progress(
            sink, job_id, "brief-no-video",
            "No video available — Brief will be generated without screenshots.",
        );
        vec![]
    };

    if cancel.is_cancelled() {
        cleanup_frames_dir(&frames_dir);
        return Err(pipeline::JOB_CANCELLED_MSG.to_string());
    }

    // ── FDD caption-gate: drop attendees / calendar / chat-only frames ────────
    let frames = if matches!(settings.brief_doc_mode, BriefDocMode::Fdd) && !frames.is_empty() {
        let before = frames.len();
        emit_brief_progress(
            sink,
            job_id,
            "brief-gate",
            &format!("Caption-gate: classifying {before} screenshot(s) for product UI…"),
        );
        let decisions = match provider.gate_frames(&frames, cancel).await {
            Ok(d) => d,
            Err(e) if e == pipeline::JOB_CANCELLED_MSG => {
                cleanup_frames_dir(&frames_dir);
                return Err(e);
            }
            Err(e) => {
                emit_brief_progress(
                    sink,
                    job_id,
                    "brief-gate-fallback",
                    &format!("Caption-gate LLM failed ({e}) — heuristic filter."),
                );
                brief_gate::heuristic_decisions(&frames)
            }
        };
        let dropped: Vec<String> = frames
            .iter()
            .zip(decisions.iter())
            .filter(|(_, d)| !d.keep)
            .map(|(f, d)| {
                format!(
                    "{}@{} ({})",
                    f.label,
                    f.timestamp_str(),
                    if d.reason.is_empty() { "chrome" } else { d.reason.as_str() }
                )
            })
            .collect();
        let gated = brief_gate::apply_gate_decisions(&frames, &decisions);
        emit_brief_progress(
            sink,
            job_id,
            "brief-gate-done",
            &format!(
                "Caption-gate kept {}/{} frame(s).{}",
                gated.len(),
                before,
                if dropped.is_empty() {
                    String::new()
                } else {
                    format!(" Dropped: {}.", dropped.join("; "))
                }
            ),
        );
        gated
    } else {
        frames
    };

    // ── Pass 2: generate design doc ───────────────────────────────────────────
    let mode_label = settings.brief_doc_mode.label();
    emit_brief_progress(
        sink,
        job_id,
        "brief-doc",
        &format!("Generating {mode_label} (LLM)…"),
    );

    let mut doc = match provider.generate_doc(&transcript_llm, &frames, source_name, cancel).await {
        Ok(d) => d,
        Err(e) => {
            cleanup_frames_dir(&frames_dir);
            return Err(format!("{mode_label} document generation failed: {e}"));
        }
    };
    doc.doc_mode = settings.brief_doc_mode;

    // Annotate with any pipeline context
    if has_gaps {
        doc.has_transcript_gaps = true;
    }
    if frames.is_empty() && video_path.map(|p| !p.is_file()).unwrap_or(true) {
        doc.no_video_warning = Some(
            "Video was not available during design-doc generation — visual appendix is empty. \
             Enable 'Save downloaded video' or use a local video file for screenshots."
                .to_string(),
        );
    }
    // Update frame captions from screenshot_annotations
    let mut frames_with_captions: Vec<BriefFrame> = frames;
    for ann in &doc.screenshot_annotations {
        if let Some(frame) = frames_with_captions.iter_mut().find(|f| f.index == ann.frame_index) {
            frame.caption = ann.caption.clone();
        }
    }

    // Harvest ADO / work-item ids from transcript + captions (FDD); never invent.
    if matches!(settings.brief_doc_mode, BriefDocMode::Fdd) {
        let mut hay = transcript_llm.clone();
        for f in &frames_with_captions {
            if !f.caption.is_empty() {
                hay.push('\n');
                hay.push_str(&f.caption);
            }
        }
        let ids = brief_ado::harvest_ado_ids(&hay);
        if !ids.is_empty() {
            brief_ado::merge_work_items_into_doc(&mut doc, &ids, "transcript+captions");
            emit_brief_progress(
                sink,
                job_id,
                "brief-ado",
                &format!("Harvested {} ADO work-item id(s): {}.", ids.len(), ids.join(", ")),
            );
        }
    }

    // ── Markdown (primary shareable doc, video-to-spec style) ─────────────────
    emit_brief_progress(sink, job_id, "brief-md", "Writing Markdown + screenshots…");
    let md_path = match brief_md::write_design_markdown(
        &doc,
        &frames_with_captions,
        out_dir,
        stem,
    ) {
        Ok(p) => {
            emit_brief_progress(
                sink,
                job_id,
                "brief-md-done",
                &format!("Markdown saved: {}", p.display()),
            );
            Some(p)
        }
        Err(e) => {
            emit_brief_progress(
                sink,
                job_id,
                "brief-md-warn",
                &format!("Markdown write failed (continuing with PDF): {e}"),
            );
            None
        }
    };

    // ── Render PDF ────────────────────────────────────────────────────────────
    emit_brief_progress(sink, job_id, "brief-pdf", "Rendering PDF…");

    let suffix = settings.brief_doc_mode.pdf_suffix();
    let pdf_path = out_dir.join(format!("{stem}.{suffix}.pdf"));
    if let Err(e) = brief_pdf::render_brief_pdf(&doc, &frames_with_captions, &pdf_path) {
        cleanup_frames_dir(&frames_dir);
        return Err(format!("PDF rendering failed: {e}"));
    }

    // ── Cleanup temp frames ───────────────────────────────────────────────────
    cleanup_frames_dir(&frames_dir);

    let saved = md_path
        .as_ref()
        .map(|p| format!("{} and {}", p.display(), pdf_path.display()))
        .unwrap_or_else(|| pdf_path.display().to_string());
    emit_brief_progress(
        sink, job_id, "brief-done",
        &format!("{mode_label} saved: {saved}"),
    );

    Ok(pdf_path)
}

// ── Helpers ────────────────────────────────────────────────────────────────────

fn emit_brief_progress(sink: &SinkHandle, job_id: &str, phase: &str, message: &str) {
    sink.emit(crate::progress::JobEvent::QueueJobProgress(QueueJobProgressEvent {
        job_id: job_id.to_string(),
        phase: phase.to_string(),
        message: message.to_string(),
        subtask_index: None,
        subtask_total: None,
        subtask_percent: None,
    }));
}

fn cleanup_frames_dir(frames_dir: &Path) {
    if frames_dir.is_dir() {
        let _ = std::fs::remove_dir_all(frames_dir);
    }
}

/// Truncate transcript to `max_chars` at a line boundary if possible.
fn truncate_transcript(text: &str, max_chars: usize) -> (String, bool) {
    if text.len() <= max_chars {
        return (text.to_string(), false);
    }
    // Find a newline boundary near the limit
    let slice = &text[..max_chars];
    let cut = slice.rfind('\n').unwrap_or(max_chars);
    let truncated = format!(
        "{}\n\n[Transcript truncated — document covers first {} characters only.]",
        &text[..cut],
        max_chars,
    );
    (truncated, true)
}

/// Detect hallucination loops: same line repeated 5+ times in a 100-line window.
fn detect_transcript_gaps(text: &str) -> bool {
    let lines: Vec<&str> = text.lines().collect();
    for window in lines.windows(100.min(lines.len())) {
        let mut counts = std::collections::HashMap::new();
        for line in window {
            let trimmed = line.trim();
            if trimmed.len() > 10 {
                *counts.entry(trimmed).or_insert(0u32) += 1;
            }
        }
        if counts.values().any(|&c| c >= 5) {
            return true;
        }
    }
    false
}

/// Choose design-doc anchors.
///
/// - **FDD**: LLM timestamps are authoritative (like video-to-spec). Soft-snap to
///   a nearby scene-cut when one exists; never drop a moment just because ffmpeg
///   found few scene changes.
/// - **Brief**: ≤ `max_frames` scenes → keep all; else LLM pick hard-snapped to scenes.
async fn select_scene_anchors(
    provider: &dyn BriefProvider,
    transcript: &str,
    scenes: &[f64],
    max_frames: usize,
    mode: BriefDocMode,
    duration_secs: f64,
    cancel: &CancellationToken,
    sink: &SinkHandle,
    job_id: &str,
) -> Result<Vec<BriefAnchor>, String> {
    if matches!(mode, BriefDocMode::Fdd) {
        return select_fdd_anchors(
            provider,
            transcript,
            scenes,
            max_frames,
            duration_secs,
            cancel,
            sink,
            job_id,
        )
        .await;
    }

    if scenes.is_empty() {
        emit_brief_progress(
            sink,
            job_id,
            "brief-scenes-empty",
            "No scene changes detected — falling back to transcript heuristics.",
        );
        return Ok(heuristic_anchors(transcript, MIN_ANCHORS, max_frames));
    }

    if scenes.len() <= max_frames {
        return Ok(brief_frames::scenes_to_anchors(scenes));
    }

    emit_brief_progress(
        sink,
        job_id,
        "brief-scene-select",
        &format!("Too many scenes ({}) — asking LLM to pick up to {max_frames}…", scenes.len()),
    );

    match provider
        .detect_moments(transcript, MIN_ANCHORS, max_frames, cancel)
        .await
    {
        Ok(chosen) => Ok(snap_anchors_to_scenes(chosen, scenes, max_frames)),
        Err(e) if e == pipeline::JOB_CANCELLED_MSG => Err(e),
        Err(e) => {
            emit_brief_progress(
                sink,
                job_id,
                "brief-scene-select-fallback",
                &format!("LLM scene pick failed ({e}) — uniform sample of scene changes."),
            );
            let sampled = brief_frames::uniformly_sample(scenes, max_frames);
            Ok(brief_frames::scenes_to_anchors(&sampled))
        }
    }
}

/// FDD: transcript → LLM anchors at real timestamps (not limited by scene count).
async fn select_fdd_anchors(
    provider: &dyn BriefProvider,
    transcript: &str,
    scenes: &[f64],
    max_frames: usize,
    duration_secs: f64,
    cancel: &CancellationToken,
    sink: &SinkHandle,
    job_id: &str,
) -> Result<Vec<BriefAnchor>, String> {
    let want_max = FDD_MAX_ANCHORS.min(max_frames);
    let want_min = FDD_MIN_ANCHORS.min(want_max);

    emit_brief_progress(
        sink,
        job_id,
        "brief-scene-select",
        &format!(
            "FDD mode — asking LLM for {want_min}–{want_max} product moments (LLM timestamps, soft scene snap)…"
        ),
    );

    match provider
        .detect_moments(transcript, want_min, want_max, cancel)
        .await
    {
        Ok(chosen) => {
            let finalized = finalize_fdd_anchors(chosen, scenes, want_max, duration_secs);
            emit_brief_progress(
                sink,
                job_id,
                "brief-anchors-ready",
                &format!(
                    "FDD anchors ready: {} moment(s) (scenes available: {}, duration ≈ {:.0}s).",
                    finalized.len(),
                    scenes.len(),
                    duration_secs
                ),
            );
            Ok(finalized)
        }
        Err(e) if e == pipeline::JOB_CANCELLED_MSG => Err(e),
        Err(e) => {
            emit_brief_progress(
                sink,
                job_id,
                "brief-scene-select-fallback",
                &format!("FDD LLM anchors failed ({e}) — scenes/heuristics fallback."),
            );
            if !scenes.is_empty() {
                let sampled = brief_frames::uniformly_sample(scenes, want_max);
                Ok(brief_frames::scenes_to_anchors(&sampled))
            } else {
                Ok(heuristic_anchors(transcript, want_min, want_max))
            }
        }
    }
}

/// Keep LLM times; coerce to duration; optional soft-snap; gap; top up from scenes.
fn finalize_fdd_anchors(
    chosen: Vec<BriefAnchor>,
    scenes: &[f64],
    max_frames: usize,
    duration_secs: f64,
) -> Vec<BriefAnchor> {
    let mut ranked = chosen;
    ranked.sort_by(|a, b| {
        b.importance
            .cmp(&a.importance)
            .then(
                a.seconds
                    .partial_cmp(&b.seconds)
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
    });

    let mut out: Vec<BriefAnchor> = Vec::new();
    for mut anchor in ranked {
        if out.len() >= max_frames {
            break;
        }
        let Some(secs) = crate::brief_provider::coerce_anchor_seconds(anchor.seconds, duration_secs)
            .or_else(|| {
                if duration_secs <= 1.0 {
                    Some(anchor.seconds.max(0.0))
                } else {
                    None
                }
            })
        else {
            continue;
        };
        // Screen has usually settled a beat after the spoken cue.
        anchor.seconds = (secs + FDD_ANCHOR_OFFSET_SECS).max(0.5);
        if duration_secs > 1.0 {
            anchor.seconds = anchor.seconds.min((duration_secs - 0.5).max(0.0));
        }

        if let Some(&scene) = scenes
            .iter()
            .min_by(|a, b| {
                (*a - anchor.seconds)
                    .abs()
                    .partial_cmp(&(*b - anchor.seconds).abs())
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .filter(|s| (*s - anchor.seconds).abs() <= FDD_SOFT_SNAP_SECS)
        {
            anchor.seconds = scene;
        }

        if out
            .iter()
            .any(|o| (o.seconds - anchor.seconds).abs() < FDD_MIN_GAP_SECS)
        {
            continue;
        }
        if anchor.label.trim().is_empty() {
            anchor.label = format!("moment_{:02}", out.len());
        }
        out.push(anchor);
    }

    // Top up from scene changes when the LLM returned too few.
    if out.len() < FDD_MIN_ANCHORS {
        for &scene in scenes {
            if out.len() >= max_frames {
                break;
            }
            if out
                .iter()
                .any(|o| (o.seconds - scene).abs() < FDD_MIN_GAP_SECS)
            {
                continue;
            }
            out.push(BriefAnchor {
                seconds: scene,
                label: format!("scene_{:02}", out.len()),
                reason: format!("Scene change at {}", format_hms_pipeline(scene)),
                importance: 2,
            });
        }
    }

    out.sort_by(|a, b| a.seconds.partial_cmp(&b.seconds).unwrap_or(std::cmp::Ordering::Equal));
    out.truncate(max_frames);
    out
}

/// Map LLM-chosen moments onto the nearest scene-change timestamps.
fn snap_anchors_to_scenes(
    chosen: Vec<BriefAnchor>,
    scenes: &[f64],
    max_frames: usize,
) -> Vec<BriefAnchor> {
    const MAX_SNAP_SECS: f64 = 20.0;
    let mut out: Vec<BriefAnchor> = Vec::new();
    let mut used: Vec<f64> = Vec::new();

    for mut anchor in chosen {
        let Some((scene, dist)) = scenes
            .iter()
            .map(|&s| (s, (s - anchor.seconds).abs()))
            .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
        else {
            continue;
        };
        if dist > MAX_SNAP_SECS {
            continue;
        }
        if used.iter().any(|&u| (u - scene).abs() < 0.5) {
            continue;
        }
        anchor.seconds = scene;
        if anchor.label.is_empty() {
            anchor.label = format!("scene_{:02}", out.len());
        }
        used.push(scene);
        out.push(anchor);
        if out.len() >= max_frames {
            break;
        }
    }

    if out.len() < MIN_ANCHORS {
        for &scene in scenes {
            if out.len() >= max_frames {
                break;
            }
            if used.iter().any(|&u| (u - scene).abs() < 0.5) {
                continue;
            }
            out.push(BriefAnchor {
                seconds: scene,
                label: format!("scene_{:02}", out.len()),
                reason: format!("Scene change at {}", format_hms_pipeline(scene)),
                importance: 2,
            });
            used.push(scene);
        }
    }

    out.sort_by(|a, b| a.seconds.partial_cmp(&b.seconds).unwrap_or(std::cmp::Ordering::Equal));
    out.truncate(max_frames);
    out
}

fn format_hms_pipeline(seconds: f64) -> String {
    let total = seconds.max(0.0) as u64;
    format!(
        "{:02}:{:02}:{:02}",
        total / 3600,
        (total % 3600) / 60,
        total % 60
    )
}

/// Transcript heuristic when scene-detect found nothing.
fn heuristic_anchors(transcript: &str, min: usize, max: usize) -> Vec<BriefAnchor> {
    let deictic = [
        "here", "screen", "diagram", "code", "this", "look", "see", "table", "shown", "demo",
    ];

    let mut anchors: Vec<BriefAnchor> = Vec::new();
    let mut last_seconds = -60.0f64;

    for line in transcript.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        if let Some(seconds) = parse_vtt_start_time(trimmed) {
            if seconds - last_seconds < 60.0 {
                continue;
            }
            last_seconds = seconds;
            anchors.push(BriefAnchor {
                seconds,
                label: format!("moment_{:02}", anchors.len() + 1),
                reason: String::new(),
                importance: 3,
            });
            if anchors.len() >= max {
                break;
            }
            continue;
        }

        let lower = trimmed.to_lowercase();
        if deictic.iter().any(|&d| lower.contains(d)) {
            if let Some(anchor) = anchors.last_mut() {
                if anchor.reason.is_empty() {
                    anchor.reason = trimmed.chars().take(120).collect();
                }
            }
        }
    }

    if anchors.len() < min {
        let total_lines = transcript.lines().count();
        let step = (total_lines / (min + 1)).max(1);
        let mut extra_idx = 0usize;
        for line in transcript.lines().step_by(step) {
            if anchors.len() >= min {
                break;
            }
            if let Some(seconds) = parse_vtt_start_time(line.trim()) {
                if !anchors.iter().any(|a| (a.seconds - seconds).abs() < 60.0) {
                    anchors.push(BriefAnchor {
                        seconds,
                        label: format!("frame_{extra_idx:02}"),
                        reason: String::new(),
                        importance: 2,
                    });
                    extra_idx += 1;
                }
            }
        }
        anchors.sort_by(|a, b| a.seconds.partial_cmp(&b.seconds).unwrap_or(std::cmp::Ordering::Equal));
    }

    anchors
}

/// Parse the start time from a VTT cue timestamp "HH:MM:SS.mmm --> HH:MM:SS.mmm".
fn parse_vtt_start_time(line: &str) -> Option<f64> {
    let arrow = line.find("-->")?;
    let start = &line[..arrow].trim();
    crate::brief_provider::parse_timestamp(start.split('.').next().unwrap_or(start))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_at_boundary() {
        let text = "line1\nline2\nline3";
        let (t, was) = truncate_transcript(text, 10);
        assert!(was);
        assert!(t.starts_with("line1\n") || t.starts_with("line1"));
    }

    #[test]
    fn truncate_no_truncate_needed() {
        let text = "short";
        let (t, was) = truncate_transcript(text, 100);
        assert!(!was);
        assert_eq!(t, "short");
    }

    #[test]
    fn detect_gaps_repeated_lines() {
        let repeated = "let me share my screen\n".repeat(10);
        assert!(detect_transcript_gaps(&repeated));
    }

    #[test]
    fn detect_gaps_normal_text() {
        let text = "Hello world\nThis is a test\nDifferent content\nMore stuff";
        assert!(!detect_transcript_gaps(text));
    }

    #[test]
    fn parse_vtt_start_time_basic() {
        assert!(parse_vtt_start_time("00:05:30.000 --> 00:05:35.000").is_some());
        assert_eq!(parse_vtt_start_time("00:05:30.000 --> 00:05:35.000"), Some(330.0));
    }

    #[test]
    fn fdd_anchors_keep_llm_times_without_nearby_scene() {
        // Only early scenes — old hard-snap would drop mid-meeting LLM moments.
        let scenes = vec![1.0, 140.0, 154.0];
        let chosen = vec![
            BriefAnchor {
                seconds: 500.0,
                label: "hotfix_scope".into(),
                reason: "earnings without sync".into(),
                importance: 5,
            },
            BriefAnchor {
                seconds: 680.0,
                label: "ui_synced_flag".into(),
                reason: "synced to erp visibility".into(),
                importance: 5,
            },
            BriefAnchor {
                seconds: 1110.0,
                label: "popup_unsynced".into(),
                reason: "popup before statements".into(),
                importance: 4,
            },
        ];
        let out = finalize_fdd_anchors(chosen, &scenes, 20, 40.0 * 60.0);
        assert!(out.len() >= 3, "expected LLM moments kept, got {}", out.len());
        assert!(out.iter().any(|a| a.label == "hotfix_scope"));
        // +2s offset applied when not soft-snapped
        let hotfix = out.iter().find(|a| a.label == "hotfix_scope").unwrap();
        assert!((hotfix.seconds - 502.0).abs() < 0.01);
    }

    #[test]
    fn fdd_anchors_coerce_out_of_range_hour_bug() {
        let scenes: Vec<f64> = vec![];
        let chosen = vec![BriefAnchor {
            // Literal 08:01:19 → 28879s; coerce to ~500s on a 30min tape.
            seconds: crate::brief_provider::parse_timestamp("08:01:19").unwrap(),
            label: "hotfix_scope".into(),
            reason: "test".into(),
            importance: 5,
        }];
        let out = finalize_fdd_anchors(chosen, &scenes, 20, 30.0 * 60.0);
        assert_eq!(out.len(), 1);
        assert!((out[0].seconds - 502.0).abs() < 1.0, "got {}", out[0].seconds);
    }

    // Integration test omitted from unit tests (requires tauri mock app + AppHandle).
    // Covered by manual smoke test and e2e tests.
}
