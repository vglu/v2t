use serde::Serialize;
use std::path::PathBuf;
#[cfg(target_os = "macos")]
use std::process::Command;
use tauri::AppHandle;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DependencyReport {
    pub ffmpeg_found: bool,
    pub ffmpeg_path: Option<String>,
    pub yt_dlp_found: bool,
    pub yt_dlp_path: Option<String>,
    pub whisper_cli_found: bool,
    pub whisper_cli_path: Option<String>,
    /// `localWhisper` only: selected ggml file exists and SHA-1 matches catalog.
    pub whisper_model_ready: bool,
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

/// Typical Homebrew / PATH locations on macOS (Apple Silicon, Intel, Linuxbrew-on-Mac, keg layouts).
#[cfg(target_os = "macos")]
const MACOS_WHISPER_STATIC_PATHS: &[&str] = &[
    "/opt/homebrew/bin/whisper-cli",
    "/usr/local/bin/whisper-cli",
    "/opt/homebrew/opt/whisper-cpp/bin/whisper-cli",
    "/usr/local/opt/whisper-cpp/bin/whisper-cli",
    "/opt/homebrew/bin/whisper",
    "/usr/local/bin/whisper",
    "/opt/homebrew/opt/whisper-cpp/bin/whisper",
    "/usr/local/opt/whisper-cpp/bin/whisper",
    "/home/linuxbrew/.linuxbrew/bin/whisper-cli",
    "/home/linuxbrew/.linuxbrew/bin/whisper",
    "/opt/homebrew/bin/main",
    "/usr/local/bin/main",
    "/opt/homebrew/opt/whisper-cpp/bin/main",
    "/usr/local/opt/whisper-cpp/bin/main",
];

#[cfg(target_os = "macos")]
fn macos_which(executable: &str) -> Option<PathBuf> {
    let output = Command::new("/usr/bin/which")
        .arg(executable)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&output.stdout);
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return None;
    }
    let p = PathBuf::from(trimmed);
    if p.is_file() {
        Some(p)
    } else {
        None
    }
}

/// Resolve `whisper-cli` / `whisper` / `main` from PATH (`which`) and fixed paths (no `AppHandle`).
#[cfg(target_os = "macos")]
pub(crate) fn macos_search_whisper_cli_in_path() -> Option<PathBuf> {
    for name in ["whisper-cli", "whisper"] {
        if let Some(p) = macos_which(name) {
            return Some(p);
        }
    }
    for &static_path in MACOS_WHISPER_STATIC_PATHS {
        let p = PathBuf::from(static_path);
        if p.is_file() {
            return Some(p);
        }
    }
    macos_which("main")
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
    #[cfg(target_os = "macos")]
    {
        if let Some(p) = macos_search_whisper_cli_in_path() {
            return Some(p);
        }
    }
    None
}

/// ffmpeg / yt-dlp / whisper-cli only (no ggml check). Used in unit tests.
pub fn report_tool_paths(
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
        whisper_model_ready: false,
    }
}

fn local_whisper_model_verified(
    app: &AppHandle,
    transcription_mode: Option<&str>,
    whisper_model: Option<&str>,
    whisper_models_dir: Option<&str>,
) -> bool {
    if transcription_mode.map(str::trim) != Some("localWhisper") {
        return false;
    }
    let Some(mid) = whisper_model.map(str::trim).filter(|s| !s.is_empty()) else {
        return false;
    };
    let Some(entry) = crate::whisper_catalog::catalog_entry(mid) else {
        return false;
    };
    let Ok(dir) = crate::model_download::resolve_models_dir(app, whisper_models_dir) else {
        return false;
    };
    let path = dir.join(entry.file_name);
    if !path.is_file() {
        return false;
    }
    crate::model_download::file_matches_sha1(&path, entry.sha1_hex).unwrap_or(false)
}

pub fn check_dependencies(
    app: &AppHandle,
    ffmpeg_override: Option<&str>,
    yt_dlp_override: Option<&str>,
    whisper_cli_override: Option<&str>,
    transcription_mode: Option<&str>,
    whisper_model: Option<&str>,
    whisper_models_dir: Option<&str>,
) -> DependencyReport {
    let mut r = report_tool_paths(ffmpeg_override, yt_dlp_override, whisper_cli_override);
    if transcription_mode.map(str::trim) == Some("browserWhisper") {
        r.whisper_cli_found = true;
        r.whisper_model_ready = true;
        return r;
    }
    r.whisper_model_ready = local_whisper_model_verified(
        app,
        transcription_mode,
        whisper_model,
        whisper_models_dir,
    );
    r
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;

    #[test]
    fn empty_override_uses_none_without_exe() {
        let r = report_tool_paths(None, None, None);
        assert!(!r.ffmpeg_found);
        assert!(!r.yt_dlp_found);
        assert!(!r.whisper_cli_found);
        assert!(!r.whisper_model_ready);
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
        let r = report_tool_paths(Some(p), None, None);
        assert!(r.ffmpeg_found);
        assert_eq!(r.ffmpeg_path.as_deref(), Some(p));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
