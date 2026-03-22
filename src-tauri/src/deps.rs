use serde::Serialize;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DependencyReport {
    pub ffmpeg_found: bool,
    pub ffmpeg_path: Option<String>,
    pub yt_dlp_found: bool,
    pub yt_dlp_path: Option<String>,
    pub whisper_cli_found: bool,
    pub whisper_cli_path: Option<String>,
}

fn exe_file_name(stem: &str) -> String {
    if cfg!(target_os = "windows") {
        format!("{stem}.exe")
    } else {
        stem.to_string()
    }
}

fn sidecar_dir() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
}

/// Resolve a tool: use override if the path exists, else look next to the executable.
pub fn resolve_tool_path(override_path: Option<&str>, default_stem: &str) -> Option<PathBuf> {
    if let Some(p) = override_path {
        let trimmed = p.trim();
        if !trimmed.is_empty() {
            let pb = PathBuf::from(trimmed);
            if pb.is_file() {
                return Some(pb);
            }
        }
    }
    let name = exe_file_name(default_stem);
    let dir = sidecar_dir()?;
    let c = dir.join(&name);
    if c.is_file() {
        return Some(c);
    }
    let in_bin = dir.join("bin").join(&name);
    if in_bin.is_file() {
        return Some(in_bin);
    }
    None
}

/// whisper.cpp builds ship `whisper-cli`; older builds used `main`.
pub fn resolve_whisper_cli_path(override_path: Option<&str>) -> Option<PathBuf> {
    if let Some(p) = override_path {
        let trimmed = p.trim();
        if !trimmed.is_empty() {
            let pb = PathBuf::from(trimmed);
            if pb.is_file() {
                return Some(pb);
            }
        }
    }
    for stem in ["whisper-cli", "main"] {
        let name = exe_file_name(stem);
        let dir = sidecar_dir()?;
        let c = dir.join(&name);
        if c.is_file() {
            return Some(c);
        }
        let in_bin = dir.join("bin").join(&name);
        if in_bin.is_file() {
            return Some(in_bin);
        }
    }
    None
}

pub fn check_dependencies(
    ffmpeg_override: Option<&str>,
    yt_dlp_override: Option<&str>,
    whisper_cli_override: Option<&str>,
) -> DependencyReport {
    let ffmpeg_path = resolve_tool_path(ffmpeg_override, "ffmpeg");
    let yt_dlp_path = resolve_tool_path(yt_dlp_override, "yt-dlp");
    let whisper_cli_path = resolve_whisper_cli_path(whisper_cli_override);
    DependencyReport {
        ffmpeg_found: ffmpeg_path.is_some(),
        ffmpeg_path: ffmpeg_path.as_ref().and_then(|p| p.to_str().map(String::from)),
        yt_dlp_found: yt_dlp_path.is_some(),
        yt_dlp_path: yt_dlp_path.as_ref().and_then(|p| p.to_str().map(String::from)),
        whisper_cli_found: whisper_cli_path.is_some(),
        whisper_cli_path: whisper_cli_path
            .as_ref()
            .and_then(|p| p.to_str().map(String::from)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;

    #[test]
    fn empty_override_uses_none_without_exe() {
        let r = check_dependencies(None, None, None);
        assert!(!r.ffmpeg_found);
        assert!(!r.yt_dlp_found);
        assert!(!r.whisper_cli_found);
    }

    #[test]
    fn override_wins_when_file_exists() {
        let dir = std::env::temp_dir().join("v2t-deps-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let fake = dir.join(if cfg!(target_os = "windows") {
            "ffmpeg.exe"
        } else {
            "ffmpeg"
        });
        File::create(&fake).unwrap();
        let p = fake.to_str().unwrap();
        let r = check_dependencies(Some(p), None, None);
        assert!(r.ffmpeg_found);
        assert_eq!(r.ffmpeg_path.as_deref(), Some(p));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
