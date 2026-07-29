//! Optional media-info sidecar next to the transcript (`*.info.json`).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

use crate::pipeline::{run_yt_dlp_streaming, tail_stderr};
use crate::subs;

const DUMP_HEARTBEAT: Duration = Duration::from_secs(60);

/// Fields written beside each `.txt` when `keepMediaInfo` is on.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaInfoDocument {
    /// Schema marker for future readers.
    pub schema: &'static str,
    pub title: String,
    /// Original queue source: URL or local path.
    pub source: String,
    pub source_kind: String,
    /// When v2t finished writing this transcript (UTC).
    pub transcribed_at: String,
    pub transcription_date: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    pub job_id: String,
    pub track: u32,
    pub track_total: u32,
    pub transcript_txt: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transcript_vtt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local_media: Option<LocalMediaFacts>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote: Option<RemoteMediaFacts>,
    /// Short English note for humans opening the file in a text editor.
    pub notes: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalMediaFacts {
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<u64>,
    /// File modified time (UTC), when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RemoteMediaFacts {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original_title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub webpage_url: Option<String>,
    /// Uploader / channel display name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uploader: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel_url: Option<String>,
    /// yt-dlp `upload_date` as YYYY-MM-DD when parseable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upload_date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_sec: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub view_count: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub like_count: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thumbnail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fps: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ext: Option<String>,
}

#[derive(Debug, Deserialize)]
struct YtDump {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    fulltitle: Option<String>,
    #[serde(default)]
    webpage_url: Option<String>,
    #[serde(default)]
    original_url: Option<String>,
    #[serde(default)]
    channel: Option<String>,
    #[serde(default)]
    uploader: Option<String>,
    #[serde(default)]
    channel_url: Option<String>,
    #[serde(default)]
    upload_date: Option<String>,
    #[serde(default)]
    duration: Option<f64>,
    #[serde(default)]
    duration_string: Option<String>,
    #[serde(default)]
    view_count: Option<u64>,
    #[serde(default)]
    like_count: Option<u64>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    thumbnail: Option<String>,
    #[serde(default)]
    width: Option<u64>,
    #[serde(default)]
    height: Option<u64>,
    #[serde(default)]
    fps: Option<f64>,
    #[serde(default)]
    ext: Option<String>,
}

fn format_upload_date(raw: &str) -> Option<String> {
    let d = raw.trim();
    if d.len() == 8 && d.chars().all(|c| c.is_ascii_digit()) {
        return Some(format!("{}-{}-{}", &d[0..4], &d[4..6], &d[6..8]));
    }
    if !d.is_empty() {
        return Some(d.to_string());
    }
    None
}

fn truncate_desc(s: &str, max: usize) -> String {
    let t = s.trim();
    if t.chars().count() <= max {
        return t.to_string();
    }
    let mut out: String = t.chars().take(max).collect();
    out.push('…');
    out
}

impl From<YtDump> for RemoteMediaFacts {
    fn from(d: YtDump) -> Self {
        let original_title = d
            .fulltitle
            .filter(|s| !s.trim().is_empty())
            .or(d.title.filter(|s| !s.trim().is_empty()));
        let webpage_url = d
            .webpage_url
            .filter(|s| !s.trim().is_empty())
            .or(d.original_url.filter(|s| !s.trim().is_empty()));
        Self {
            id: d.id.filter(|s| !s.trim().is_empty()),
            original_title,
            webpage_url,
            channel: d.channel.filter(|s| !s.trim().is_empty()),
            uploader: d.uploader.filter(|s| !s.trim().is_empty()),
            channel_url: d.channel_url.filter(|s| !s.trim().is_empty()),
            upload_date: d.upload_date.as_deref().and_then(format_upload_date),
            duration_sec: d.duration,
            duration: d.duration_string.filter(|s| !s.trim().is_empty()),
            view_count: d.view_count,
            like_count: d.like_count,
            description: d
                .description
                .as_deref()
                .map(|s| truncate_desc(s, 2000))
                .filter(|s| !s.is_empty()),
            thumbnail: d.thumbnail.filter(|s| !s.trim().is_empty()),
            width: d.width,
            height: d.height,
            fps: d.fps,
            ext: d.ext.filter(|s| !s.trim().is_empty()),
        }
    }
}

/// Best-effort `yt-dlp --dump-json` for a single URL (not playlists).
pub async fn fetch_remote_facts(
    yt_dlp: &Path,
    url: &str,
    cookies_from_browser: Option<&str>,
    yt_dlp_js_runtimes: Option<&str>,
    cancel: &CancellationToken,
) -> Result<RemoteMediaFacts, String> {
    if subs::is_pure_playlist_url(url) {
        return Err("playlist URL — skip remote media-info dump".into());
    }
    let mut args: Vec<String> = Vec::new();
    if let Some(b) = cookies_from_browser
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        args.push("--cookies-from-browser".into());
        args.push(b.to_string());
    }
    if let Some(j) = yt_dlp_js_runtimes.map(str::trim).filter(|s| !s.is_empty()) {
        args.push("--js-runtimes".into());
        args.push(j.to_string());
    }
    args.extend([
        "--skip-download".into(),
        "--dump-json".into(),
        "--encoding".into(),
        "utf-8".into(),
        "--no-warnings".into(),
        "--no-playlist".into(),
        url.trim().to_string(),
    ]);

    let out = run_yt_dlp_streaming(
        yt_dlp,
        &args,
        DUMP_HEARTBEAT,
        cancel,
        None,
        "yt-dlp-info",
    )
    .await?;
    if !out.status.success() {
        return Err(format!(
            "yt-dlp info dump failed (exit {}): {}",
            out.status.code().unwrap_or(-1),
            tail_stderr(&out.stderr)
        ));
    }
    let json = std::str::from_utf8(&out.stdout)
        .map_err(|e| format!("yt-dlp info stdout not UTF-8: {e}"))?;
    parse_remote_facts_from_json(json)
}

