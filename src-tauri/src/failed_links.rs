use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

fn normalize_url_for_retry(url: &str) -> String {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    // Remove list/index/start_radio query params, same intent as
    // src/components/QueuePanel.tsx → stripPlaylistParams.
    let (base, query_plus_frag) = match trimmed.split_once('?') {
        Some((b, q)) => (b, Some(q)),
        None => (trimmed, None),
    };

    let query_plus_frag = match query_plus_frag {
        Some(q) => q,
        None => return base.to_string(),
    };

    let (query, frag) = query_plus_frag.split_once('#').map_or((query_plus_frag, None), |(q, f)| (q, Some(f)));

    let mut kept: Vec<&str> = Vec::new();
    for kv in query.split('&') {
        if kv.is_empty() {
            continue;
        }
        let (k, _v) = kv.split_once('=').map_or((kv, ""), |p| (p.0, p.1));
        let key = k.trim();
        if key.eq_ignore_ascii_case("list")
            || key.eq_ignore_ascii_case("index")
            || key.eq_ignore_ascii_case("start_radio")
        {
            continue;
        }
        kept.push(kv);
    }

    let mut out = String::from(base);
    if !kept.is_empty() {
        out.push('?');
        out.push_str(&kept.join("&"));
    }
    if let Some(f) = frag {
        out.push('#');
        out.push_str(f);
    }
    out
}

