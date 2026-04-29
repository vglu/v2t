//! Embedded `tools.manifest.json`: download URLs and optional SHA-256 for verification.

use serde::Deserialize;
use std::sync::OnceLock;

const MANIFEST_STR: &str = include_str!("../tools.manifest.json");

#[derive(Debug, Clone, Deserialize)]
pub struct ToolDownloadEntry {
    pub url: String,
    #[serde(default)]
    pub sha256: String,
}

#[derive(Debug, Deserialize)]
struct ToolsManifestFile {
    #[serde(default)]
    schema_version: u32,
    windows: WindowsSection,
    macos: MacosSection,
}

#[derive(Debug, Deserialize)]
struct WindowsSection {
    yt_dlp_exe: ToolDownloadEntry,
    ffmpeg_zip: ToolDownloadEntry,
    whisper_zip_cpu: ToolDownloadEntry,
    whisper_zip_cublas: ToolDownloadEntry,
    whisper_zip_vulkan: ToolDownloadEntry,
}

#[derive(Debug, Deserialize)]
struct MacosSection {
    yt_dlp: ToolDownloadEntry,
    ffmpeg_darwin_arm64: ToolDownloadEntry,
    ffmpeg_darwin_x64: ToolDownloadEntry,
}

#[allow(dead_code)]
pub struct ToolsManifest {
    pub windows_yt_dlp_exe: ToolDownloadEntry,
    pub windows_ffmpeg_zip: ToolDownloadEntry,
    pub windows_whisper_zip_cpu: ToolDownloadEntry,
    pub windows_whisper_zip_cublas: ToolDownloadEntry,
    pub windows_whisper_zip_vulkan: ToolDownloadEntry,
    pub macos_yt_dlp: ToolDownloadEntry,
    pub macos_ffmpeg_darwin_arm64: ToolDownloadEntry,
    pub macos_ffmpeg_darwin_x64: ToolDownloadEntry,
}

static MANIFEST: OnceLock<ToolsManifest> = OnceLock::new();

pub fn tools_manifest() -> &'static ToolsManifest {
    MANIFEST.get_or_init(|| {
        let f: ToolsManifestFile =
            serde_json::from_str(MANIFEST_STR).expect("tools.manifest.json must parse");
        let _ = f.schema_version;
        ToolsManifest {
            windows_yt_dlp_exe: f.windows.yt_dlp_exe,
            windows_ffmpeg_zip: f.windows.ffmpeg_zip,
            windows_whisper_zip_cpu: f.windows.whisper_zip_cpu,
            windows_whisper_zip_cublas: f.windows.whisper_zip_cublas,
            windows_whisper_zip_vulkan: f.windows.whisper_zip_vulkan,
            macos_yt_dlp: f.macos.yt_dlp,
            macos_ffmpeg_darwin_arm64: f.macos.ffmpeg_darwin_arm64,
            macos_ffmpeg_darwin_x64: f.macos.ffmpeg_darwin_x64,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_manifest_parses() {
        let m = tools_manifest();
        assert!(!m.windows_yt_dlp_exe.url.is_empty());
        assert!(!m.windows_whisper_zip_cpu.sha256.is_empty());
        assert!(!m.macos_yt_dlp.sha256.is_empty());
        // GPU variants ship with empty sha256 today (upstream re-uploads can shift digests),
        // but the URLs must always be present.
        assert!(m.windows_whisper_zip_cublas.url.contains("whisper-cublas"));
        assert!(m.windows_whisper_zip_vulkan.url.contains("whisper-vulkan"));
    }
}
