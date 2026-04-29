//! YouTube subtitle fast-path.
//!
//! When a video has *manual* subtitles in a priority language, fetch them via
//! yt-dlp and convert to plain text — this skips the download + Whisper passes
//! entirely (seconds vs. minutes for an hour-long lecture). Auto-generated
//! captions (`automatic_captions`) are intentionally never used: on UA / RU
//! they consistently rank below Whisper-medium.
//!
//! ## Flow
//! 1. `probe_subs(url)` runs `yt-dlp --skip-download --dump-json` for one
//!    network round-trip and returns the per-video metadata (id, title, manual
//!    + auto subtitle maps).
//! 2. `pick_priority_lang(prefs, &probe)` walks the user's priority list and
//!    returns the first matching key from `probe.subtitles`. We only consult
//!    *manual* subs — see acceptance criterion in `docs/WAVES.md` Wave 5.
//! 3. `download_srt(url, lang, work_dir)` runs `yt-dlp --write-subs --sub-langs
//!    <lang> --skip-download --convert-subs srt --no-write-auto-subs` into a
//!    fresh temp dir and returns the path of the produced `.srt` file.
//! 4. `srt_to_plain_text(srt)` strips block indices, timing arrows, and inline
//!    SSA / HTML tags, returning prose suitable for the `.txt` transcript.
//!
//! Best-effort everywhere: any failure (network, missing manual subs, broken
//! SRT) returns `Err(...)` so the caller can fall back to the regular pipeline.

use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

use crate::pipeline::{run_yt_dlp_streaming, tail_stderr};

/// `--dump-json --skip-download` is one round-trip; 60 s is generous on flaky links.
const PROBE_HEARTBEAT: Duration = Duration::from_secs(60);
/// Subtitle download is small (tens of KiB), but proxies / CDNs occasionally stall.
const FETCH_HEARTBEAT: Duration = Duration::from_secs(120);

/// Subset of yt-dlp's `--dump-json` output that we care about. The full record
/// is large; serde silently drops unknown fields.
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct SubsProbe {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    /// Manual subtitle tracks keyed by language code (e.g. `"uk"`, `"en-US"`).
    /// Each value is a list of available formats — we only check non-emptiness.
    #[serde(default)]
    pub subtitles: HashMap<String, Vec<serde_json::Value>>,
    /// YouTube ASR captions — populated but **not** consulted by the fast-path.
    #[serde(default)]
    pub automatic_captions: HashMap<String, Vec<serde_json::Value>>,
}

pub fn parse_probe(json: &str) -> Result<SubsProbe, String> {
    serde_json::from_str(json).map_err(|e| format!("Failed to parse yt-dlp JSON: {e}"))
}

/// True for `youtube.com/playlist?list=…` and the bare-list YT-Music variant.
/// `watch?v=…&list=…` returns false because we treat that as a single video
/// (yt-dlp `--no-playlist` already strips the playlist context). Used to gate
/// the fast-path: probing one URL for a multi-entry playlist would only return
/// the first entry's subtitles, leading to silently wrong transcripts for the
/// rest.
pub fn is_pure_playlist_url(url: &str) -> bool {
    let lower = url.trim().to_lowercase();
    lower.contains("youtube.com/playlist")
        || lower.contains("/feed/playlists")
        || lower.contains("music.youtube.com/playlist")
}

/// Walk `priorities` in order; for each, return the first matching key from
/// `probe.subtitles`. Both exact (`"en"` == `"en"`) and prefix (`"en"` matches
/// `"en-US"`) matches count, since YouTube emits both shapes.
pub fn pick_priority_lang(priorities: &[String], probe: &SubsProbe) -> Option<String> {
    for code in priorities {
        let key = code.trim().to_lowercase();
        if key.is_empty() {
            continue;
        }
        // Exact match first — preferred over a regional variant.
        if probe
            .subtitles
            .get(&key)
            .filter(|v| !v.is_empty())
            .is_some()
        {
            return Some(key);
        }
        let prefix = format!("{key}-");
        if let Some((k, _)) = probe
            .subtitles
            .iter()
            .find(|(k, v)| !v.is_empty() && k.to_lowercase().starts_with(&prefix))
        {
            return Some(k.clone());
        }
    }
    None
}

