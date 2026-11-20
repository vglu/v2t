//! Frame extraction for the Brief pipeline.
//!
//! Primary source of timestamps: ffmpeg **scene-change** detection.
//! Each extracted frame is optionally **screen-only cropped** (Teams/Meet
//! participant strip on the right + speaker-name bar at the bottom).
//! Near-duplicate frames are dropped via a perceptual dHash.

use std::path::{Path, PathBuf};
use std::process::Stdio;

use tokio_util::sync::CancellationToken;

use crate::brief_provider::{BriefAnchor, BriefFrame};

/// Maximum frames extracted per job (D12: ~20–30).
pub const MAX_FRAMES: usize = 25;
/// How many raw scene hits to keep before LLM / uniform selection.
pub const MAX_SCENE_CANDIDATES: usize = 80;
/// ffmpeg `scene` score threshold (0–1). Higher = fewer cuts.
const SCENE_THRESHOLD: f64 = 0.28;
/// Minimum gap between accepted scene cuts (seconds).
const MIN_SCENE_GAP_SECS: f64 = 4.0;
/// Hamming-distance threshold for perceptual dedup (lower = stricter).
const DEDUP_THRESHOLD: u32 = 10;
/// Hash grid side: 9×9 grayscale for dHash.
const HASH_GRID: usize = 9;

