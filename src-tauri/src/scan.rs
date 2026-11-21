use std::fs;
use std::path::Path;

/// Audio/video only — used when enqueueing a folder for speech transcription.
/// Images/PDF/DOCX are vision/OCR inputs and must not block the Whisper queue.
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

fn resolve_dir(root: &Path) -> Result<std::path::PathBuf, String> {
    match root.canonicalize() {
        Ok(p) if p.is_dir() => Ok(p),
        Ok(_) => Err("Path is not a directory".to_string()),
        // OneDrive / cloud placeholders often fail canonicalize while still listing.
        Err(e) => {
            if root.is_dir() {
                Ok(root.to_path_buf())
            } else if root.is_file() {
                Err("Path is not a directory".to_string())
            } else {
                Err(format!("Invalid folder path: {e}"))
            }
        }
    }
}

/// List media files under `root`. Sorted for stable output. Skips symlinks to dirs for safety.
pub fn scan_media_folder(root: &Path, recursive: bool) -> Result<Vec<String>, String> {
    let root = resolve_dir(root)?;

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

/// Expand a drop/paste mix of files and folders into concrete media file paths.
///
/// - Directories → scanned (same rules as [`scan_media_folder`]).
/// - Media files → included as-is.
/// - Non-media files → skipped (listed in `skipped_non_media`).
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExpandMediaPathsResult {
    pub files: Vec<String>,
    pub empty_folders: Vec<String>,
    pub skipped_non_media: Vec<String>,
    pub errors: Vec<String>,
}

pub fn expand_media_paths(paths: &[String], recursive: bool) -> ExpandMediaPathsResult {
    let mut files: Vec<String> = Vec::new();
    let mut empty_folders: Vec<String> = Vec::new();
    let mut skipped_non_media: Vec<String> = Vec::new();
    let mut errors: Vec<String> = Vec::new();

    for raw in paths {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        let p = std::path::PathBuf::from(trimmed);
        if p.is_dir() {
            match scan_media_folder(&p, recursive) {
                Ok(found) if found.is_empty() => empty_folders.push(trimmed.to_string()),
                Ok(found) => files.extend(found),
                Err(e) => errors.push(format!("{trimmed}: {e}")),
            }
            continue;
        }
        if p.is_file() {
            if is_media_file(&p) {
                match push_path(&mut files, &p) {
                    Ok(()) => {}
                    Err(e) => errors.push(format!("{trimmed}: {e}")),
                }
            } else {
                skipped_non_media.push(trimmed.to_string());
            }
            continue;
        }
        errors.push(format!("{trimmed}: path not found"));
    }

    files.sort();
    files.dedup();
    ExpandMediaPathsResult {
        files,
        empty_folders,
        skipped_non_media,
        errors,
    }
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

    #[test]
    fn expand_mixes_folder_and_file() {
        let dir = std::env::temp_dir().join("v2t-scan-test-expand");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let nested = dir.join("clip.mp4");
        File::create(&nested).unwrap();
        let loose = std::env::temp_dir().join("v2t-scan-test-expand-loose.mp3");
        File::create(&loose).unwrap();
        let txt = dir.join("notes.txt");
        File::create(&txt).unwrap();

        let result = expand_media_paths(
            &[
                dir.to_string_lossy().to_string(),
                loose.to_string_lossy().to_string(),
                txt.to_string_lossy().to_string(),
            ],
            false,
        );
        assert_eq!(result.files.len(), 2);
        assert!(result.files.iter().any(|p| p.ends_with("clip.mp4")));
        assert!(result.files.iter().any(|p| p.ends_with("v2t-scan-test-expand-loose.mp3")));
        assert_eq!(result.skipped_non_media.len(), 1);
        assert!(result.empty_folders.is_empty());
        assert!(result.errors.is_empty());

        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_file(&loose);
    }
}