/// `yt-dlp --skip-download --dump-json --no-warnings -- <url>` — single line of JSON.
/// Adds `--no-playlist` for `watch?v=…&list=…` shaped URLs so we get one record.
pub async fn probe_subs(
    yt_dlp: &Path,
    url: &str,
    cookies_from_browser: Option<&str>,
    yt_dlp_js_runtimes: Option<&str>,
    cancel: &CancellationToken,
) -> Result<SubsProbe, String> {
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
    if crate::pipeline::youtube_watch_url_should_use_no_playlist(url) {
        args.push("--no-playlist".into());
    }
    args.extend([
        "--skip-download".into(),
        "--dump-json".into(),
        "--encoding".into(),
        "utf-8".into(),
        "--no-warnings".into(),
        url.trim().to_string(),
    ]);

    let out =
        run_yt_dlp_streaming(yt_dlp, &args, PROBE_HEARTBEAT, cancel, None, "yt-dlp-subs").await?;
    if !out.status.success() {
        return Err(format!(
            "yt-dlp probe failed (exit {}): {}",
            out.status.code().unwrap_or(-1),
            tail_stderr(&out.stderr)
        ));
    }

    let json = std::str::from_utf8(&out.stdout)
        .map_err(|e| format!("yt-dlp probe stdout not UTF-8: {e}"))?;
    // For single-video URLs `--dump-json` emits one JSON object on one line.
    // For playlists it emits NDJSON; pick the first parseable record (callers
    // reject playlist URLs upstream, so this is just defensive).
    let first = json
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .ok_or_else(|| "yt-dlp probe produced no JSON".to_string())?;
    parse_probe(first)
}

/// Run `yt-dlp --write-subs --sub-langs <lang> --skip-download --convert-subs srt
/// --no-write-auto-subs -o <work_dir>/v2t-subs.%(ext)s -- <url>` and return the
/// resulting `.srt` path. yt-dlp names the file with the language suffix
/// (`v2t-subs.uk.srt`); we glob the work dir for any `.srt` to be tolerant.
pub async fn download_srt(
    yt_dlp: &Path,
    url: &str,
    lang: &str,
    work_dir: &Path,
    cookies_from_browser: Option<&str>,
    yt_dlp_js_runtimes: Option<&str>,
    cancel: &CancellationToken,
) -> Result<PathBuf, String> {
    std::fs::create_dir_all(work_dir).map_err(|e| format!("create subs work dir: {e}"))?;

    let template_path = work_dir.join("v2t-subs.%(ext)s");
    let template = template_path
        .to_str()
        .ok_or("Subs work path is not valid UTF-8")?
        .replace('\\', "/");

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
    if crate::pipeline::youtube_watch_url_should_use_no_playlist(url) {
        args.push("--no-playlist".into());
    }
    args.extend([
        "--skip-download".into(),
        "--write-subs".into(),
        "--no-write-auto-subs".into(),
        "--sub-langs".into(),
        lang.to_string(),
        "--convert-subs".into(),
        "srt".into(),
        "--encoding".into(),
        "utf-8".into(),
        "--no-warnings".into(),
        "-o".into(),
        template,
        url.trim().to_string(),
    ]);

    let out =
        run_yt_dlp_streaming(yt_dlp, &args, FETCH_HEARTBEAT, cancel, None, "yt-dlp-subs").await?;
    if !out.status.success() {
        return Err(format!(
            "yt-dlp sub fetch failed (exit {}): {}",
            out.status.code().unwrap_or(-1),
            tail_stderr(&out.stderr)
        ));
    }

    let mut srts: Vec<PathBuf> = std::fs::read_dir(work_dir)
        .map_err(|e| format!("read subs work dir: {e}"))?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.is_file()
                && p.extension()
                    .and_then(|e| e.to_str())
                    .map(|e| e.eq_ignore_ascii_case("srt"))
                    .unwrap_or(false)
        })
        .collect();
    srts.sort();
    srts.into_iter()
        .next()
        .ok_or_else(|| "yt-dlp produced no .srt file (subtitle may have been removed)".to_string())
}

