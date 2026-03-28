//! Download `whisper-cli` from the public Homebrew bottle (GHCR) — no `brew` binary required.

use std::path::{Path, PathBuf};

use flate2::read::GzDecoder;
use futures_util::StreamExt;
use serde::Deserialize;
use serde_json::Value;
use sha2::Digest;
use tauri::AppHandle;
use tar::Archive;

use crate::tool_download::{emit, ToolDownloadProgress};

const FORMULA_API: &str = "https://formulae.brew.sh/api/formula/whisper-cpp.json";
const GHCR_TOKEN_URL: &str = "https://ghcr.io/token?service=ghcr.io&scope=";

#[derive(Debug, Deserialize)]
struct GhcrTokenResp {
    token: String,
}

fn brew_bottle_preference_keys() -> Vec<&'static str> {
    match std::env::consts::ARCH {
        "aarch64" => vec!["arm64_tahoe", "arm64_sequoia", "arm64_sonoma"],
        "x86_64" => vec!["sonoma"],
        _ => vec![],
    }
}

fn pick_bottle(files: &Value) -> Result<(String, String), String> {
    for key in brew_bottle_preference_keys() {
        if let Some(entry) = files.get(key) {
            let url = entry
                .get("url")
                .and_then(|v| v.as_str())
                .ok_or_else(|| format!("bottle {key}: missing url"))?;
            let sha = entry
                .get("sha256")
                .and_then(|v| v.as_str())
                .ok_or_else(|| format!("bottle {key}: missing sha256"))?;
            return Ok((url.to_string(), sha.to_lowercase()));
        }
    }
    Err(
        "No Homebrew bottle for this Mac CPU (try Intel Sonoma or Apple Silicon Sequoia/Sonoma)."
            .to_string(),
    )
}

async fn ghcr_anonymous_token(scope: &str) -> Result<String, String> {
    let enc = urlencoding::encode(scope);
    let url = format!("{GHCR_TOKEN_URL}{enc}");
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|e| format!("HTTP client: {e}"))?;
    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("GHCR token: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("GHCR token HTTP {}", resp.status()));
    }
    let t: GhcrTokenResp = resp.json().await.map_err(|e| format!("token JSON: {e}"))?;
    Ok(t.token)
}

async fn download_bottle_file(
    app: &AppHandle,
    blob_url: &str,
    bearer: &str,
    dest: &Path,
    expect_sha: &str,
    label: &str,
) -> Result<(), String> {
    use tokio::io::AsyncWriteExt;
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(7200))
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()
        .map_err(|e| format!("HTTP client: {e}"))?;

    let resp = client
        .get(blob_url)
        .header("Authorization", format!("Bearer {bearer}"))
        .header("Accept", "application/vnd.oci.image.layer.v1.tar+gzip")
        .send()
        .await
        .map_err(|e| format!("GET bottle: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("GET bottle HTTP {}", resp.status()));
    }

    let mut hasher = sha2::Sha256::new();
    let mut stream = resp.bytes_stream();
    let mut file = tokio::fs::File::create(dest)
        .await
        .map_err(|e| format!("create {}: {e}", dest.display()))?;

    let mut received: u64 = 0;
    let mut last_emit = 0u64;

    while let Some(item) = stream.next().await {
        let chunk = match item {
            Ok(c) => c,
            Err(e) => {
                let _ = tokio::fs::remove_file(dest).await;
                return Err(format!("bottle stream: {e}"));
            }
        };
        hasher.update(&chunk);
        file.write_all(&chunk)
            .await
            .map_err(|e| format!("write: {e}"))?;
        received += chunk.len() as u64;
        if received.saturating_sub(last_emit) > 512 * 1024 {
            last_emit = received;
            emit(
                app,
                ToolDownloadProgress::new(
                    label.to_string(),
                    "downloading",
                    received,
                    None,
                    format!("Downloaded {} KiB (Homebrew bottle)", received / 1024),
                ),
            );
        }
    }
    file.sync_all()
        .await
        .map_err(|e| format!("sync: {e}"))?;

    let got = hex::encode(hasher.finalize());
    if got != expect_sha {
        let _ = tokio::fs::remove_file(dest).await;
        return Err(format!(
            "Bottle SHA-256 mismatch (expected {expect_sha}, got {got})"
        ));
    }
    Ok(())
}

