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

pub fn format_output_filename(
    template: &str,
    title: &str,
    date: &str,
    index: u32,
    track: u32,
    source: &str,
) -> String {
    template
        .replace("{title}", &sanitize_filename_segment(title))
        .replace("{date}", &sanitize_filename_segment(date))
        .replace("{index}", &index.to_string())
        .replace("{track}", &track.to_string())
        .replace("{source}", &sanitize_filename_segment(source))
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
        );
        assert_eq!(out, "My _ Talk_2025-01-01_3_2_youtube.txt");
    }
}
