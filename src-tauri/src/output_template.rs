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
/// If the template has no `{ext}` and `ext == "mp4"`, rewrites a trailing `.txt` to `.mp4` (legacy templates).
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
    if ext == "mp4" {
        return rewrite_txt_suffix_to_mp4(&t);
    }
    t
}

fn rewrite_txt_suffix_to_mp4(name: &str) -> String {
    if let Some(stripped) = name.strip_suffix(".txt") {
        format!("{stripped}.mp4")
    } else if let Some(dot) = name.rfind('.') {
        let (base, _) = name.split_at(dot);
        format!("{base}.mp4")
    } else {
        format!("{name}.mp4")
    }
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