fn extract_whisper_cli_from_bottle_tar_gz(archive_path: &Path, dest_cli: &Path) -> Result<(), String> {
    let f = std::fs::File::open(archive_path).map_err(|e| format!("open bottle: {e}"))?;
    let dec = GzDecoder::new(f);
    let mut archive = Archive::new(dec);
    let mut extracted = false;
    for entry in archive.entries().map_err(|e| format!("tar entries: {e}"))? {
        let mut entry = entry.map_err(|e| format!("tar entry: {e}"))?;
        let path = entry.path().map_err(|e| format!("tar path: {e}"))?;
        let path_s = path.to_string_lossy();
        if path_s.ends_with("/bin/whisper-cli") || path.file_name().and_then(|n| n.to_str()) == Some("whisper-cli")
        {
            if let Some(parent) = dest_cli.parent() {
                std::fs::create_dir_all(parent).map_err(|e| format!("mkdir: {e}"))?;
            }
            let mut out = std::fs::File::create(dest_cli)
                .map_err(|e| format!("create {}: {e}", dest_cli.display()))?;
            std::io::copy(&mut entry, &mut out).map_err(|e| format!("copy whisper-cli: {e}"))?;
            extracted = true;
            break;
        }
    }
    if !extracted {
        return Err(
            "whisper-cli not found inside Homebrew bottle (formula layout changed)".to_string(),
        );
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(dest_cli)
            .map_err(|e| format!("stat: {e}"))?
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(dest_cli, perms).map_err(|e| format!("chmod: {e}"))?;
    }
    Ok(())
}

/// Download `whisper-cli` from the official Homebrew bottle (GHCR, anonymous pull token).
pub async fn download_whisper_cli_from_homebrew_bottle(
    app: &AppHandle,
    dest_dir: &Path,
) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dest_dir).map_err(|e| format!("mkdir: {e}"))?;

    emit(
        app,
        ToolDownloadProgress::new(
            "whisper-cli",
            "downloading",
            0,
            None,
            "Fetching Homebrew formula (whisper-cpp)…",
        ),
    );

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|e| format!("HTTP client: {e}"))?;
    let resp = client
        .get(FORMULA_API)
        .send()
        .await
        .map_err(|e| format!("formula GET: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("formula HTTP {}", resp.status()));
    }
    let v: Value = resp.json().await.map_err(|e| format!("formula JSON: {e}"))?;
    let files = v
        .get("bottle")
        .and_then(|b| b.get("stable"))
        .and_then(|s| s.get("files"))
        .ok_or_else(|| "formula: missing bottle.stable.files".to_string())?;
    let (blob_url, sha256) = pick_bottle(files)?;

    emit(
        app,
        ToolDownloadProgress {
            tool: "whisper-cli".to_string(),
            phase: "downloading".to_string(),
            bytes_received: 0,
            total_bytes: None,
            message: "Getting GHCR pull token…".to_string(),
        },
    );
    let scope = "repository:homebrew/core/whisper-cpp:pull";
    let token = ghcr_anonymous_token(scope).await?;

    let tmp = std::env::temp_dir().join(format!(
        "v2t-whisper-bottle-{}.tar.gz",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    ));

    download_bottle_file(app, &blob_url, &token, &tmp, &sha256, "whisper-cli").await?;

    emit(
        app,
        ToolDownloadProgress::new(
            "whisper-cli",
            "extracting",
            tmp.metadata().map(|m| m.len()).unwrap_or(0),
            None,
            "Extracting whisper-cli from bottle…",
        ),
    );

    let dest_cli = dest_dir.join("whisper-cli");
    let tmp_clone = tmp.clone();
    let dest_clone = dest_cli.clone();
    tokio::task::spawn_blocking(move || extract_whisper_cli_from_bottle_tar_gz(&tmp_clone, &dest_clone))
        .await
        .map_err(|e| format!("extract task: {e}"))??;

    let _ = std::fs::remove_file(&tmp);

    if !dest_cli.is_file() {
        return Err("whisper-cli missing after bottle extract".to_string());
    }

    emit(
        app,
        ToolDownloadProgress::new(
            "whisper-cli",
            "done",
            dest_cli.metadata().map(|m| m.len()).unwrap_or(0),
            None,
            "whisper-cli ready. If macOS blocks it: xattr -dr com.apple.quarantine \"path\" (see Privacy & Security).",
        ),
    );

    Ok(dest_cli)
}
