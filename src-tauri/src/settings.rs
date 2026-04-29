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

fn default_subtitle_priority_langs() -> Vec<String> {
    vec!["uk".to_string(), "ru".to_string(), "en".to_string()]
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

/// Which browser yt-dlp should read cookies from.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum CookiesFromBrowser {
    /// OS default: Edge on Windows, Chrome on macOS, Firefox on Linux.
    #[default]
    Auto,
    Chrome,
    Brave,
    Edge,
    Firefox,
    /// Disabled — do not pass --cookies-from-browser.
    #[serde(rename = "none")]
    Disabled,
}

/// Backend used by `whisper-cli`. `Auto` picks CUDA if NVIDIA is detected, else CPU.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum WhisperAcceleration {
    #[default]
    Auto,
    Cuda,
    Vulkan,
    Cpu,
}

/// UI language code. `Auto` defers to the OS locale (`navigator.language` on
/// the React side); other variants are ISO 639-1 codes for the locales
/// supported by the i18n catalogs in `src/locales/`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum UiLanguage {
    #[default]
    Auto,
    En,
    Uk,
    Ru,
    De,
    Es,
    Fr,
    Pl,
}

/// Audio format for saved downloaded audio.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum DownloadedAudioFormat {
    /// Keep bestaudio from yt-dlp (URL) or `-c:a copy` for local video — no re-encoding.
    #[default]
    Original,
    Mp3,
    M4a,
}

impl DownloadedAudioFormat {
    /// Value for yt-dlp `--audio-format`, or `None` when the original container should be preserved.
    pub fn yt_dlp_arg(&self) -> Option<&'static str> {
        match self {
            DownloadedAudioFormat::Original => None,
            DownloadedAudioFormat::Mp3 => Some("mp3"),
            DownloadedAudioFormat::M4a => Some("m4a"),
        }
    }
}

impl CookiesFromBrowser {
    /// Returns the yt-dlp `--cookies-from-browser` value, or `None` if disabled.
    pub fn yt_dlp_arg(&self) -> Option<&'static str> {
        match self {
            CookiesFromBrowser::Auto => {
                if cfg!(target_os = "windows") {
                    Some("edge")
                } else if cfg!(target_os = "macos") {
                    Some("chrome")
                } else {
                    Some("firefox")
                }
            }
            CookiesFromBrowser::Chrome => Some("chrome"),
            CookiesFromBrowser::Brave => Some("brave"),
            CookiesFromBrowser::Edge => Some("edge"),
            CookiesFromBrowser::Firefox => Some("firefox"),
            CookiesFromBrowser::Disabled => None,
        }
    }
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
    /// Also save extracted audio (URL: copy from yt-dlp output; local video: ffmpeg extract).
    #[serde(default)]
    pub keep_downloaded_audio: bool,
    /// Format for saved audio. `Original` keeps bestaudio / copies the source audio stream without re-encoding.
    #[serde(default)]
    pub downloaded_audio_format: DownloadedAudioFormat,
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
    /// Browser for yt-dlp `--cookies-from-browser` (helps with age-gated YouTube / TikTok).
    #[serde(default)]
    pub cookies_from_browser: CookiesFromBrowser,
    /// `local Whisper` backend (CPU vs CUDA vs Vulkan). `Auto` resolves based on detected GPU.
    #[serde(default)]
    pub whisper_acceleration: WhisperAcceleration,
    /// When a YouTube video has manual subtitles in a priority language, fetch
    /// them via yt-dlp and skip the download + Whisper passes entirely.
    /// Auto-generated captions are intentionally never used (lower quality than
    /// Whisper-medium for non-English).
    #[serde(default)]
    pub use_subtitles_when_available: bool,
    /// Priority order for picking which manual subtitle track to fetch.
    /// First match wins; missing field deserializes to `["uk", "ru", "en"]`.
    #[serde(default = "default_subtitle_priority_langs")]
    pub subtitle_priority_langs: Vec<String>,
    /// When the subtitle fast-path runs, also save the raw `.srt` file
    /// next to the `.txt` transcript (preserves timings).
    #[serde(default)]
    pub keep_srt: bool,
    /// UI language; `Auto` lets the React layer derive from `navigator.language`.
    #[serde(default)]
    pub ui_language: UiLanguage,
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
            keep_downloaded_audio: false,
            downloaded_audio_format: DownloadedAudioFormat::Original,
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
            cookies_from_browser: CookiesFromBrowser::Auto,
            whisper_acceleration: WhisperAcceleration::Auto,
            use_subtitles_when_available: false,
            subtitle_priority_langs: default_subtitle_priority_langs(),
            keep_srt: false,
            ui_language: UiLanguage::Auto,
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

    #[test]
    fn missing_ui_language_defaults_to_auto() {
        let json = r#"{"outputDir":null,"filenameTemplate":"{title}_{date}.txt","ffmpegPath":null,"ytDlpPath":null,"deleteAudioAfter":true,"apiBaseUrl":"https://api.openai.com/v1","apiModel":"whisper-1","apiKey":"","language":null,"recursiveFolderScan":false}"#;
        let s: AppSettings = serde_json::from_str(json).unwrap();
        assert_eq!(s.ui_language, UiLanguage::Auto);
    }

    #[test]
    fn ui_language_roundtrip_uk() {
        let mut s = AppSettings::default();
        s.ui_language = UiLanguage::Uk;
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("\"uiLanguage\":\"uk\""));
        let back: AppSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(back.ui_language, UiLanguage::Uk);
    }

    #[test]
    fn missing_subtitle_fields_use_defaults() {
        let json = r#"{"outputDir":null,"filenameTemplate":"{title}_{date}.txt","ffmpegPath":null,"ytDlpPath":null,"deleteAudioAfter":true,"apiBaseUrl":"https://api.openai.com/v1","apiModel":"whisper-1","apiKey":"","language":null,"recursiveFolderScan":false}"#;
        let s: AppSettings = serde_json::from_str(json).unwrap();
        assert!(!s.use_subtitles_when_available);
        assert_eq!(
            s.subtitle_priority_langs,
            vec!["uk".to_string(), "ru".to_string(), "en".to_string()]
        );
        assert!(!s.keep_srt);
    }
}
