use std::path::{Path, PathBuf};

use futures_util::StreamExt;
use tokio::io::AsyncWriteExt;
use serde::Serialize;
use sha1::{Digest, Sha1};
use tauri::AppHandle;
use tauri::Emitter;
use tauri::Manager;

use crate::whisper_catalog::WhisperModelCatalogEntry;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ModelDownloadProgress {
    model_id: String,
    phase: String,
    bytes_received: u64,
    total_bytes: Option<u64>,
    message: String,
}

fn emit(app: &AppHandle, payload: ModelDownloadProgress) {
    let _ = app.emit("model-download-progress", &payload);
}

/// Resolve directory for ggml `.bin` files (override or `app_data/models`).
pub fn resolve_models_dir(app: &AppHandle, override_dir: Option<&str>) -> Result<PathBuf, String> {
    if let Some(s) = override_dir {
        let t = s.trim();
        if !t.is_empty() {
            return Ok(PathBuf::from(t));
        }
    }
    let base = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("app_data_dir: {e}"))?;
    Ok(base.join("models"))
}

/// Streamed SHA-1 check without loading the whole file into RAM.
pub fn file_matches_sha1(path: &Path, expected_hex: &str) -> Result<bool, String> {
    use std::io::Read;

    let mut f = std::fs::File::open(path).map_err(|e| format!("open model: {e}"))?;
    let mut h = Sha1::new();
    let mut buf = vec![0u8; 1024 * 1024];
    loop {
        let n = f
            .read(&mut buf)
            .map_err(|e| format!("read model: {e}"))?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    let got = hex::encode(h.finalize());
    Ok(got.eq_ignore_ascii_case(expected_hex.trim()))
}

pub async fn download_whisper_model_file(
    app: &AppHandle,
    entry: &'static WhisperModelCatalogEntry,
    models_dir: &Path,
) -> Result<(), String> {
    std::fs::create_dir_all(models_dir).map_err(|e| format!("create models dir: {e}"))?;

    let dest = models_dir.join(entry.file_name);
    if dest.is_file() && file_matches_sha1(&dest, entry.sha1_hex)? {
        emit(
            app,
            ModelDownloadProgress {
                model_id: entry.id.to_string(),
                phase: "done".to_string(),
                bytes_received: dest
                    .metadata()
                    .map(|m| m.len())
                    .unwrap_or(0),
                total_bytes: None,
                message: "Model already present".to_string(),
            },
        );
        return Ok(());
    }

    let partial = dest.with_extension("bin.partial");
    let _ = std::fs::remove_file(&partial);

    emit(
        app,
        ModelDownloadProgress {
            model_id: entry.id.to_string(),
            phase: "downloading".to_string(),
            bytes_received: 0,
            total_bytes: None,
            message: "Starting download…".to_string(),
        },
    );

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(7200))
        .build()
        .map_err(|e| format!("HTTP client: {e}"))?;

    let resp = client
        .get(entry.url)
        .send()
        .await
        .map_err(|e| format!("Download request failed: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("Download HTTP {}", resp.status()));
    }

    let total = resp
        .headers()
        .get(reqwest::header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok());

    let mut stream = resp.bytes_stream();
    let mut file = tokio::fs::File::create(&partial)
        .await
        .map_err(|e| format!("create partial file: {e}"))?;
    let mut hasher = Sha1::new();
    let mut received: u64 = 0;
    let mut last_emit = 0u64;

    while let Some(item) = stream.next().await {
        let chunk = match item {
            Ok(c) => c,
            Err(e) => {
                let _ = tokio::fs::remove_file(&partial).await;
                return Err(format!("download stream: {e}"));
            }
        };
        hasher.update(&chunk);
        file
            .write_all(&chunk)
            .await
            .map_err(|e| format!("write partial: {e}"))?;
        received += chunk.len() as u64;

        if received.saturating_sub(last_emit) > 512 * 1024 || total == Some(received) {
            last_emit = received;
            emit(
                app,
                ModelDownloadProgress {
                    model_id: entry.id.to_string(),
                    phase: "downloading".to_string(),
                    bytes_received: received,
                    total_bytes: total,
                    message: format!("Downloaded {} MiB", received / (1024 * 1024)),
                },
            );
        }
    }

    file.sync_all()
        .await
        .map_err(|e| format!("sync partial: {e}"))?;
    drop(file);

    emit(
        app,
        ModelDownloadProgress {
            model_id: entry.id.to_string(),
            phase: "verifying".to_string(),
            bytes_received: received,
            total_bytes: total,
            message: "Verifying SHA-1…".to_string(),
        },
    );

    let got = hex::encode(hasher.finalize());
    if !got.eq_ignore_ascii_case(entry.sha1_hex) {
        let _ = std::fs::remove_file(&partial);
        return Err(format!(
            "Model checksum mismatch (expected {}, got {}). Upstream file may have changed — update v2t catalog.",
            entry.sha1_hex, got
        ));
    }

    std::fs::rename(&partial, &dest).map_err(|e| format!("rename model file: {e}"))?;

    emit(
        app,
        ModelDownloadProgress {
            model_id: entry.id.to_string(),
            phase: "done".to_string(),
            bytes_received: received,
            total_bytes: total,
            message: "Model ready".to_string(),
        },
    );

    Ok(())
}