/// Parse `RemoteMediaFacts` from a raw yt-dlp JSON string (output of `--dump-json`
/// or `--flat-playlist --dump-single-json` for a single-video URL).
/// The first non-empty line is used to allow multi-line yt-dlp stdout.
pub fn parse_remote_facts_from_json(json: &str) -> Result<RemoteMediaFacts, String> {
    let first = json
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .ok_or_else(|| "yt-dlp JSON is empty".to_string())?;
    let dump: YtDump =
        serde_json::from_str(first).map_err(|e| format!("parse yt-dlp info JSON: {e}"))?;
    Ok(RemoteMediaFacts::from(dump))
}

pub fn local_media_facts(path: &Path) -> LocalMediaFacts {
    let path_s = path.to_string_lossy().to_string();
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .map(str::to_string);
    let meta = fs::metadata(path).ok();
    let size_bytes = meta.as_ref().map(|m| m.len());
    let modified_at = meta
        .and_then(|m| m.modified().ok())
        .map(DateTime::<Utc>::from)
        .map(|dt| dt.to_rfc3339());
    LocalMediaFacts {
        path: path_s,
        file_name,
        size_bytes,
        modified_at,
    }
}

/// Sibling path: `Foo_2026-07-29.txt` → `Foo_2026-07-29.info.json`.
pub fn info_path_for_transcript(transcript_txt: &Path) -> PathBuf {
    transcript_txt.with_extension("info.json")
}

pub fn write_media_info(transcript_txt: &Path, doc: &MediaInfoDocument) -> Result<PathBuf, String> {
    let path = info_path_for_transcript(transcript_txt);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("create_dir_all for media info: {e}"))?;
    }
    let raw = serde_json::to_string_pretty(doc)
        .map_err(|e| format!("serialize media info: {e}"))?;
    fs::write(&path, raw).map_err(|e| format!("write media info: {e}"))?;
    Ok(path)
}

pub fn build_document(
    title: &str,
    source: &str,
    source_kind: &str,
    transcription_date: &str,
    language: Option<&str>,
    job_id: &str,
    track: u32,
    track_total: u32,
    transcript_txt: &Path,
    export_webvtt: bool,
    local_media: Option<LocalMediaFacts>,
    remote: Option<RemoteMediaFacts>,
) -> MediaInfoDocument {
    let vtt = if export_webvtt {
        Some(
            transcript_txt
                .with_extension("vtt")
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string(),
        )
        .filter(|s| !s.is_empty())
    } else {
        None
    };
    MediaInfoDocument {
        schema: "v2t.mediaInfo.v1",
        title: title.to_string(),
        source: source.to_string(),
        source_kind: source_kind.to_string(),
        transcribed_at: Utc::now().to_rfc3339(),
        transcription_date: transcription_date.to_string(),
        language: language.map(str::to_string).filter(|s| !s.trim().is_empty()),
        job_id: job_id.to_string(),
        track,
        track_total,
        transcript_txt: transcript_txt
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string(),
        transcript_vtt: vtt,
        local_media,
        remote,
        notes: "Sidecar written by Video to Text (v2t). Contains source title/URL, timestamps, and optional yt-dlp fields when available.".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn info_path_replaces_txt_extension() {
        let p = PathBuf::from(r"C:\out\Talk_2026-07-29.txt");
        assert_eq!(
            info_path_for_transcript(&p),
            PathBuf::from(r"C:\out\Talk_2026-07-29.info.json")
        );
    }

    #[test]
    fn upload_date_formats() {
        assert_eq!(
            format_upload_date("20210404").as_deref(),
            Some("2021-04-04")
        );
    }

    #[test]
    fn write_roundtrip() {
        let dir = std::env::temp_dir().join(format!(
            "v2t-media-info-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let txt = dir.join("sample_2026-07-29.txt");
        fs::write(&txt, "hello").unwrap();
        let doc = build_document(
            "Sample",
            "https://example.com/v",
            "url",
            "2026-07-29",
            Some("ru"),
            "job-1",
            1,
            1,
            &txt,
            true,
            None,
            Some(RemoteMediaFacts {
                original_title: Some("Original".into()),
                webpage_url: Some("https://example.com/v".into()),
                upload_date: Some("2021-04-04".into()),
                ..Default::default()
            }),
        );
        let written = write_media_info(&txt, &doc).unwrap();
        assert!(written.ends_with("sample_2026-07-29.info.json"));
        let raw = fs::read_to_string(&written).unwrap();
        assert!(raw.contains("v2t.mediaInfo.v1"));
        assert!(raw.contains("Original"));
        assert!(raw.contains("sample_2026-07-29.vtt"));
        let _ = fs::remove_dir_all(&dir);
    }
}
