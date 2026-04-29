//! Translate yt-dlp stderr lines into short, log-friendly progress messages.
//!
//! Heuristics only — yt-dlp output format is not stable across versions. We bucket
//! download percent to 5%-steps so a multi-hour playlist produces tens of progress
//! lines per video, not thousands.

/// Parse one stderr line. Returns `Some(short_message)` for lines we recognize as
/// useful progress (item N/M, download %, extract-audio, merger). Returns `None`
/// for noise (`[youtube] xyz: Downloading webpage`, `[info] …`, blanks, etc.).
pub fn parse_yt_dlp_line(line: &str) -> Option<String> {
    let l = line.trim();
    if l.is_empty() {
        return None;
    }

    // [download] Downloading item N of M
    if let Some(rest) = l.strip_prefix("[download] Downloading item ") {
        let mut it = rest.split_whitespace();
        let n = it.next()?;
        let _of = it.next()?;
        let m = it.next()?;
        n.parse::<u32>().ok()?;
        m.parse::<u32>().ok()?;
        return Some(format!("item {n}/{m}"));
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
                    let bucket = ((percent / 5.0).floor() as u32).saturating_mul(5).min(100);
                    let tail = l[p_idx + 1..]
                        .split_whitespace()
                        .collect::<Vec<_>>()
                        .join(" ");
                    return Some(if tail.is_empty() {
                        format!("{bucket}%")
                    } else {
                        format!("{bucket}% {tail}")
                    });
                }
            }
        }
    }

    if l.starts_with("[ExtractAudio]") {
        return Some("extracting audio".to_string());
    }

    if l.starts_with("[Merger]") {
        return Some("merging formats".to_string());
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
            Some("item 5/39".to_string())
        );
        assert_eq!(
            parse_yt_dlp_line("[download] Downloading item 1 of 1"),
            Some("item 1/1".to_string())
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
        assert!(r1.starts_with("10% "), "got {r1}");
        assert!(r2.starts_with("10% "), "got {r2}");
        assert!(r3.starts_with("15% "), "got {r3}");
    }

    #[test]
    fn caps_percent_at_100() {
        let r = parse_yt_dlp_line("[download] 100% of  45.67MiB in 00:21").unwrap();
        assert!(r.starts_with("100% "), "got {r}");
    }

    #[test]
    fn collapses_whitespace_in_tail() {
        let r = parse_yt_dlp_line("[download]   5.0% of   45.67MiB at   1.50MiB/s ETA 00:30").unwrap();
        assert_eq!(r, "5% of 45.67MiB at 1.50MiB/s ETA 00:30");
    }

    #[test]
    fn recognizes_extract_and_merger() {
        assert_eq!(
            parse_yt_dlp_line("[ExtractAudio] Destination: foo.m4a"),
            Some("extracting audio".into())
        );
        assert_eq!(
            parse_yt_dlp_line("[Merger] Merging formats into \"foo.mp4\""),
            Some("merging formats".into())
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
