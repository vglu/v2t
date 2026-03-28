use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use tauri::Manager;

use crate::api_key_store;

const SETTINGS_FILE: &str = "settings.json";

fn default_onboarding_completed_for_serde() -> bool {
    // Missing field in existing settings.json → treat as completed (no wizard for upgrades).
    true
}

fn default_whisper_model_id() -> String {
    "base".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum TranscriptionMode {
    #[default]
    HttpApi,
    LocalWhisper,
    /// Transformers.js / WASM in the webview after Rust prepare.
    BrowserWhisper,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub output_dir: Option<String>,
    pub filename_template: String,
    pub ffmpeg_path: Option<String>,
    pub yt_dlp_path: Option<String>,
    /// Passed to yt-dlp as `--js-runtimes` when non-empty (YouTube EJS).
    #[serde(default)]
    pub yt_dlp_js_runtimes: Option<String>,
    pub delete_audio_after: bool,
    /// After URL download + transcribe, also save best merged video (mp4) to the output folder.
    #[serde(default)]
    pub keep_downloaded_video: bool,
    pub api_base_url: String,
    pub api_model: String,
    pub api_key: String,
    pub language: Option<String>,
    /// When adding a folder to the queue, scan subfolders for media.
    #[serde(default)]
    pub recursive_folder_scan: bool,
    /// First-run setup wizard; `false` only for fresh installs (see `Default`). Absent in old JSON → completed.
    #[serde(default = "default_onboarding_completed_for_serde")]
    pub onboarding_completed: bool,
    /// OpenAI-compatible HTTP API vs local whisper.cpp CLI.
    #[serde(default)]
    pub transcription_mode: TranscriptionMode,
    /// Path to `whisper-cli` or legacy `main` binary (optional — search next to app).
    #[serde(default)]
    pub whisper_cli_path: Option<String>,
    /// Directory for `ggml-*.bin` files (optional — default `app_data_dir/models`).
    #[serde(default)]
    pub whisper_models_dir: Option<String>,
    /// Catalog id: `tiny`, `base`, `small`, …
    #[serde(default = "default_whisper_model_id")]
    pub whisper_model: String,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            output_dir: None,
            filename_template: "{title}_{date}.{ext}".to_string(),
            ffmpeg_path: None,
            yt_dlp_path: None,
            yt_dlp_js_runtimes: None,
            delete_audio_after: true,
            keep_downloaded_video: false,
            api_base_url: "https://api.openai.com/v1".to_string(),
            api_model: "whisper-1".to_string(),
            api_key: String::new(),
            language: None,
            recursive_folder_scan: false,
            onboarding_completed: false,
            transcription_mode: TranscriptionMode::HttpApi,
            whisper_cli_path: None,
            whisper_models_dir: None,
            whisper_model: default_whisper_model_id(),
        }
    }
}

fn settings_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|e| format!("Failed to resolve app config dir: {e}"))?;
    fs::create_dir_all(&dir).map_err(|e| format!("Failed to create config dir: {e}"))?;
    Ok(dir.join(SETTINGS_FILE))
}

pub fn load(app: &tauri::AppHandle) -> Result<AppSettings, String> {
    let path = settings_path(app)?;
    let mut s = if !path.exists() {
        AppSettings::default()
    } else {
        let raw = fs::read_to_string(&path).map_err(|e| format!("Failed to read settings: {e}"))?;
        serde_json::from_str(&raw).map_err(|e| format!("Invalid settings JSON: {e}"))?
    };

    match api_key_store::get() {
        Ok(Some(k)) => s.api_key = k,
        Ok(None) => {
            if !s.api_key.is_empty() {
                let k = std::mem::take(&mut s.api_key);
                let _ = api_key_store::set(&k);
                s.api_key = k;
            }
        }
        Err(_) => {
            // Headless / no credential store: keep key from JSON if present.
        }
    }

    Ok(s)
}

pub fn save(app: &tauri::AppHandle, settings: &AppSettings) -> Result<(), String> {
    api_key_store::set(&settings.api_key)?;
    let mut for_disk = settings.clone();
    for_disk.api_key.clear();
    let path = settings_path(app)?;
    let raw = serde_json::to_string_pretty(&for_disk)
        .map_err(|e| format!("Failed to serialize settings: {e}"))?;
    fs::write(&path, raw).map_err(|e| format!("Failed to write settings: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_settings_roundtrip_json() {
        let s = AppSettings::default();
        let json = serde_json::to_string(&s).unwrap();
        let back: AppSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
    }

    #[test]
    fn missing_recursive_folder_scan_defaults_false() {
        let json = r#"{"outputDir":null,"filenameTemplate":"{title}_{date}.txt","ffmpegPath":null,"ytDlpPath":null,"deleteAudioAfter":true,"apiBaseUrl":"https://api.openai.com/v1","apiModel":"whisper-1","apiKey":"","language":null}"#;
        let s: AppSettings = serde_json::from_str(json).unwrap();
        assert!(!s.recursive_folder_scan);
    }

    #[test]
    fn missing_onboarding_completed_deserializes_true() {
        let json = r#"{"outputDir":null,"filenameTemplate":"{title}_{date}.txt","ffmpegPath":null,"ytDlpPath":null,"deleteAudioAfter":true,"apiBaseUrl":"https://api.openai.com/v1","apiModel":"whisper-1","apiKey":"","language":null,"recursiveFolderScan":false}"#;
        let s: AppSettings = serde_json::from_str(json).unwrap();
        assert!(s.onboarding_completed);
    }
}
