//! Translate yt-dlp stderr/stdout lines into structured progress events.
//!
//! Heuristics only — yt-dlp output format is not stable across versions. We bucket
//! download percent to 5%-steps so a multi-hour playlist produces tens of progress
//! lines per video, not thousands.

/// Structured event extracted from one yt-dlp output line.
///
/// `Item` fires once when yt-dlp begins a new playlist entry; `Progress` fires
/// every ~5% bucket inside the current entry; `ExtractAudio` / `Merger` mark the
/// post-download phase. Lines we don't recognize → `None` from `parse_yt_dlp_line`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum YtDlpEvent {
    /// `[download] Downloading item N of M`. `n` is 1-based.
    Item { n: u32, total: u32 },
    /// `[download] N% of SIZE at RATE ETA TIME` — `percent_bucket` is 0..=100 in 5-steps.
    /// `tail` is the human-friendly remainder (e.g. `"of 47.85MiB at 2.34MiB/s ETA 00:25"`).
    Progress { percent_bucket: u8, tail: String },
    /// `[ExtractAudio] Destination: …`
    ExtractAudio,
    /// `[Merger] Merging formats into …`
    Merger,
}

impl YtDlpEvent {
    /// Short human-friendly message for the legacy log line / pipeline-log fallback.
    pub fn short_message(&self) -> String {
        match self {
            YtDlpEvent::Item { n, total } => format!("item {n}/{total}"),
            YtDlpEvent::Progress { percent_bucket, tail } => {
                if tail.is_empty() {
                    format!("{percent_bucket}%")
                } else {
                    format!("{percent_bucket}% {tail}")
                }
            }
            YtDlpEvent::ExtractAudio => "extracting audio".to_string(),
            YtDlpEvent::Merger => "merging formats".to_string(),
        }
    }
}

