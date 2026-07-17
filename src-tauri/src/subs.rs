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

/// True when the URL should download/transcribe a **multi-video playlist**, not a single item.
/// Includes `playlist?list=…` pages and browser-style `watch?v=…&list=…` links.
/// Used to gate the subtitle fast-path (probing one URL would only return the first entry).
pub fn is_pure_playlist_url(url: &str) -> bool {
    let lower = url.trim().to_lowercase();
    if lower.contains("youtube.com/playlist")
        || lower.contains("/feed/playlists")
        || lower.contains("music.youtube.com/playlist")
    {
        return true;
    }
    let on_youtube = lower.contains("youtube.com")
        || lower.contains("youtu.be")
        || lower.contains("youtube-nocookie.com")
        || lower.contains("music.youtube.com");
    if !on_youtube || !lower.contains("list=") {
        return false;
    }
    lower.contains("watch?")
        || lower.contains("youtu.be/")
        || lower.contains("/shorts/")
        || lower.contains("/live/")
        || lower.contains("/embed/")
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
    if let Some(j) = yt_dlp_js_runtimes.map(str::trim).filter(|s| !s.is_empty()) {
        args.push("--js-runtimes".into());
        args.push(j.to_string());
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
///
/// Thin wrapper over [`srt_to_timed_transcript`] so existing callers and tests
/// keep a plain-text-only entry point.
#[allow(dead_code)] // public crate API retained; production path uses timed entry points
pub fn srt_to_plain_text(srt: &str) -> String {
    srt_to_timed_transcript(srt, false)
        .expect("lenient plain path never fails")
        .text
}

/// Convert SRT into a timed transcript.
///
/// When `export_webvtt` is false, parsing is lenient (legacy): cues without
/// usable timings still contribute plain text, and incomplete timing is skipped.
/// When `export_webvtt` is true, every nonempty cue must have valid `end > start`
/// timing or the call returns `Err` (caller may fall back to Whisper).
///
/// Timed cues store speaker separately via [`extract_voice_speaker`];
/// `format_webvtt` re-wraps voice tags. The plain `text` field always strips
/// angle/SSA tags.
pub fn srt_to_timed_transcript(
    srt: &str,
    export_webvtt: bool,
) -> Result<crate::timed_transcript::TimedTranscript, String> {
    use crate::timed_transcript::{extract_voice_speaker, TimedSegment, TimedTranscript};

    let mut plain_parts: Vec<String> = Vec::new();
    let mut segments: Vec<TimedSegment> = Vec::new();

    for block in srt.split("\n\n").flat_map(|s| s.split("\r\n\r\n")) {
        let mut timing: Option<(u64, u64)> = None;
        let mut saw_timing_line = false;
        let mut plain_buf: Vec<String> = Vec::new();
        let mut timed_buf: Vec<String> = Vec::new();
        for raw in block.lines() {
            let line = raw.trim();
            if line.is_empty() {
                continue;
            }
            if line.chars().all(|c| c.is_ascii_digit()) {
                continue;
            }
            if is_timing_line(line) {
                saw_timing_line = true;
                timing = parse_srt_timing_line(line).filter(|(start, end)| *end > *start);
                continue;
            }
            let plain_cleaned = clean_inline_tags(line);
            let plain_trimmed = plain_cleaned.trim();
            if !plain_trimmed.is_empty() {
                plain_buf.push(plain_trimmed.to_string());
            }
            let timed_cleaned = clean_ssa_keep_angle_tags(line);
            let timed_trimmed = timed_cleaned.trim();
            if !timed_trimmed.is_empty() {
                timed_buf.push(timed_trimmed.to_string());
            }
        }
        if plain_buf.is_empty() && timed_buf.is_empty() {
            continue;
        }
        let cue_plain = plain_buf.join(" ");
        let cue_timed = if timed_buf.is_empty() {
            cue_plain.clone()
        } else {
            timed_buf.join(" ")
        };
        if !cue_plain.is_empty() {
            plain_parts.push(cue_plain);
        } else if !cue_timed.is_empty() {
            // Angle-only cue still counts as nonempty for export validation.
            plain_parts.push(clean_inline_tags(&cue_timed).trim().to_string());
        }

        if export_webvtt {
            if !saw_timing_line || timing.is_none() {
                return Err(
                    "Malformed subtitle timing: nonempty cue lacks valid end>start timestamps"
                        .to_string(),
                );
            }
            let (start_ms, end_ms) = timing.expect("checked above");
            let segment_text = if cue_timed.is_empty() {
                plain_parts.last().cloned().unwrap_or_default()
            } else {
                cue_timed
            };
            if segment_text.trim().is_empty() {
                return Err("Malformed subtitle cue: empty text after cleanup".to_string());
            }
            let (speaker, body) = extract_voice_speaker(&segment_text);
            let body = body.trim().to_string();
            if body.is_empty() {
                return Err("Malformed subtitle cue: empty text after cleanup".to_string());
            }
            segments.push(TimedSegment {
                start_ms,
                end_ms,
                text: body,
                speaker,
            });
        } else if let Some((start_ms, end_ms)) = timing {
            let segment_text = if cue_timed.is_empty() {
                plain_parts.last().cloned().unwrap_or_default()
            } else {
                cue_timed
            };
            if !segment_text.trim().is_empty() {
                let (speaker, body) = extract_voice_speaker(&segment_text);
                let body = body.trim().to_string();
                if !body.is_empty() {
                    segments.push(TimedSegment {
                        start_ms,
                        end_ms,
                        text: body,
                        speaker,
                    });
                }
            }
        }
    }

    let text = plain_parts.join("\n");
    if export_webvtt && !text.trim().is_empty() && segments.is_empty() {
        return Err("Malformed subtitles: nonempty transcript has no valid timed cues".to_string());
    }

    Ok(TimedTranscript { text, segments })
}

fn parse_srt_timing_line(line: &str) -> Option<(u64, u64)> {
    let (left, right) = line.split_once("-->")?;
    let start_ms = parse_srt_clock(left.trim())?;
    let end_side = right.trim().split_whitespace().next().unwrap_or("");
    let end_ms = parse_srt_clock(end_side)?;
    Some((start_ms, end_ms))
}

fn parse_srt_clock(value: &str) -> Option<u64> {
    let normalized = value.replace(',', ".");
    let parts: Vec<&str> = normalized.split(':').collect();
    let (hours, minutes, seconds_part) = match parts.as_slice() {
        [h, m, s] => (h.parse::<u64>().ok()?, m.parse::<u64>().ok()?, *s),
        [m, s] => (0, m.parse::<u64>().ok()?, *s),
        _ => return None,
    };
    let (secs, millis) = if let Some((s, ms)) = seconds_part.split_once('.') {
        let millis = match ms.len() {
            0 => 0,
            1 => ms.parse::<u64>().ok()? * 100,
            2 => ms.parse::<u64>().ok()? * 10,
            _ => ms.get(..3)?.parse::<u64>().ok()?,
        };
        (s.parse::<u64>().ok()?, millis)
    } else {
        (seconds_part.parse::<u64>().ok()?, 0)
    };
    Some(hours * 3_600_000 + minutes * 60_000 + secs * 1_000 + millis)
}

fn is_timing_line(line: &str) -> bool {
    // `00:00:01,000 --> 00:00:04,000` (with optional `,123` or `.123` separator).
    line.contains("-->")
        && line
            .split("-->")
            .all(|side| side.trim().chars().any(|c| c == ':'))
}

/// Drop `<…>` HTML/SSA inline tags (`<i>`, `<b>`, `<font color=...>`) and `{\an8}` SSA blocks.
/// Speaker-like prefixes such as `[SPEAKER]:` are preserved; angle-bracket tags are removed
/// to match the historical plain-text behaviour.
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

/// Strip SSA `{\an8}` blocks but keep angle-bracket markup for timed cue payloads
/// (WebVTT escaping decides which tags survive).
fn clean_ssa_keep_angle_tags(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut in_brace = false;
    for c in line.chars() {
        match c {
            '{' => in_brace = true,
            '}' if in_brace => in_brace = false,
            _ if !in_brace => out.push(c),
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
    fn is_pure_playlist_url_detects_playlist_pages_and_watch_list_links() {
        assert!(is_pure_playlist_url(
            "https://www.youtube.com/playlist?list=PL123"
        ));
        assert!(is_pure_playlist_url(
            "https://music.youtube.com/playlist?list=OLAK"
        ));
        assert!(is_pure_playlist_url(
            "https://www.youtube.com/watch?v=abc&list=PL123"
        ));
        assert!(is_pure_playlist_url("https://youtu.be/abc?list=PL123"));
        assert!(!is_pure_playlist_url("https://www.youtube.com/watch?v=abc"));
        assert!(!is_pure_playlist_url("https://example.com/x"));
    }

    #[test]
    fn srt_to_plain_ignores_empty_cues() {
        let srt =
            "1\n00:00:01,000 --> 00:00:04,000\n\n\n2\n00:00:05,000 --> 00:00:08,000\nReal text\n";
        let plain = srt_to_plain_text(srt);
        assert_eq!(plain, "Real text");
    }

    #[test]
    fn srt_to_timed_preserves_plain_and_ms_cues() {
        let srt = "1\n00:00:01,000 --> 00:00:04,250\nHello world\n\n2\n00:00:05,000 --> 00:00:08,000\n[SPEAKER]: Second line\n";
        let timed = srt_to_timed_transcript(srt, false).unwrap();
        assert_eq!(timed.text, srt_to_plain_text(srt));
        assert_eq!(timed.segments.len(), 2);
        assert_eq!(timed.segments[0].start_ms, 1_000);
        assert_eq!(timed.segments[0].end_ms, 4_250);
        assert_eq!(timed.segments[0].text, "Hello world");
        assert_eq!(timed.segments[1].text, "[SPEAKER]: Second line");
        let vtt = crate::timed_transcript::format_webvtt(&timed.segments).unwrap();
        assert!(vtt.contains("00:00:01.000 --> 00:00:04.250"));
        assert!(vtt.contains("[SPEAKER]: Second line"));
    }

    #[test]
    fn srt_to_timed_handles_dot_separator() {
        let srt = "1\n00:00:00.500 --> 00:00:01.000\nDot times\n";
        let timed = srt_to_timed_transcript(srt, false).unwrap();
        assert_eq!(timed.segments[0].start_ms, 500);
        assert_eq!(timed.segments[0].end_ms, 1_000);
    }

    #[test]
    fn srt_export_strict_rejects_missing_timing_on_nonempty_cue() {
        let srt = "1\nHello without timing\n\n2\n00:00:01,000 --> 00:00:02,000\nOk\n";
        let err = srt_to_timed_transcript(srt, true).unwrap_err();
        assert!(err.contains("Malformed"));
        // Lenient path still succeeds with partial segments.
        let lenient = srt_to_timed_transcript(srt, false).unwrap();
        assert!(lenient.text.contains("Hello without timing"));
        assert_eq!(lenient.segments.len(), 1);
    }

    #[test]
    fn srt_timed_extracts_voice_speaker_plain_strips() {
        let srt = "1\n00:00:01,000 --> 00:00:02,000\n<v Alice>Hello</v> world\n";
        let timed = srt_to_timed_transcript(srt, true).unwrap();
        assert_eq!(timed.text, "Hello world");
        assert_eq!(timed.segments[0].speaker.as_deref(), Some("Alice"));
        assert_eq!(timed.segments[0].text, "Hello world");
        let vtt = crate::timed_transcript::format_webvtt(&timed.segments).unwrap();
        assert!(vtt.contains("<v Alice>Hello world</v>"));
    }
}