/// Strip SRT block indices, `00:00:00,000 --> 00:00:00,000` cue lines, inline
/// SSA / HTML tags (`<i>`, `{\an8}`), and collapse blanks. Lines from one cue
/// are joined with a single space; cues are separated by a newline so the
/// reader still sees natural paragraph breaks.
pub fn srt_to_plain_text(srt: &str) -> String {
    let mut out = String::new();
    for block in srt.split("\n\n").flat_map(|s| s.split("\r\n\r\n")) {
        let mut buf: Vec<String> = Vec::new();
        for raw in block.lines() {
            let line = raw.trim();
            if line.is_empty() {
                continue;
            }
            if line.chars().all(|c| c.is_ascii_digit()) {
                continue;
            }
            if is_timing_line(line) {
                continue;
            }
            let cleaned = clean_inline_tags(line);
            let trimmed = cleaned.trim();
            if !trimmed.is_empty() {
                buf.push(trimmed.to_string());
            }
        }
        if !buf.is_empty() {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(&buf.join(" "));
        }
    }
    out
}

fn is_timing_line(line: &str) -> bool {
    // `00:00:01,000 --> 00:00:04,000` (with optional `,123` or `.123` separator).
    line.contains("-->")
        && line
            .split("-->")
            .all(|side| side.trim().chars().any(|c| c == ':'))
}

