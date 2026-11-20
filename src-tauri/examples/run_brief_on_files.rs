//! Run Brief on a local video + transcript (no Tauri window).
//!
//! ```text
//! cargo run --example run_brief_on_files --release -- \
//!   --video path.mp4 --transcript path.txt [--vtt path.vtt] --out-dir dir
//! ```

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio_util::sync::CancellationToken;
use v2t_lib::brief_pipeline;
use v2t_lib::gemini_key_store;
use v2t_lib::progress::{JobEvent, ProgressSink, SinkHandle};
use v2t_lib::settings::AppSettings;

struct PrintSink;
impl ProgressSink for PrintSink {
    fn emit(&self, event: JobEvent) {
        if let JobEvent::QueueJobProgress(p) = event {
            eprintln!("[{}] {}", p.phase, p.message);
        }
    }
}

fn arg_value(args: &[String], name: &str) -> Option<String> {
    args.windows(2).find(|w| w[0] == name).map(|w| w[1].clone())
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let video = arg_value(&args, "--video").expect("--video required");
    let transcript = arg_value(&args, "--transcript").expect("--transcript required");
    let vtt = arg_value(&args, "--vtt");
    let out_dir = arg_value(&args, "--out-dir").expect("--out-dir required");

    let out = PathBuf::from(&out_dir);
    std::fs::create_dir_all(&out).expect("create out-dir");

    let txt_name = Path::new(&transcript)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("transcript.txt");
    let work_txt = out.join(txt_name);
    std::fs::copy(&transcript, &work_txt).expect("copy transcript");

    let work_vtt = if let Some(v) = vtt.as_ref() {
        let name = Path::new(v)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("transcript.vtt");
        let dest = out.join(name);
        std::fs::copy(v, &dest).expect("copy vtt");
        Some(dest)
    } else {
        None
    };

    let api_key = gemini_key_store::get()
        .ok()
        .flatten()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| std::env::var("V2T_GAK").ok())
        .expect("Gemini API key missing (keyring or V2T_GAK)");

    let mut settings = AppSettings::default();
    settings.brief_doc_mode = match std::env::var("V2T_BRIEF_MODE")
        .unwrap_or_else(|_| "fdd".into())
        .to_ascii_lowercase()
        .as_str()
    {
        "brief" => v2t_lib::settings::BriefDocMode::Brief,
        _ => v2t_lib::settings::BriefDocMode::Fdd,
    };
    settings.brief_model = std::env::var("V2T_BRIEF_MODEL")
        .unwrap_or_else(|_| "gemini-3.1-flash-lite".into());
    settings.gemini_api_key = api_key;
    settings.ffmpeg_path = std::env::var("FFMPEG_PATH")
        .ok()
        .or_else(|| which::which("ffmpeg").ok().map(|p| p.to_string_lossy().into_owned()));
    if settings.ffmpeg_path.is_none() {
        eprintln!("warning: ffmpeg not on PATH; set FFMPEG_PATH");
    }

    let sink: SinkHandle = Arc::new(PrintSink);
    let cancel = CancellationToken::new();

    eprintln!("Running Brief on {} …", video);
    let pdf = brief_pipeline::run_brief_pipeline(
        &sink,
        "compare-brief",
        &work_txt,
        work_vtt.as_deref(),
        Some(Path::new(&video)),
        &settings,
        &cancel,
    )
    .await
    .expect("brief pipeline");

    println!("{}", pdf.display());
}