/// Keep only failed URLs from an existing links file.
///
/// If `links_file_path` doesn't exist, the file is created with `failed_urls`
/// (de-duplicated, preserving input order).
///
/// If `copy_to_dir` is provided, we also write a copy into that directory
/// using the same basename as `links_file_path`.
pub fn save_failed_links_sidecar(
    links_file_path: &Path,
    failed_urls: &[String],
    unsupported_urls: &[String],
    copy_to_dir: Option<&Path>,
) -> Result<u32, String> {
    let failed_urls_trimmed: Vec<String> = failed_urls
        .iter()
        .map(|u| u.trim())
        .filter(|u| !u.is_empty())
        .map(|u| u.to_string())
        .collect();

    if failed_urls_trimmed.is_empty() {
        return Ok(0);
    }

    let mut failed_norm = HashSet::new();
    for u in &failed_urls_trimmed {
        failed_norm.insert(normalize_url_for_retry(u));
    }

    let unsupported_urls_trimmed: Vec<String> = unsupported_urls
        .iter()
        .map(|u| u.trim())
        .filter(|u| !u.is_empty())
        .map(|u| u.to_string())
        .collect();

    let mut unsupported_norm = HashSet::new();
    for u in &unsupported_urls_trimmed {
        unsupported_norm.insert(normalize_url_for_retry(u));
    }

    let basename = links_file_path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| "Invalid links file path basename".to_string())?;

    let unsupported_file_path = {
        let parent = links_file_path
            .parent()
            .ok_or_else(|| "links file parent is missing".to_string())?;
        let stem = links_file_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(basename);
        parent.join(format!("{stem}.unsupported.txt"))
    };

    let mut kept_lines: Vec<String> = Vec::new();
    if links_file_path.is_file() {
        let raw = fs::read_to_string(links_file_path)
            .map_err(|e| format!("Read links file failed: {e}"))?;
        for line in raw.lines() {
            let t = line.trim();
            if t.is_empty() {
                continue;
            }
            let norm = normalize_url_for_retry(t);
            if failed_norm.contains(&norm) && !unsupported_norm.contains(&norm) {
                kept_lines.push(t.to_string());
            }
        }
    } else {
        // Create new file with failed URLs (de-dup, preserve order).
        let mut seen = HashSet::new();
        for u in failed_urls_trimmed {
            let norm = normalize_url_for_retry(&u);
            if seen.insert(norm.clone()) && !unsupported_norm.contains(&norm) {
                kept_lines.push(u);
            }
        }
    }

    // Ensure parent dir exists.
    if let Some(parent) = links_file_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Create links file parent failed: {e}"))?;
    }

    let body = if kept_lines.is_empty() {
        String::new()
    } else {
        kept_lines.join("\n") + "\n"
    };
    fs::write(links_file_path, body).map_err(|e| format!("Write links file failed: {e}"))?;

    if let Some(copy_dir) = copy_to_dir {
        let copy_path: PathBuf = copy_dir.join(basename);
        if let Some(parent) = copy_path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        fs::write(&copy_path, fs::read_to_string(links_file_path).unwrap_or_default())
            .map_err(|e| format!("Write copy links file failed: {e}"))?;
    }

    // Write unsupported URLs sidecar file next to broken links file.
    if !unsupported_urls_trimmed.is_empty() {
        // Mirror existing file update behavior: only keep lines that still
        // belong to current unsupported set.
        let mut kept_unsupported: Vec<String> = Vec::new();
        if unsupported_file_path.is_file() {
            let raw = fs::read_to_string(&unsupported_file_path)
                .map_err(|e| format!("Read unsupported links file failed: {e}"))?;
            for line in raw.lines() {
                let t = line.trim();
                if t.is_empty() {
                    continue;
                }
                let norm = normalize_url_for_retry(t);
                if unsupported_norm.contains(&norm) {
                    kept_unsupported.push(t.to_string());
                }
            }
        } else {
            let mut seen = HashSet::new();
            for u in unsupported_urls_trimmed {
                let norm = normalize_url_for_retry(&u);
                if seen.insert(norm) {
                    kept_unsupported.push(u);
                }
            }
        }

        let body = if kept_unsupported.is_empty() {
            String::new()
        } else {
            kept_unsupported.join("\n") + "\n"
        };
        fs::write(&unsupported_file_path, body)
            .map_err(|e| format!("Write unsupported links file failed: {e}"))?;
    }

    Ok(kept_lines.len() as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_strips_list_params() {
        let in_url = "https://youtu.be/abc?list=PLx&index=3&start_radio=1&v=abc";
        let out = normalize_url_for_retry(in_url);
        assert!(!out.contains("list="));
        assert!(!out.contains("index="));
        assert!(!out.contains("start_radio="));
        assert!(out.contains("v=abc"));
    }

    #[test]
    fn excludes_unsupported_from_broken_links_and_writes_sidecar() {
        let tmp = std::env::temp_dir().join(format!(
            "v2t-failed-links-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
        ));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        let broken_path = tmp.join("broken_links.txt");
        fs::write(
            &broken_path,
            "https://tiktok.com/@user\nhttps://example.com/video1\n",
        )
        .unwrap();

        let failed_urls = vec![
            "https://example.com/video1".to_string(),
            "https://tiktok.com/@user".to_string(),
        ];
        let unsupported_urls = vec!["https://tiktok.com/@user".to_string()];

        let saved = save_failed_links_sidecar(
            &broken_path,
            &failed_urls,
            &unsupported_urls,
            None,
        )
        .unwrap();
        assert_eq!(saved, 1);

        let broken_body = fs::read_to_string(&broken_path).unwrap();
        assert!(broken_body.contains("https://example.com/video1"));
        assert!(!broken_body.contains("https://tiktok.com/@user"));

        let unsupported_path = tmp.join("broken_links.unsupported.txt");
        let unsupported_body = fs::read_to_string(&unsupported_path).unwrap();
        assert!(unsupported_body.contains("https://tiktok.com/@user"));
        assert!(!unsupported_body.contains("https://example.com/video1"));

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn writes_unsupported_sidecar_when_main_file_is_new() {
        let tmp = std::env::temp_dir().join(format!(
            "v2t-failed-links-{}-{}-new",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
        ));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        let broken_path = tmp.join("broken_links.txt");

        let failed_urls = vec!["https://example.com/video1".to_string()];
        let unsupported_urls = vec![
            "https://tiktok.com/photo/123".to_string(),
            "https://tiktok.com/photo/123".to_string(),
        ];

        let saved = save_failed_links_sidecar(
            &broken_path,
            &failed_urls,
            &unsupported_urls,
            None,
        )
        .unwrap();
        assert_eq!(saved, 1);

        let unsupported_path = tmp.join("broken_links.unsupported.txt");
        let unsupported_body = fs::read_to_string(&unsupported_path).unwrap();
        assert!(unsupported_body.contains("https://tiktok.com/photo/123"));
        // De-dup.
        assert_eq!(
            unsupported_body.lines().filter(|l| !l.trim().is_empty()).count(),
            1
        );

        let _ = fs::remove_dir_all(&tmp);
    }
}