/// Detect scene-change timestamps in `video` via ffmpeg `select=gt(scene,…)`.
///
/// Returns seconds from start, sorted, deduped by [`MIN_SCENE_GAP_SECS`], capped
/// at [`MAX_SCENE_CANDIDATES`]. Always includes a frame near the start (~1s) so
/// a static title slide is not missed.
pub async fn detect_scene_changes(
    ffmpeg: &Path,
    video: &Path,
    cancel: &CancellationToken,
) -> Result<Vec<f64>, String> {
    if cancel.is_cancelled() {
        return Err(crate::pipeline::JOB_CANCELLED_MSG.to_string());
    }

    let video_s = video.to_str().ok_or("non-UTF8 video path")?;
    let filter = format!("select='gt(scene,{SCENE_THRESHOLD})',showinfo");

    let output = tokio::process::Command::new(ffmpeg)
        .args([
            "-hide_banner",
            "-i",
            video_s,
            "-vf",
            &filter,
            "-an",
            "-f",
            "null",
            "-",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .await
        .map_err(|e| format!("ffmpeg scene-detect spawn failed: {e}"))?;

    if cancel.is_cancelled() {
        return Err(crate::pipeline::JOB_CANCELLED_MSG.to_string());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    let mut times = parse_showinfo_pts(&stderr);

    // Ensure we always have an early frame (title / first screen).
    if times.first().map(|&t| t > 2.5).unwrap_or(true) {
        times.insert(0, 1.0);
    }

    let spaced = enforce_min_gap(times, MIN_SCENE_GAP_SECS);
    let capped = if spaced.len() > MAX_SCENE_CANDIDATES {
        uniformly_sample(&spaced, MAX_SCENE_CANDIDATES)
    } else {
        spaced
    };

    if capped.is_empty() {
        // Fallback: a few evenly spaced probes if scene-detect found nothing.
        let duration = probe_duration_secs(ffmpeg, video).await.unwrap_or(600.0);
        let n = 8usize.min(MAX_FRAMES);
        let step = (duration / (n as f64 + 1.0)).max(30.0);
        Ok((1..=n).map(|i| (i as f64) * step).filter(|&t| t < duration).collect())
    } else {
        Ok(capped)
    }
}

/// Convert scene timestamps into provisional anchors (LLM may re-label later).
pub fn scenes_to_anchors(scenes: &[f64]) -> Vec<BriefAnchor> {
    scenes
        .iter()
        .enumerate()
        .map(|(i, &seconds)| BriefAnchor {
            seconds,
            label: format!("scene_{i:02}"),
            reason: format!("Scene change at {}", format_hms(seconds)),
            importance: 3,
        })
        .collect()
}

/// Uniformly pick `n` timestamps from a sorted list (keeps first and last).
pub fn uniformly_sample(times: &[f64], n: usize) -> Vec<f64> {
    if times.is_empty() || n == 0 {
        return Vec::new();
    }
    if times.len() <= n {
        return times.to_vec();
    }
    if n == 1 {
        return vec![times[times.len() / 2]];
    }
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let idx = i * (times.len() - 1) / (n - 1);
        out.push(times[idx]);
    }
    out.dedup_by(|a, b| (*a - *b).abs() < 0.05);
    out
}

/// Extract frames at the given anchor timestamps from `video`.
///
/// Applies screen-only crop when the heuristic says the recording looks like a
/// Teams/Meet share (right participant strip + bottom name bar). Deduplicates
/// near-identical frames and caps at [`MAX_FRAMES`].
pub async fn extract_frames(
    ffmpeg: &Path,
    video: &Path,
    anchors: &[BriefAnchor],
    out_dir: &Path,
    cancel: &CancellationToken,
) -> Result<Vec<BriefFrame>, String> {
    std::fs::create_dir_all(out_dir)
        .map_err(|e| format!("Cannot create frames dir {}: {e}", out_dir.display()))?;

    let mut sorted: Vec<&BriefAnchor> = anchors.iter().collect();
    sorted.sort_by(|a, b| a.seconds.partial_cmp(&b.seconds).unwrap_or(std::cmp::Ordering::Equal));

    if sorted.len() > MAX_FRAMES {
        sorted.sort_by(|a, b| {
            b.importance
                .cmp(&a.importance)
                .then(a.seconds.partial_cmp(&b.seconds).unwrap_or(std::cmp::Ordering::Equal))
        });
        sorted.truncate(MAX_FRAMES);
        sorted.sort_by(|a, b| a.seconds.partial_cmp(&b.seconds).unwrap_or(std::cmp::Ordering::Equal));
    }

    let (width, height) = probe_video_size(ffmpeg, video).await.unwrap_or((0, 0));
    let crop_vf = screen_share_crop_vf(width, height);

    let mut frames: Vec<BriefFrame> = Vec::new();
    let mut seen_hashes: Vec<u64> = Vec::new();

    for (raw_idx, anchor) in sorted.iter().enumerate() {
        if cancel.is_cancelled() {
            return Err(crate::pipeline::JOB_CANCELLED_MSG.to_string());
        }

        let safe_label = sanitize_label(&anchor.label);
        let filename = format!("{:02}_{safe_label}.png", raw_idx);
        let dest = out_dir.join(&filename);

        if let Err(e) =
            extract_one_frame(ffmpeg, video, anchor.seconds, &dest, crop_vf.as_deref()).await
        {
            eprintln!(
                "[brief_frames] Skipping anchor '{}' at {:.1}s: {e}",
                anchor.label, anchor.seconds
            );
            continue;
        }

        // Second pass: trim desktop wallpaper / empty margins to the active UI window.
        if let Err(e) = refine_png_to_content_window(&dest) {
            eprintln!(
                "[brief_frames] Content-window refine skipped for {}: {e}",
                dest.display()
            );
        }

        match perceptual_hash(ffmpeg, &dest) {
            Ok(hash) => {
                if is_duplicate(hash, &seen_hashes, DEDUP_THRESHOLD) {
                    let _ = std::fs::remove_file(&dest);
                    continue;
                }
                seen_hashes.push(hash);
            }
            Err(e) => {
                eprintln!("[brief_frames] Could not hash {}: {e}", dest.display());
            }
        }

        frames.push(BriefFrame {
            index: frames.len(),
            seconds: anchor.seconds,
            label: anchor.label.clone(),
            path: dest,
            caption: anchor.reason.clone(),
        });
    }

    Ok(frames)
}

/// Pixel crop rectangle (x, y, w, h) in image coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CropRect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

impl CropRect {
    pub fn to_ffmpeg_vf(self) -> String {
        format!("crop={}:{}:{}:{}", self.w, self.h, self.x, self.y)
    }
}

/// Heuristic crop for shared-screen recordings (Teams / Meet / Zoom gallery).
///
/// - Landscape ≥ 1280×720 → drop right strip (≤18% / 320px) and bottom bar (≤10% / 96px).
/// - Otherwise → `None` (full frame; safer when layout is unknown).
///
/// After extract, [`refine_png_to_content_window`] further trims wallpaper.
pub fn screen_share_crop_vf(width: u32, height: u32) -> Option<String> {
    if width < 1280 || height < 720 {
        return None;
    }
    // Prefer landscape meeting captures; skip tall/portrait.
    if height as f64 / width as f64 > 0.85 {
        return None;
    }

    let right = ((width as f64) * 0.16).round() as u32;
    let right = right.clamp(160, 320);
    let bottom = ((height as f64) * 0.08).round() as u32;
    let bottom = bottom.clamp(48, 96);

    let crop_w = width.saturating_sub(right);
    let crop_h = height.saturating_sub(bottom);
    if crop_w < 640 || crop_h < 360 {
        return None;
    }

    Some(format!("crop={crop_w}:{crop_h}:0:0"))
}

/// Open a PNG and, if a clear UI window is detected, overwrite the file cropped to it.
///
/// Returns `Ok(true)` when the file was rewritten, `Ok(false)` when left unchanged.
pub fn refine_png_to_content_window(path: &Path) -> Result<bool, String> {
    let img = image::open(path).map_err(|e| format!("open png: {e}"))?;
    let rgb = img.to_rgb8();
    let width = rgb.width();
    let height = rgb.height();
    if width < 640 || height < 360 {
        return Ok(false);
    }

    let mut luma = Vec::with_capacity((width * height) as usize);
    for p in rgb.pixels() {
        // Rec. 601 luma
        let y = (0.299 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32) as u8;
        luma.push(y);
    }

    let Some(rect) = content_window_bbox(&luma, width, height) else {
        return Ok(false);
    };

    // Skip tiny gains (almost full frame).
    let area = (rect.w as u64) * (rect.h as u64);
    let full = (width as u64) * (height as u64);
    if area * 100 / full >= 92 {
        return Ok(false);
    }

    let cropped = image::imageops::crop_imm(&rgb, rect.x, rect.y, rect.w, rect.h).to_image();
    cropped
        .save(path)
        .map_err(|e| format!("save cropped png: {e}"))?;
    Ok(true)
}

/// Find the bounding box of high-activity UI content (text/controls), excluding
/// smooth wallpaper margins. Operates on 8-bit luma.
///
/// Returns `None` when activity is too sparse or fills the whole frame.
pub fn content_window_bbox(luma: &[u8], width: u32, height: u32) -> Option<CropRect> {
    if luma.len() != (width as usize) * (height as usize) || width < 32 || height < 32 {
        return None;
    }

    const GRID_W: usize = 32;
    const GRID_H: usize = 24;
    let cell_w = width as usize / GRID_W;
    let cell_h = height as usize / GRID_H;
    if cell_w == 0 || cell_h == 0 {
        return None;
    }

    let mut active = [[false; GRID_W]; GRID_H];
    let mut scores = [[0.0f32; GRID_W]; GRID_H];

    for gy in 0..GRID_H {
        for gx in 0..GRID_W {
            let x0 = gx * cell_w;
            let y0 = gy * cell_h;
            let x1 = if gx + 1 == GRID_W {
                width as usize
            } else {
                x0 + cell_w
            };
            let y1 = if gy + 1 == GRID_H {
                height as usize
            } else {
                y0 + cell_h
            };

            let mut sum = 0u64;
            let mut sum_sq = 0u64;
            let mut n = 0u64;
            for y in y0..y1 {
                let row = y * width as usize;
                for x in x0..x1 {
                    let v = luma[row + x] as u64;
                    sum += v;
                    sum_sq += v * v;
                    n += 1;
                }
            }
            if n == 0 {
                continue;
            }
            let mean = sum as f32 / n as f32;
            let var = (sum_sq as f32 / n as f32) - mean * mean;
            scores[gy][gx] = var;
        }
    }

    // Adaptive threshold: cells above ~40th percentile of variance are "UI-like".
    let mut flat: Vec<f32> = scores.iter().flatten().copied().collect();
    flat.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let thresh = flat
        .get(flat.len() * 55 / 100)
        .copied()
        .unwrap_or(80.0)
        .max(40.0);

    for gy in 0..GRID_H {
        for gx in 0..GRID_W {
            active[gy][gx] = scores[gy][gx] >= thresh;
        }
    }

    let mut min_gx = GRID_W;
    let mut max_gx = 0usize;
    let mut min_gy = GRID_H;
    let mut max_gy = 0usize;
    let mut count = 0usize;
    for gy in 0..GRID_H {
        for gx in 0..GRID_W {
            if active[gy][gx] {
                count += 1;
                min_gx = min_gx.min(gx);
                max_gx = max_gx.max(gx);
                min_gy = min_gy.min(gy);
                max_gy = max_gy.max(gy);
            }
        }
    }

    if count < 6 {
        return None;
    }

    // Pad one cell so window chrome / title bars survive.
    let min_gx = min_gx.saturating_sub(1);
    let min_gy = min_gy.saturating_sub(1);
    let max_gx = (max_gx + 1).min(GRID_W - 1);
    let max_gy = (max_gy + 1).min(GRID_H - 1);

    let x = (min_gx * cell_w) as u32;
    let y = (min_gy * cell_h) as u32;
    let w = ((max_gx + 1) * cell_w).min(width as usize) as u32 - x;
    let h = ((max_gy + 1) * cell_h).min(height as usize) as u32 - y;

    if w < 480 || h < 270 {
        return None;
    }

    let area = (w as u64) * (h as u64);
    let full = (width as u64) * (height as u64);
    let pct = area * 100 / full;
    // Must remove a meaningful margin (wallpaper) but keep a usable window.
    if pct < 35 || pct > 92 {
        return None;
    }

    Some(CropRect { x, y, w, h })
}

async fn extract_one_frame(
    ffmpeg: &Path,
    video: &Path,
    seconds: f64,
    dest: &Path,
    crop_vf: Option<&str>,
) -> Result<(), String> {
    let seconds_clamped = seconds.max(0.0);
    let mut vf_parts: Vec<String> = Vec::new();
    if let Some(crop) = crop_vf {
        vf_parts.push(crop.to_string());
    }
    vf_parts.push("scale='min(1280,iw)':-2".to_string());
    let vf = vf_parts.join(",");

    let args = [
        "-hide_banner",
        "-loglevel",
        "error",
        "-y",
        "-ss",
        &format!("{seconds_clamped:.3}"),
        "-i",
        video.to_str().ok_or("non-UTF8 video path")?,
        "-frames:v",
        "1",
        "-vf",
        &vf,
        dest.to_str().ok_or("non-UTF8 dest path")?,
    ];

    let output = tokio::process::Command::new(ffmpeg)
        .args(&args)
        .output()
        .await
        .map_err(|e| format!("ffmpeg spawn failed: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("ffmpeg exited {}: {}", output.status, stderr.trim()));
    }

    match dest.metadata() {
        Ok(m) if m.len() >= 1024 => Ok(()),
        Ok(_) => Err(format!(
            "ffmpeg produced an empty/tiny frame file at {}",
            dest.display()
        )),
        Err(e) => Err(format!("Frame file not created: {e}")),
    }
}

fn parse_showinfo_pts(stderr: &str) -> Vec<f64> {
    let mut times = Vec::new();
    for line in stderr.lines() {
        // showinfo lines contain: pts_time:12.345
        if let Some(idx) = line.find("pts_time:") {
            let rest = &line[idx + "pts_time:".len()..];
            let token = rest
                .split(|c: char| c.is_whitespace() || c == ',')
                .next()
                .unwrap_or("");
            if let Ok(t) = token.parse::<f64>() {
                if t.is_finite() && t >= 0.0 {
                    times.push(t);
                }
            }
        }
    }
    times.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    times.dedup_by(|a, b| (*a - *b).abs() < 0.05);
    times
}

fn enforce_min_gap(times: Vec<f64>, min_gap: f64) -> Vec<f64> {
    let mut out = Vec::new();
    for t in times {
        if out
            .last()
            .map(|&prev| t - prev >= min_gap)
            .unwrap_or(true)
        {
            out.push(t);
        }
    }
    out
}

fn format_hms(seconds: f64) -> String {
    let total = seconds.max(0.0) as u64;
    format!(
        "{:02}:{:02}:{:02}",
        total / 3600,
        (total % 3600) / 60,
        total % 60
    )
}

async fn probe_video_size(ffmpeg: &Path, video: &Path) -> Result<(u32, u32), String> {
    let ffprobe = sibling_ffprobe(ffmpeg);
    let tool = if ffprobe.is_file() {
        ffprobe
    } else {
        ffmpeg.to_path_buf()
    };

    // Prefer ffprobe; fall back to ffmpeg -i parse.
    if tool.file_name().and_then(|n| n.to_str()).unwrap_or("").starts_with("ffprobe") {
        let output = tokio::process::Command::new(&tool)
            .args([
                "-v",
                "error",
                "-select_streams",
                "v:0",
                "-show_entries",
                "stream=width,height",
                "-of",
                "csv=p=0:s=x",
                video.to_str().ok_or("non-UTF8 video path")?,
            ])
            .output()
            .await
            .map_err(|e| format!("ffprobe spawn failed: {e}"))?;
        let text = String::from_utf8_lossy(&output.stdout);
        let text = text.trim();
        if let Some((w, h)) = text.split_once('x') {
            let width: u32 = w.trim().parse().map_err(|_| "bad width")?;
            let height: u32 = h.trim().parse().map_err(|_| "bad height")?;
            return Ok((width, height));
        }
    }

    let output = tokio::process::Command::new(ffmpeg)
        .args(["-hide_banner", "-i", video.to_str().ok_or("non-UTF8 video path")?])
        .output()
        .await
        .map_err(|e| format!("ffmpeg probe spawn failed: {e}"))?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    parse_ffmpeg_size(&stderr).ok_or_else(|| "could not parse video size".to_string())
}

pub async fn probe_duration_secs(ffmpeg: &Path, video: &Path) -> Result<f64, String> {
    let ffprobe = sibling_ffprobe(ffmpeg);
    if ffprobe.is_file() {
        let output = tokio::process::Command::new(&ffprobe)
            .args([
                "-v",
                "error",
                "-show_entries",
                "format=duration",
                "-of",
                "default=nw=1:nk=1",
                video.to_str().ok_or("non-UTF8 video path")?,
            ])
            .output()
            .await
            .map_err(|e| format!("ffprobe duration spawn failed: {e}"))?;
        let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if let Ok(d) = text.parse::<f64>() {
            if d.is_finite() && d > 0.0 {
                return Ok(d);
            }
        }
    }
    Err("duration unavailable".to_string())
}

fn sibling_ffprobe(ffmpeg: &Path) -> PathBuf {
    let name = if cfg!(windows) {
        "ffprobe.exe"
    } else {
        "ffprobe"
    };
    ffmpeg
        .parent()
        .map(|d| d.join(name))
        .unwrap_or_else(|| PathBuf::from(name))
}

fn parse_ffmpeg_size(stderr: &str) -> Option<(u32, u32)> {
    // Stream #0:0: Video: h264, yuv420p, 1920x1080, ...
    for line in stderr.lines() {
        if !line.contains("Video:") {
            continue;
        }
        for token in line.split(|c: char| c == ',' || c.is_whitespace()) {
            if let Some((w, h)) = token.split_once('x') {
                if let (Ok(width), Ok(height)) = (w.parse::<u32>(), h.parse::<u32>()) {
                    if width >= 160 && height >= 120 {
                        return Some((width, height));
                    }
                }
            }
        }
    }
    None
}

fn perceptual_hash(ffmpeg: &Path, image_path: &Path) -> Result<u64, String> {
    let output = std::process::Command::new(ffmpeg)
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-i",
            image_path.to_str().ok_or("non-UTF8 path")?,
            "-vf",
            &format!("scale={HASH_GRID}:{HASH_GRID},format=gray"),
            "-f",
            "rawvideo",
            "-pix_fmt",
            "gray",
            "-",
        ])
        .output()
        .map_err(|e| format!("ffmpeg hash spawn failed: {e}"))?;

    let raw = output.stdout;
    let expected = HASH_GRID * HASH_GRID;
    if raw.len() < expected {
        return Err(format!("ffmpeg hash output too short: {} bytes", raw.len()));
    }

    let mut bits: u64 = 0;
    for row in 0..HASH_GRID {
        for col in 0..(HASH_GRID - 1) {
            let a = raw[row * HASH_GRID + col] as u16;
            let b = raw[row * HASH_GRID + col + 1] as u16;
            bits = (bits << 1) | u64::from(a < b);
        }
    }
    let mut bits2: u64 = 0;
    for row in 0..(HASH_GRID - 1) {
        for col in 0..HASH_GRID {
            let a = raw[row * HASH_GRID + col] as u16;
            let b = raw[(row + 1) * HASH_GRID + col] as u16;
            bits2 = (bits2 << 1) | u64::from(a < b);
        }
    }
    Ok(bits ^ bits2.rotate_left(32))
}

fn is_duplicate(candidate: u64, seen: &[u64], threshold: u32) -> bool {
    seen.iter().any(|&h| hamming64(candidate, h) <= threshold)
}

fn hamming64(a: u64, b: u64) -> u32 {
    (a ^ b).count_ones()
}

fn sanitize_label(label: &str) -> String {
    let clean: String = label
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '_' { c } else { '_' })
        .collect::<String>()
        .split('_')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("_");
    clean[..clean.len().min(40)].to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_label_basic() {
        assert_eq!(sanitize_label("Architecture Diagram!"), "architecture_diagram");
        assert_eq!(sanitize_label("some_label_ok"), "some_label_ok");
        assert_eq!(sanitize_label("a-b.c"), "a_b_c");
        assert_eq!(sanitize_label("___"), "");
    }

    #[test]
    fn hamming64_identity() {
        assert_eq!(hamming64(0xDEADBEEF, 0xDEADBEEF), 0);
    }

    #[test]
    fn hamming64_all_different() {
        assert_eq!(hamming64(0, u64::MAX), 64);
    }

    #[test]
    fn dedup_identical() {
        let seen = vec![12345u64];
        assert!(is_duplicate(12345, &seen, 1));
    }

    #[test]
    fn parse_showinfo_pts_extracts_times() {
        let stderr = r#"
[Parsed_showinfo_1 @ 0] n:   0 pts:  12345 pts_time:12.345 pos: ...
[Parsed_showinfo_1 @ 0] n:   1 pts:  23456 pts_time:45.678 pos: ...
"#;
        let times = parse_showinfo_pts(stderr);
        assert_eq!(times, vec![12.345, 45.678]);
    }

    #[test]
    fn enforce_min_gap_drops_close_hits() {
        let times = vec![1.0, 2.0, 10.0, 11.5, 20.0];
        assert_eq!(enforce_min_gap(times, 4.0), vec![1.0, 10.0, 20.0]);
    }

    #[test]
    fn uniformly_sample_keeps_ends() {
        let times: Vec<f64> = (0..10).map(|i| i as f64 * 10.0).collect();
        let sampled = uniformly_sample(&times, 3);
        assert_eq!(sampled.first(), Some(&0.0));
        assert_eq!(sampled.last(), Some(&90.0));
        assert_eq!(sampled.len(), 3);
    }

    #[test]
    fn screen_share_crop_for_1080p() {
        let vf = screen_share_crop_vf(1920, 1080).expect("crop");
        // 1920*0.16=307 → crop 1613×994 from top-left
        assert_eq!(vf, "crop=1613:994:0:0");
    }

    #[test]
    fn screen_share_no_crop_for_small_or_portrait() {
        assert!(screen_share_crop_vf(800, 600).is_none());
        assert!(screen_share_crop_vf(1080, 1920).is_none());
    }

    #[test]
    fn content_window_bbox_finds_busy_center_on_flat_wallpaper() {
        let w = 640u32;
        let h = 360u32;
        let mut luma = vec![30u8; (w * h) as usize]; // flat dark wallpaper
        // Busy UI rectangle in the center with high-frequency pattern
        for y in 60..300 {
            for x in 80..560 {
                let v = if (x + y) % 3 == 0 { 220 } else { 40 };
                luma[(y * w + x) as usize] = v;
            }
        }
        let bbox = content_window_bbox(&luma, w, h).expect("bbox");
        assert!(bbox.x >= 40 && bbox.x <= 120, "x={}", bbox.x);
        assert!(bbox.y >= 20 && bbox.y <= 100, "y={}", bbox.y);
        assert!(bbox.w >= 400, "w={}", bbox.w);
        assert!(bbox.h >= 180, "h={}", bbox.h);
        assert!(bbox.x + bbox.w <= w);
        assert!(bbox.y + bbox.h <= h);
    }

    #[test]
    fn content_window_bbox_rejects_uniform_frame() {
        let w = 640u32;
        let h = 360u32;
        let luma = vec![128u8; (w * h) as usize];
        assert!(content_window_bbox(&luma, w, h).is_none());
    }

    #[test]
    fn scenes_to_anchors_labels() {
        let a = scenes_to_anchors(&[1.0, 12.5]);
        assert_eq!(a.len(), 2);
        assert_eq!(a[0].label, "scene_00");
        assert!((a[1].seconds - 12.5).abs() < f64::EPSILON);
    }

    #[test]
    fn parse_ffmpeg_size_from_banner() {
        let stderr = "Stream #0:0(eng): Video: h264 (High), yuv420p, 1920x1080, 30 fps";
        assert_eq!(parse_ffmpeg_size(stderr), Some((1920, 1080)));
    }
}
