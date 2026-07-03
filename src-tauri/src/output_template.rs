//! Filename template and sanitization (aligned with `src/lib/outputTemplate.ts`).

/// Remove characters unsafe for Windows / macOS file names.
pub fn sanitize_filename_segment(raw: &str) -> String {
    let mut out = String::new();
    for ch in raw.trim().chars().take(120) {
        let bad = matches!(ch, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*')
            || ch.is_control();
        out.push(if bad { '_' } else { ch });
    }
    let t = out.trim();
    if t.is_empty() {
        "untitled".to_string()
    } else {
        t.to_string()
    }
}

/// Replace `{title}`, `{date}`, `{index}`, `{track}`, `{source}`, `{ext}`.
/// If the template has no `{ext}` and `ext != "txt"`, rewrites a trailing `.txt` to `.{ext}` (legacy templates).
pub fn format_output_filename(
    template: &str,
    title: &str,
    date: &str,
    index: u32,
    track: u32,
    source: &str,
    ext: &str,
) -> String {
    let t = template
        .replace("{title}", &sanitize_filename_segment(title))
        .replace("{date}", &sanitize_filename_segment(date))
        .replace("{index}", &index.to_string())
        .replace("{track}", &track.to_string())
        .replace("{source}", &sanitize_filename_segment(source));
    if template.contains("{ext}") {
        return t.replace("{ext}", ext);
    }
    if ext != "txt" {
        return rewrite_suffix_to_ext(&t, ext);
    }
    t
}

fn rewrite_suffix_to_ext(name: &str, ext: &str) -> String {
    if let Some(stripped) = name.strip_suffix(".txt") {
        format!("{stripped}.{ext}")
    } else if let Some(dot) = name.rfind('.') {
        let (base, _) = name.split_at(dot);
        format!("{base}.{ext}")
    } else {
        format!("{name}.{ext}")
    }
}

/// When a job has multiple tracks (playlist) and the template omits `{track}`,
/// append `_t{N}` before the extension so outputs do not overwrite each other.
pub fn disambiguate_playlist_filename(
    template: &str,
    name: &str,
    track: u32,
    n_tracks: u32,
) -> String {
    if n_tracks <= 1 || template.contains("{track}") {
        return name.to_string();
    }
    inject_track_suffix(name, track)
}

fn inject_track_suffix(name: &str, track: u32) -> String {
    if let Some(dot) = name.rfind('.') {
        let (base, ext) = name.split_at(dot);
        format!("{base}_t{track}{ext}")
    } else {
        format!("{name}_t{track}")
    }
}

/// Output filename for one job track (txt / arbitrary ext).
pub fn format_job_output_filename(
    template: &str,
    title: &str,
    date: &str,
    index: u32,
    track: u32,
    n_tracks: u32,
    source: &str,
    ext: &str,
) -> String {
    let name = format_output_filename(template, title, date, index, track, source, ext);
    disambiguate_playlist_filename(template, &name, track, n_tracks)
}

/// Video file next to transcript: same template rules with `ext = mp4`.
pub fn video_filename_from_transcript_template(
    template: &str,
    title: &str,
    date: &str,
    index: u32,
    track: u32,
    source: &str,
) -> String {
    format_output_filename(template, title, date, index, track, source, "mp4")
}

/// Audio file next to transcript: template rules with a caller-chosen extension
/// (`m4a`, `mp3`, `opus`, `webm`, …).
pub fn audio_filename_from_transcript_template(
    template: &str,
    title: &str,
    date: &str,
    index: u32,
    track: u32,
    source: &str,
    ext: &str,
) -> String {
    format_output_filename(template, title, date, index, track, source, ext)
}

/// Audio filename for a job track; auto-suffixes when `n_tracks > 1` and template has no `{track}`.
pub fn audio_filename_for_job(
    template: &str,
    title: &str,
    date: &str,
    index: u32,
    track: u32,
    n_tracks: u32,
    source: &str,
    ext: &str,
) -> String {
    disambiguate_playlist_filename(
        template,
        &audio_filename_from_transcript_template(template, title, date, index, track, source, ext),
        track,
        n_tracks,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn playlist_auto_suffix_when_template_omits_track() {
        let t = "{title}_{date}.txt";
        assert_eq!(
            format_job_output_filename(t, "Clip", "2026-07-03", 1, 1, 3, "url", "txt"),
            "Clip_2026-07-03_t1.txt"
        );
        assert_eq!(
            format_job_output_filename(t, "Clip", "2026-07-03", 1, 2, 3, "url", "txt"),
            "Clip_2026-07-03_t2.txt"
        );
        assert_eq!(
            audio_filename_for_job(t, "Clip", "2026-07-03", 1, 3, 3, "url", "m4a"),
            "Clip_2026-07-03_t3.m4a"
        );
    }

    #[test]
    fn single_track_no_auto_suffix_without_placeholder() {
        assert_eq!(
            format_job_output_filename(
                "{title}_{date}.txt",
                "Clip",
                "2026-07-03",
                1,
                1,
                1,
                "url",
                "txt",
            ),
            "Clip_2026-07-03.txt"
        );
    }

    #[test]
    fn explicit_track_placeholder_not_doubled() {
        assert_eq!(
            format_job_output_filename(
                "{title}_{date}_t{track}.txt",
                "Clip",
                "2026-07-03",
                1,
                2,
                5,
                "url",
                "txt",
            ),
            "Clip_2026-07-03_t2.txt"
        );
    }

    #[test]
    fn user_default_template_eleven_unique_playlist_outputs() {
        let template = "{title}_{date}.txt";
        let title = "https___www.youtube.com_watch_v=6r7";
        let date = "2026-07-03";
        let names: Vec<String> = (1..=11)
            .map(|track| {
                format_job_output_filename(template, title, date, 1, track, 11, "url", "txt")
            })
            .collect();
        assert_eq!(names.len(), 11);
        let unique: std::collections::HashSet<_> = names.iter().collect();
        assert_eq!(unique.len(), 11, "expected 11 unique paths, got {names:?}");
        assert_eq!(names[0], "https___www.youtube.com_watch_v=6r7_2026-07-03_t1.txt");
        assert_eq!(names[10], "https___www.youtube.com_watch_v=6r7_2026-07-03_t11.txt");
    }

    #[test]
    fn sanitize_strips_illegal() {
        assert_eq!(sanitize_filename_segment("a<b>c:d\"e"), "a_b_c_d_e");
    }

    #[test]
    fn sanitize_empty_becomes_untitled() {
        assert_eq!(sanitize_filename_segment("   "), "untitled");
    }

    #[test]
    fn format_replaces_placeholders() {
        let out = format_output_filename(
            "{title}_{date}_{index}_{track}_{source}.txt",
            "My / Talk",
            "2025-01-01",
            3,
            2,
            "youtube",
            "txt",
        );
        assert_eq!(out, "My _ Talk_2025-01-01_3_2_youtube.txt");
    }

    #[test]
    fn ext_placeholder() {
        let out = format_output_filename(
            "{title}_{date}.{ext}",
            "Clip",
            "2026-03-27",
            1,
            1,
            "url",
            "txt",
        );
        assert_eq!(out, "Clip_2026-03-27.txt");
        let v = format_output_filename(
            "{title}_{date}.{ext}",
            "Clip",
            "2026-03-27",
            1,
            1,
            "url",
            "mp4",
        );
        assert_eq!(v, "Clip_2026-03-27.mp4");
    }

    #[test]
    fn video_name_derived_from_txt_template() {
        let v = video_filename_from_transcript_template(
            "{title}_{date}.txt",
            "Clip",
            "2026-03-27",
            1,
            1,
            "url",
        );
        assert_eq!(v, "Clip_2026-03-27.mp4");
    }

    #[test]
    fn audio_name_uses_ext_placeholder() {
        let v = audio_filename_from_transcript_template(
            "{title}_{date}.{ext}",
            "Clip",
            "2026-03-27",
            1,
            1,
            "url",
            "m4a",
        );
        assert_eq!(v, "Clip_2026-03-27.m4a");
    }

    #[test]
    fn audio_name_derived_from_legacy_txt_template() {
        let v = audio_filename_from_transcript_template(
            "{title}_{date}.txt",
            "Clip",
            "2026-03-27",
            1,
            1,
            "url",
            "opus",
        );
        assert_eq!(v, "Clip_2026-03-27.opus");
    }

    #[test]
    fn audio_name_various_exts() {
        for ext in ["mp3", "m4a", "opus", "webm", "ogg", "flac"] {
            let v = audio_filename_from_transcript_template(
                "{title}.{ext}", "t", "d", 1, 1, "s", ext,
            );
            assert_eq!(v, format!("t.{ext}"));
        }
    }
}