/// Parse one stderr/stdout line. Returns `Some(YtDlpEvent)` for lines we recognize as
/// useful progress (item N/M, download %, extract-audio, merger). Returns `None`
/// for noise (`[youtube] xyz: Downloading webpage`, `[info] …`, blanks, etc.).
pub fn parse_yt_dlp_line(line: &str) -> Option<YtDlpEvent> {
    let l = line.trim();
    if l.is_empty() {
        return None;
    }

    // [download] Downloading item N of M
    if let Some(rest) = l.strip_prefix("[download] Downloading item ") {
        let mut it = rest.split_whitespace();
        let n_s = it.next()?;
        let _of = it.next()?;
        let m_s = it.next()?;
        let n = n_s.parse::<u32>().ok()?;
        let total = m_s.parse::<u32>().ok()?;
        return Some(YtDlpEvent::Item { n, total });
    }

    // [download]   12.3% of   45.67MiB at   2.34MiB/s ETA 00:25
    // [download] 100% of   45.67MiB in 00:21
    if l.starts_with("[download]") && l.contains('%') && l.contains(" of ") {
        if let Some(p_idx) = l.find('%') {
            let bytes = l.as_bytes();
            let mut start = p_idx;
            while start > 0 {
                let c = bytes[start - 1] as char;
                if c.is_ascii_digit() || c == '.' {
                    start -= 1;
                } else {
                    break;
                }
            }
            if start < p_idx {
                if let Ok(percent) = l[start..p_idx].parse::<f32>() {
                    let bucket_u32 = ((percent / 5.0).floor() as u32)
                        .saturating_mul(5)
                        .min(100);
                    let percent_bucket = bucket_u32 as u8;
                    let tail = l[p_idx + 1..]
                        .split_whitespace()
                        .collect::<Vec<_>>()
                        .join(" ");
                    return Some(YtDlpEvent::Progress { percent_bucket, tail });
                }
            }
        }
    }

    if l.starts_with("[ExtractAudio]") {
        return Some(YtDlpEvent::ExtractAudio);
    }

    if l.starts_with("[Merger]") {
        return Some(YtDlpEvent::Merger);
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_item_count() {
        assert_eq!(
            parse_yt_dlp_line("[download] Downloading item 5 of 39"),
            Some(YtDlpEvent::Item { n: 5, total: 39 })
        );
        assert_eq!(
            parse_yt_dlp_line("[download] Downloading item 1 of 1"),
            Some(YtDlpEvent::Item { n: 1, total: 1 })
        );
    }

    #[test]
    fn item_line_without_numbers_is_rejected() {
        assert_eq!(
            parse_yt_dlp_line("[download] Downloading item foo of bar"),
            None
        );
    }

    #[test]
    fn buckets_percent_to_5_steps() {
        let r1 = parse_yt_dlp_line("[download]  12.3% of  45.67MiB at  2.34MiB/s ETA 00:25").unwrap();
        let r2 = parse_yt_dlp_line("[download]  13.1% of  45.67MiB at  2.30MiB/s ETA 00:24").unwrap();
        let r3 = parse_yt_dlp_line("[download]  17.0% of  45.67MiB at  2.31MiB/s ETA 00:20").unwrap();
        assert!(matches!(r1, YtDlpEvent::Progress { percent_bucket: 10, .. }), "got {r1:?}");
        assert!(matches!(r2, YtDlpEvent::Progress { percent_bucket: 10, .. }), "got {r2:?}");
        assert!(matches!(r3, YtDlpEvent::Progress { percent_bucket: 15, .. }), "got {r3:?}");
    }

    #[test]
    fn caps_percent_at_100() {
        let r = parse_yt_dlp_line("[download] 100% of  45.67MiB in 00:21").unwrap();
        assert!(matches!(r, YtDlpEvent::Progress { percent_bucket: 100, .. }), "got {r:?}");
    }

    #[test]
    fn collapses_whitespace_in_tail() {
        let r = parse_yt_dlp_line("[download]   5.0% of   45.67MiB at   1.50MiB/s ETA 00:30").unwrap();
        match r {
            YtDlpEvent::Progress { percent_bucket, tail } => {
                assert_eq!(percent_bucket, 5);
                assert_eq!(tail, "of 45.67MiB at 1.50MiB/s ETA 00:30");
            }
            other => panic!("expected Progress, got {other:?}"),
        }
    }

    #[test]
    fn short_message_renders_progress() {
        let r = parse_yt_dlp_line("[download]   5.0% of   45.67MiB at   1.50MiB/s ETA 00:30").unwrap();
        assert_eq!(r.short_message(), "5% of 45.67MiB at 1.50MiB/s ETA 00:30");
    }

    #[test]
    fn recognizes_extract_and_merger() {
        assert_eq!(
            parse_yt_dlp_line("[ExtractAudio] Destination: foo.m4a"),
            Some(YtDlpEvent::ExtractAudio)
        );
        assert_eq!(
            parse_yt_dlp_line("[Merger] Merging formats into \"foo.mp4\""),
            Some(YtDlpEvent::Merger)
        );
    }

    #[test]
    fn short_message_renders_named_phases() {
        assert_eq!(YtDlpEvent::ExtractAudio.short_message(), "extracting audio");
        assert_eq!(YtDlpEvent::Merger.short_message(), "merging formats");
        assert_eq!(
            YtDlpEvent::Item { n: 3, total: 7 }.short_message(),
            "item 3/7"
        );
    }

    #[test]
    fn ignores_noise_and_blanks() {
        assert_eq!(parse_yt_dlp_line(""), None);
        assert_eq!(parse_yt_dlp_line("    "), None);
        assert_eq!(parse_yt_dlp_line("[youtube] xyz: Downloading webpage"), None);
        assert_eq!(parse_yt_dlp_line("[info] Available formats for xyz"), None);
        assert_eq!(parse_yt_dlp_line("[download] Destination: foo.webm"), None);
    }
}
