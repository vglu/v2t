//! One-click download of ffmpeg + yt-dlp into `app_data_dir/v2t/bin` (Windows only).

use std::path::{Path, PathBuf};

use futures_util::StreamExt;
use serde::Serialize;
use tauri::path::BaseDirectory;
use tauri::AppHandle;
use tauri::Emitter;
use tauri::Manager;

#[cfg(windows)]
const YT_DLP_URL: &str =
    "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp.exe";

/// BtbN FFmpeg-Builds (GPL). Zip layout includes `…/bin/ffmpeg.exe`.
#[cfg(windows)]
const FFMPEG_ZIP_URL: &str =
    "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-master-latest-win64-gpl.zip";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ToolDownloadProgress {
    tool: String,
    phase: String,
    bytes_received: u64,
    total_bytes: Option<u64>,
    message: String,
}

fn emit(app: &AppHandle, payload: ToolDownloadProgress) {
    let _ = app.emit("tool-download-progress", &payload);
}

pub fn managed_bin_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let base = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("app_data_dir: {e}"))?;
    Ok(base.join("v2t").join("bin"))
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadedMediaTools {
    pub ffmpeg_path: String,
    pub yt_dlp_path: String,
}

/// User-visible Documents folder (OS standard).
pub fn default_documents_dir(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .resolve("", BaseDirectory::Document)
        .map_err(|e| format!("resolve Document: {e}"))
}

#[cfg(not(windows))]
pub async fn download_media_tools_windows(_app: &AppHandle) -> Result<DownloadedMediaTools, String> {
    Err("Automatic download of ffmpeg / yt-dlp is only available on Windows. Install via your package manager and set paths in Settings.".to_string())
}

#[cfg(windows)]
pub async fn download_media_tools_windows(app: &AppHandle) -> Result<DownloadedMediaTools, String> {
    let dir = managed_bin_dir(app)?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("create bin dir: {e}"))?;

    let yt_dest = dir.join("yt-dlp.exe");
    let ff_dest = dir.join("ffmpeg.exe");

    emit(
        app,
        ToolDownloadProgress {
            tool: "yt-dlp".to_string(),
            phase: "downloading".to_string(),
            bytes_received: 0,
            total_bytes: None,
            message: "Downloading yt-dlp.exe…".to_string(),
        },
    );

    download_file_streaming(app, YT_DLP_URL, &yt_dest, "yt-dlp").await?;

    emit(
        app,
        ToolDownloadProgress {
            tool: "yt-dlp".to_string(),
            phase: "done".to_string(),
            bytes_received: yt_dest
                .metadata()
                .map(|m| m.len())
                .unwrap_or(0),
            total_bytes: None,
            message: "yt-dlp ready".to_string(),
        },
    );

    let tmp_zip = std::env::temp_dir().join(format!(
        "v2t-ffmpeg-{}.zip",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    ));

    emit(
        app,
        ToolDownloadProgress {
            tool: "ffmpeg".to_string(),
            phase: "downloading".to_string(),
            bytes_received: 0,
            total_bytes: None,
            message: "Downloading FFmpeg zip (this may take a while)…".to_string(),
        },
    );

    download_file_streaming(app, FFMPEG_ZIP_URL, &tmp_zip, "ffmpeg").await?;

    emit(
        app,
        ToolDownloadProgress {
            tool: "ffmpeg".to_string(),
            phase: "extracting".to_string(),
            bytes_received: tmp_zip.metadata().map(|m| m.len()).unwrap_or(0),
            total_bytes: None,
            message: "Extracting ffmpeg.exe…".to_string(),
        },
    );

    let zip_path = tmp_zip.clone();
    let ff_dest_clone = ff_dest.clone();
    tokio::task::spawn_blocking(move || extract_ffmpeg_exe_from_zip(&zip_path, &ff_dest_clone))
        .await
        .map_err(|e| format!("extract task: {e}"))??;

    let _ = std::fs::remove_file(&tmp_zip);

    emit(
        app,
        ToolDownloadProgress {
            tool: "ffmpeg".to_string(),
            phase: "done".to_string(),
            bytes_received: ff_dest
                .metadata()
                .map(|m| m.len())
                .unwrap_or(0),
            total_bytes: None,
            message: "ffmpeg ready".to_string(),
        },
    );

    Ok(DownloadedMediaTools {
        ffmpeg_path: ff_dest.to_string_lossy().into_owned(),
        yt_dlp_path: yt_dest.to_string_lossy().into_owned(),
    })
}

#[cfg(windows)]
fn extract_ffmpeg_exe_from_zip(zip_path: &Path, dest_ffmpeg: &Path) -> Result<(), String> {
    use std::fs::File;
    use zip::ZipArchive;

    let f = File::open(zip_path).map_err(|e| format!("open zip: {e}"))?;
    let mut archive = ZipArchive::new(f).map_err(|e| format!("read zip: {e}"))?;

    for i in 0..archive.len() {
        let mut file = archive.by_index(i).map_err(|e| format!("zip entry: {e}"))?;
        let name = file.name().replace('\\', "/");
        if name.ends_with("/bin/ffmpeg.exe") {
            let mut out = std::fs::File::create(dest_ffmpeg)
                .map_err(|e| format!("create ffmpeg.exe: {e}"))?;
            std::io::copy(&mut file, &mut out).map_err(|e| format!("write ffmpeg.exe: {e}"))?;
            return Ok(());
        }
    }

    Err("ffmpeg.exe not found inside the downloaded zip (upstream layout changed)".to_string())
}

#[cfg(windows)]
async fn download_file_streaming(
    app: &AppHandle,
    url: &str,
    dest: &Path,
    tool_label: &str,
) -> Result<(), String> {
    use tokio::fs::File;
    use tokio::io::AsyncWriteExt;

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(7200))
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()
        .map_err(|e| format!("HTTP client: {e}"))?;

    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("GET {tool_label}: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("GET {} HTTP {}", tool_label, resp.status()));
    }

    let total = resp
        .headers()
        .get(reqwest::header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok());

    let mut stream = resp.bytes_stream();
    let mut file = File::create(dest)
        .await
        .map_err(|e| format!("create {}: {e}", dest.display()))?;

    let mut received: u64 = 0;
    let mut last_emit = 0u64;

    while let Some(item) = stream.next().await {
        let chunk = match item {
            Ok(c) => c,
            Err(e) => {
                let _ = tokio::fs::remove_file(dest).await;
                return Err(format!("{tool_label} stream: {e}"));
            }
        };
        file
            .write_all(&chunk)
            .await
            .map_err(|e| format!("write {}: {e}", dest.display()))?;
        received += chunk.len() as u64;

        if received.saturating_sub(last_emit) > 1024 * 1024 || total == Some(received) {
            last_emit = received;
            emit(
                app,
                ToolDownloadProgress {
                    tool: tool_label.to_string(),
                    phase: "downloading".to_string(),
                    bytes_received: received,
                    total_bytes: total,
                    message: format!(
                        "Downloaded {} MiB{}",
                        received / (1024 * 1024),
                        total
                            .map(|t| format!(" / {} MiB", t / (1024 * 1024)))
                            .unwrap_or_default()
                    ),
                },
            );
        }
    }

    file.sync_all()
        .await
        .map_err(|e| format!("sync: {e}"))?;
    Ok(())
}
