//! Pre-resolve playlist / video metadata via `yt-dlp --flat-playlist --dump-single-json`.
//!
//! Best-effort: a network failure, a private playlist, a single-video URL, or even
//! a yt-dlp version mismatch must NOT block the main download pipeline. Callers
//! treat `Err(...)` as "no subtask list" and proceed with the regular pipeline.
//!
//! Output is always a single JSON object on stdout. yt-dlp uses two shapes:
//! - **Playlist** (`_type: "playlist"`): top-level `title` + `entries` array.
//! - **Single video** (`_type: "video"` or no `_type`): top-level `id` + `title`,
//!   no `entries`. We map this to an empty subtask list — there is nothing for
//!   the per-video UI to render.
//!
//! `playlist_index` is yt-dlp's own 1-based ordinal; we fall back to the array
//! position when missing. `url` is what yt-dlp produced for the entry; if absent
//! we synthesize the canonical YouTube watch URL from the entry id.
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

use crate::pipeline::{run_yt_dlp_streaming, tail_stderr};

/// `--dump-single-json` only does one network round-trip; 60 s is generous.
const META_HEARTBEAT: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Deserialize)]
pub struct PlaylistInfo {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub entries: Option<Vec<PlaylistEntry>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PlaylistEntry {
    pub id: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub playlist_index: Option<u32>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedSubtask {
    pub id: String,
    pub index: u32,
    pub title: String,
    pub original_url: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistResolvedPayload {
    pub job_id: String,
    pub playlist_title: Option<String>,
    pub subtasks: Vec<ResolvedSubtask>,
}

/// Run `yt-dlp --flat-playlist --dump-single-json --encoding utf-8 -- <url>` and
/// return the parsed JSON. Stdout carries the JSON; stderr is only used for an
/// error tail on non-zero exit. Heartbeat 60 s — single one-shot operation.
pub async fn resolve_playlist_metadata(
    yt_dlp: &Path,
    url: &str,
    cookies_from_browser: Option<&str>,
    yt_dlp_js_runtimes: Option<&str>,
    cancel: &CancellationToken,
) -> Result<PlaylistInfo, String> {
    let mut args: Vec<String> = Vec::new();
    if let Some(b) = cookies_from_browser
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        args.push("--cookies-from-browser".into());
        args.push(b.to_string());
    }
    if let Some(j) = yt_dlp_js_runtimes
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        args.push("--js-runtimes".into());
        args.push(j.to_string());
    }
    args.extend([
        "--flat-playlist".into(),
        "--dump-single-json".into(),
        "--encoding".into(),
        "utf-8".into(),
        "--no-warnings".into(),
        url.trim().to_string(),
    ]);

    let out = run_yt_dlp_streaming(
        yt_dlp,
        &args,
        META_HEARTBEAT,
        cancel,
        None,
        "yt-dlp-meta",
    )
    .await?;

    if !out.status.success() {
        return Err(format!(
            "yt-dlp metadata failed (exit {}): {}",
            out.status.code().unwrap_or(-1),
            tail_stderr(&out.stderr)
        ));
    }

    let json = std::str::from_utf8(&out.stdout)
        .map_err(|e| format!("yt-dlp metadata not UTF-8: {e}"))?;
    parse_playlist_info(json)
}

pub fn parse_playlist_info(json: &str) -> Result<PlaylistInfo, String> {
    serde_json::from_str(json).map_err(|e| format!("Failed to parse yt-dlp JSON: {e}"))
}

/// Map raw `entries` to a UI-shaped subtask list. Missing fields fall back to
/// stable defaults (id as title, position as index, watch?v={id} as url).
/// Returns an empty Vec when there are no entries (single-video URL case).
pub fn entries_to_subtasks(info: &PlaylistInfo) -> Vec<ResolvedSubtask> {
    let Some(entries) = info.entries.as_ref() else {
        return Vec::new();
    };
    entries
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let index = e.playlist_index.unwrap_or((i + 1) as u32);
            let title = e
                .title
                .clone()
                .filter(|t| !t.trim().is_empty())
                .unwrap_or_else(|| e.id.clone());
            let original_url = e
                .url
                .clone()
                .filter(|u| !u.trim().is_empty())
                .unwrap_or_else(|| format!("https://www.youtube.com/watch?v={}", e.id));
            ResolvedSubtask {
                id: e.id.clone(),
                index,
                title,
                original_url,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_playlist_with_entries() {
        let json = r#"{
            "_type": "playlist",
            "title": "Lessons UA",
            "entries": [
                {"id":"abc","title":"Lesson 1","url":"https://www.youtube.com/watch?v=abc","playlist_index":1},
                {"id":"def","title":"Lesson 2","url":"https://www.youtube.com/watch?v=def","playlist_index":2}
            ]
        }"#;
        let info = parse_playlist_info(json).unwrap();
        assert_eq!(info.title.as_deref(), Some("Lessons UA"));
        let subs = entries_to_subtasks(&info);
        assert_eq!(subs.len(), 2);
        assert_eq!(subs[0].id, "abc");
        assert_eq!(subs[0].index, 1);
        assert_eq!(subs[0].title, "Lesson 1");
        assert_eq!(subs[0].original_url, "https://www.youtube.com/watch?v=abc");
        assert_eq!(subs[1].index, 2);
    }

    #[test]
    fn single_video_has_no_entries() {
        let json = r#"{"_type":"video","id":"abc","title":"Solo"}"#;
        let info = parse_playlist_info(json).unwrap();
        let subs = entries_to_subtasks(&info);
        assert!(subs.is_empty());
        assert_eq!(info.title.as_deref(), Some("Solo"));
    }

    #[test]
    fn missing_url_falls_back_to_watch_link() {
        let json = r#"{
            "_type": "playlist",
            "title": "no-urls",
            "entries": [
                {"id":"xyz","title":"x"}
            ]
        }"#;
        let info = parse_playlist_info(json).unwrap();
        let subs = entries_to_subtasks(&info);
        assert_eq!(subs.len(), 1);
        assert_eq!(subs[0].original_url, "https://www.youtube.com/watch?v=xyz");
    }

    #[test]
    fn missing_title_falls_back_to_id() {
        let json = r#"{
            "_type": "playlist",
            "entries": [
                {"id":"qqq","url":"https://www.youtube.com/watch?v=qqq"}
            ]
        }"#;
        let info = parse_playlist_info(json).unwrap();
        let subs = entries_to_subtasks(&info);
        assert_eq!(subs.len(), 1);
        assert_eq!(subs[0].title, "qqq");
        assert_eq!(subs[0].id, "qqq");
    }

    #[test]
    fn missing_playlist_index_uses_position() {
        let json = r#"{
            "_type": "playlist",
            "entries": [
                {"id":"a"},
                {"id":"b"},
                {"id":"c"}
            ]
        }"#;
        let info = parse_playlist_info(json).unwrap();
        let subs = entries_to_subtasks(&info);
        assert_eq!(subs.len(), 3);
        assert_eq!(subs[0].index, 1);
        assert_eq!(subs[1].index, 2);
        assert_eq!(subs[2].index, 3);
    }

    #[test]
    fn invalid_json_returns_err() {
        let r = parse_playlist_info("not json");
        assert!(r.is_err());
    }
}
