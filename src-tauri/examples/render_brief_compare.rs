//! Offline Brief renderer for A/B compare (no Gemini key required).
//!
//! Usage:
//!   cargo run --example render_brief_compare -- \
//!     --video path.mp4 --anchors anchors.json --doc briefdoc.json --out out.brief.pdf
//!
//! Optional: set V2T_GEMINI_API_KEY to force live Gemini instead of the JSON doc
//! (not used in this example — doc JSON is the source of truth for compare runs).

use std::path::{Path, PathBuf};

use tokio_util::sync::CancellationToken;
use v2t_lib::brief_frames;
use v2t_lib::brief_pdf;
use v2t_lib::brief_provider::{BriefAnchor, BriefDoc, BriefFrame};

fn usage() -> ! {
    eprintln!(
        "Usage: render_brief_compare --video <mp4> --anchors <json> --doc <briefdoc.json> --out <pdf> [--frames-dir <dir>]"
    );
    std::process::exit(2);
}

fn arg_value(args: &[String], name: &str) -> Option<String> {
    args.windows(2)
        .find(|w| w[0] == name)
        .map(|w| w[1].clone())
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let video = arg_value(&args, "--video").unwrap_or_else(|| usage());
    let anchors_path = arg_value(&args, "--anchors").unwrap_or_else(|| usage());
    let doc_path = arg_value(&args, "--doc").unwrap_or_else(|| usage());
    let out_path = arg_value(&args, "--out").unwrap_or_else(|| usage());
    let frames_dir = arg_value(&args, "--frames-dir").unwrap_or_else(|| {
        let stem = Path::new(&out_path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("brief");
        std::env::temp_dir()
            .join(format!("v2t-compare-frames-{stem}"))
            .to_string_lossy()
            .into_owned()
    });

    let ffmpeg = which_ffmpeg();
    let anchors_raw = std::fs::read_to_string(&anchors_path).expect("read anchors");
    let anchors_file: AnchorsFile =
        serde_json::from_str(&anchors_raw).expect("parse anchors.json");
    let anchors: Vec<BriefAnchor> = anchors_file
        .anchors
        .into_iter()
        .map(|a| BriefAnchor {
            seconds: a.seconds,
            label: a.label,
            reason: a.reason.unwrap_or_default(),
            importance: a.importance.unwrap_or(3),
        })
        .collect();

    let doc_raw = std::fs::read_to_string(&doc_path).expect("read briefdoc");
    let mut doc: BriefDoc = serde_json::from_str(&doc_raw).expect("parse briefdoc.json");

    let cancel = CancellationToken::new();
    let frames_dir_pb = PathBuf::from(&frames_dir);
    println!(
        "Extracting {} anchors from {} via {}",
        anchors.len(),
        video,
        ffmpeg.display()
    );
    let mut frames = brief_frames::extract_frames(
        &ffmpeg,
        Path::new(&video),
        &anchors,
        &frames_dir_pb,
        &cancel,
    )
    .await
    .expect("extract_frames");

    // Apply captions from doc annotations when present
    for ann in &doc.screenshot_annotations {
        if let Some(frame) = frames.iter_mut().find(|f| f.index == ann.frame_index) {
            frame.caption = ann.caption.clone();
        }
    }
    // Default captions from anchor reasons
    for frame in &mut frames {
        if frame.caption.is_empty() {
            if let Some(a) = anchors.iter().find(|a| {
                (a.seconds - frame.seconds).abs() < 1.0 || a.label == frame.label
            }) {
                frame.caption = a.reason.clone();
            }
        }
    }

    if doc.has_transcript_gaps {
        println!("Note: briefdoc flags ASR gaps/hallucinations in source transcript.");
    }
    if frames.is_empty() {
        doc.no_video_warning = Some("No frames extracted.".into());
    }

    let out = PathBuf::from(&out_path);
    if let Some(parent) = out.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    brief_pdf::render_brief_pdf(&doc, &frames, &out).expect("render pdf");
    println!("Wrote {} ({} frames)", out.display(), frames.len());
    let _ = frames; // keep type linked
    let _: BriefFrame;
}

fn which_ffmpeg() -> PathBuf {
    if let Ok(p) = std::env::var("FFMPEG_PATH") {
        return PathBuf::from(p);
    }
    which::which("ffmpeg").unwrap_or_else(|_| PathBuf::from("ffmpeg"))
}

#[derive(serde::Deserialize)]
struct AnchorsFile {
    anchors: Vec<AnchorIn>,
}

#[derive(serde::Deserialize)]
struct AnchorIn {
    seconds: f64,
    label: String,
    #[serde(default)]
    reason: Option<String>,
    #[serde(default)]
    importance: Option<u8>,
}
