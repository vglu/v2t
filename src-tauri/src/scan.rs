use std::fs;
use std::path::Path;

const MEDIA_EXTENSIONS: &[&str] = &[
    "mp4", "mkv", "mov", "webm", "avi", "wmv", "m4v", "mp3", "wav", "m4a", "flac", "ogg", "opus",
    "aac", "wma",
];

fn is_media_file(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .and_then(|name| Path::new(name).extension())
        .and_then(|e| e.to_str())
        .map(|ext| {
            let lower = ext.to_ascii_lowercase();
            MEDIA_EXTENSIONS.iter().any(|m| *m == lower.as_str())
        })
        .unwrap_or(false)
}

/// List media files under `root`. Sorted for stable output. Skips symlinks to dirs for safety.
pub fn scan_media_folder(root: &Path, recursive: bool) -> Result<Vec<String>, String> {
    let root = root
        .canonicalize()
        .map_err(|e| format!("Invalid folder path: {e}"))?;
    if !root.is_dir() {
        return Err("Path is not a directory".to_string());
    }

    let mut out: Vec<String> = Vec::new();

    if recursive {
        walk_recursive(&root, &root, &mut out)?;
    } else {
        for entry in fs::read_dir(&root).map_err(|e| format!("read_dir: {e}"))? {
            let entry = entry.map_err(|e| format!("entry: {e}"))?;
            let p = entry.path();
            if p.is_file() && is_media_file(&p) {
                push_path(&mut out, &p)?;
            }
        }
    }

    out.sort();
    out.dedup();
    Ok(out)
}

fn walk_recursive(_root: &Path, current: &Path, out: &mut Vec<String>) -> Result<(), String> {
    for entry in fs::read_dir(current).map_err(|e| format!("read_dir: {e}"))? {
        let entry = entry.map_err(|e| format!("entry: {e}"))?;
        let p = entry.path();
        if p.is_dir() {
            walk_recursive(_root, &p, out)?;
        } else if p.is_file() && is_media_file(&p) {
            push_path(out, &p)?;
        }
    }
    Ok(())
}

fn push_path(out: &mut Vec<String>, p: &Path) -> Result<(), String> {
    let s = p
        .to_str()
        .ok_or_else(|| "Path is not valid UTF-8".to_string())?
        .to_string();
    out.push(s);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;

    #[test]
    fn non_recursive_picks_only_media_in_root() {
        let dir = std::env::temp_dir().join("v2t-scan-test-flat");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        File::create(dir.join("a.mp3")).unwrap();
        File::create(dir.join("readme.txt")).unwrap();
        File::create(dir.join("b.mp4")).unwrap();

        let mut got = scan_media_folder(&dir, false).unwrap();
        got.sort();
        assert_eq!(got.len(), 2);
        assert!(got.iter().any(|p| p.ends_with("a.mp3")));
        assert!(got.iter().any(|p| p.ends_with("b.mp4")));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn recursive_finds_nested() {
        let dir = std::env::temp_dir().join("v2t-scan-test-rec");
        let _ = fs::remove_dir_all(&dir);
        let sub = dir.join("inner");
        fs::create_dir_all(&sub).unwrap();
        File::create(sub.join("x.wav")).unwrap();
        File::create(dir.join("y.opus")).unwrap();

        let got = scan_media_folder(&dir, true).unwrap();
        assert_eq!(got.len(), 2);

        let _ = fs::remove_dir_all(&dir);
    }
}