/// Drop `<…>` HTML/SSA inline tags (`<i>`, `<b>`, `<font color=...>`) and `{\an8}` SSA blocks.
fn clean_inline_tags(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut in_angle = false;
    let mut in_brace = false;
    for c in line.chars() {
        match c {
            '<' if !in_brace => in_angle = true,
            '>' if in_angle => in_angle = false,
            '{' if !in_angle => in_brace = true,
            '}' if in_brace => in_brace = false,
            _ if !in_angle && !in_brace => out.push(c),
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn probe_with_subs(subs: &[(&str, bool)]) -> SubsProbe {
        let mut map: HashMap<String, Vec<serde_json::Value>> = HashMap::new();
        for (k, has) in subs {
            map.insert(
                (*k).into(),
                if *has {
                    vec![serde_json::json!({"ext":"vtt"})]
                } else {
                    Vec::new()
                },
            );
        }
        SubsProbe {
            id: None,
            title: None,
            subtitles: map,
            automatic_captions: HashMap::new(),
        }
    }

    #[test]
    fn parse_probe_extracts_subtitles_map() {
        let json = r#"{
            "id":"abc","title":"X",
            "subtitles":{"uk":[{"ext":"vtt"}],"en":[{"ext":"srv1"}]},
            "automatic_captions":{"en":[{"ext":"vtt"}]}
        }"#;
        let probe = parse_probe(json).unwrap();
        assert_eq!(probe.id.as_deref(), Some("abc"));
        assert!(probe.subtitles.contains_key("uk"));
        assert!(probe.subtitles.contains_key("en"));
        assert!(probe.automatic_captions.contains_key("en"));
    }

    #[test]
    fn parse_probe_tolerates_missing_fields() {
        let json = r#"{"id":"q"}"#;
        let probe = parse_probe(json).unwrap();
        assert!(probe.subtitles.is_empty());
        assert!(probe.automatic_captions.is_empty());
    }

    #[test]
    fn pick_lang_returns_first_priority_match() {
        let probe = probe_with_subs(&[("ru", true), ("en", true)]);
        let prefs = vec!["uk".into(), "ru".into(), "en".into()];
        assert_eq!(pick_priority_lang(&prefs, &probe).as_deref(), Some("ru"));
    }

    #[test]
    fn pick_lang_skips_empty_track_lists() {
        let probe = probe_with_subs(&[("uk", false), ("en", true)]);
        let prefs = vec!["uk".into(), "en".into()];
        assert_eq!(pick_priority_lang(&prefs, &probe).as_deref(), Some("en"));
    }

    #[test]
    fn pick_lang_matches_regional_prefix() {
        let probe = probe_with_subs(&[("en-US", true)]);
        let prefs = vec!["en".into()];
        assert_eq!(pick_priority_lang(&prefs, &probe).as_deref(), Some("en-US"));
    }

    #[test]
    fn pick_lang_prefers_exact_over_regional() {
        let probe = probe_with_subs(&[("en", true), ("en-US", true)]);
        let prefs = vec!["en".into()];
        assert_eq!(pick_priority_lang(&prefs, &probe).as_deref(), Some("en"));
    }

    #[test]
    fn pick_lang_none_when_no_manual_subs() {
        let probe = probe_with_subs(&[("uk", false)]);
        let prefs = vec!["uk".into(), "en".into()];
        assert!(pick_priority_lang(&prefs, &probe).is_none());
    }

    #[test]
    fn pick_lang_ignores_auto_captions() {
        let mut probe = probe_with_subs(&[]);
        probe
            .automatic_captions
            .insert("en".into(), vec![serde_json::json!({"ext":"vtt"})]);
        let prefs = vec!["en".into()];
        assert!(pick_priority_lang(&prefs, &probe).is_none());
    }

    #[test]
    fn srt_to_plain_basic() {
        let srt = "1\n00:00:01,000 --> 00:00:04,000\nHello world\n\n2\n00:00:05,000 --> 00:00:08,000\nSecond line\n";
        let plain = srt_to_plain_text(srt);
        assert_eq!(plain, "Hello world\nSecond line");
    }

    #[test]
    fn srt_to_plain_strips_inline_tags() {
        let srt = "1\n00:00:01,000 --> 00:00:04,000\n<i>Italic</i> and {\\an8}top text\n";
        let plain = srt_to_plain_text(srt);
        assert_eq!(plain, "Italic and top text");
    }

    #[test]
    fn srt_to_plain_joins_multi_line_cue_with_space() {
        let srt = "1\n00:00:01,000 --> 00:00:04,000\nFirst line\nsecond line\n";
        let plain = srt_to_plain_text(srt);
        assert_eq!(plain, "First line second line");
    }

    #[test]
    fn srt_to_plain_handles_crlf() {
        let srt = "1\r\n00:00:01,000 --> 00:00:04,000\r\nLine A\r\n\r\n2\r\n00:00:05,000 --> 00:00:08,000\r\nLine B\r\n";
        let plain = srt_to_plain_text(srt);
        assert_eq!(plain, "Line A\nLine B");
    }

    #[test]
    fn srt_to_plain_unicode_cyrillic() {
        let srt = "1\n00:00:01,000 --> 00:00:04,000\nПривіт, світе!\n\n2\n00:00:05,000 --> 00:00:08,000\nДруга строчка.\n";
        let plain = srt_to_plain_text(srt);
        assert_eq!(plain, "Привіт, світе!\nДруга строчка.");
    }

    #[test]
    fn is_pure_playlist_url_detects_only_playlist_pages() {
        assert!(is_pure_playlist_url(
            "https://www.youtube.com/playlist?list=PL123"
        ));
        assert!(is_pure_playlist_url(
            "https://music.youtube.com/playlist?list=OLAK"
        ));
        assert!(!is_pure_playlist_url(
            "https://www.youtube.com/watch?v=abc&list=PL123"
        ));
        assert!(!is_pure_playlist_url("https://www.youtube.com/watch?v=abc"));
        assert!(!is_pure_playlist_url("https://example.com/x"));
    }

    #[test]
    fn srt_to_plain_ignores_empty_cues() {
        let srt = "1\n00:00:01,000 --> 00:00:04,000\n\n\n2\n00:00:05,000 --> 00:00:08,000\nReal text\n";
        let plain = srt_to_plain_text(srt);
        assert_eq!(plain, "Real text");
    }
}
