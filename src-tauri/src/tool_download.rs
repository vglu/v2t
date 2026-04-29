//! One-click download of ffmpeg + yt-dlp into `app_data_dir/v2t/bin` (Windows + macOS).
//! URLs and SHA-256 expectations live in `tools.manifest.json` (see `tool_manifest.rs`, `include_str!`).
//! Also: `whisper-cli` on Windows from [ggml-org/whisper.cpp](https://github.com/ggml-org/whisper.cpp) zip (MIT).
//! On macOS, no darwin CLI zip in upstream releases — we search PATH / Homebrew via `locate_whisper_cli_macos`.

use std::path::{Path, PathBuf};

#[cfg(any(windows, target_os = "macos"))]
use futures_util::StreamExt;
use serde::Serialize;
#[cfg(any(windows, target_os = "macos"))]
use sha2::Digest;
use tauri::path::BaseDirectory;
use tauri::AppHandle;
use tauri::Emitter;
use tauri::Manager;

#[cfg(any(windows, target_os = "macos"))]
use crate::tool_manifest::tools_manifest;

use crate::settings::WhisperAcceleration;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ToolDownloadProgress {
    tool: String,
    phase: String,
    bytes_received: u64,
    total_bytes: Option<u64>,
    message: String,
}

impl ToolDownloadProgress {
    /// Used from `whisper_bottle_macos`; on Windows/Linux this crate still compiles `tool_download` without that module.
    #[allow(dead_code)]
    pub(crate) fn new(
        tool: impl Into<String>,
        phase: impl Into<String>,
        bytes_received: u64,
        total_bytes: Option<u64>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            tool: tool.into(),
            phase: phase.into(),
            bytes_received,
            total_bytes,
            message: message.into(),
        }
    }
}

pub(crate) fn emit(app: &AppHandle, payload: ToolDownloadProgress) {
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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadedWhisperCli {
    pub whisper_cli_path: String,
}

/// Result returned to the frontend after Deno installation.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledDeno {
    /// Value to set as `ytDlpJsRuntimes` in settings.
    pub js_runtimes: String,
}

/// User-visible Documents folder (OS standard).
pub fn default_documents_dir(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .resolve("", BaseDirectory::Document)
        .map_err(|e| format!("resolve Document: {e}"))
}

pub async fn download_managed_media_tools(
    _app: &AppHandle,
) -> Result<DownloadedMediaTools, String> {
    #[cfg(windows)]
    {
        return download_media_tools_inner(_app).await;
    }
    #[cfg(target_os = "macos")]
    {
        return download_media_tools_inner(_app).await;
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        Err(
            "Automatic download of ffmpeg / yt-dlp is available on Windows and macOS only. Install via your package manager and set paths in Settings."
                .to_string(),
        )
    }
}

/// Variant-aware install of `whisper-cli`.
///
/// **Windows:** downloads and extracts the official whisper.cpp zip for the requested
/// `acceleration` into `app_data_dir/v2t/bin/<variant>/`. Variants:
/// - `Auto` → resolves to `Cuda` if NVIDIA is detected, else `Cpu`.
/// - `Cpu` → `whisper-bin-x64.zip` (CPU baseline).
/// - `Cuda` → `whisper-cublas-12.4.0-bin-x64.zip` (NVIDIA, ships cuBLAS DLLs).
/// - `Vulkan` → `whisper-vulkan-bin-x64.zip` (NVIDIA / AMD / Intel via Vulkan).
///
/// **macOS:** no official CLI zip in upstream releases; we install the Homebrew bottle
/// (or fall back to PATH search). `acceleration` is ignored — Apple Silicon already uses
/// Metal automatically through the bottled binary.
///
/// **Linux:** not supported here — use distro packages or build from source.
pub async fn download_whisper_cli_managed(
    _app: &AppHandle,
    _acceleration: WhisperAcceleration,
) -> Result<DownloadedWhisperCli, String> {
    #[cfg(windows)]
    {
        let resolved = resolve_whisper_acceleration(_acceleration);
        return download_whisper_cli_windows(_app, resolved).await;
    }
    #[cfg(target_os = "macos")]
    {
        return download_whisper_cli_macos_bottle_then_fallback(_app).await;
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        Err(
            "Automatic whisper-cli setup is available on Windows (download) and macOS (Homebrew detection). On Linux install whisper.cpp from your package manager."
                .to_string(),
        )
    }
}

/// Resolve `Auto` against the current host: NVIDIA → CUDA, otherwise CPU. CUDA/Vulkan/CPU
/// pass through unchanged. Vulkan is never auto-selected — the user opts in explicitly.
pub fn resolve_whisper_acceleration(req: WhisperAcceleration) -> WhisperAcceleration {
    match req {
        WhisperAcceleration::Auto => match crate::gpu_detect::detect_gpu().kind {
            crate::gpu_detect::GpuKind::Nvidia => WhisperAcceleration::Cuda,
            _ => WhisperAcceleration::Cpu,
        },
        other => other,
    }
}

/// Subdirectory under `app_data_dir/v2t/bin/` for each variant. Multiple variants can
/// coexist so L3 fallback can find the CPU build even when the active one is CUDA.
pub fn whisper_variant_subdir(acc: WhisperAcceleration) -> &'static str {
    match acc {
        WhisperAcceleration::Auto | WhisperAcceleration::Cuda => "whisper-cpp-cublas",
        WhisperAcceleration::Vulkan => "whisper-cpp-vulkan",
        WhisperAcceleration::Cpu => "whisper-cpp-cpu",
    }
}

/// `Some(path)` if a previously-installed CPU variant is present (for L3 fallback).
pub fn locate_installed_cpu_whisper_cli(app: &AppHandle) -> Option<PathBuf> {
    let base = managed_bin_dir(app).ok()?;
    let exe_name = if cfg!(windows) { "whisper-cli.exe" } else { "whisper-cli" };
    let candidates = [
        base.join(whisper_variant_subdir(WhisperAcceleration::Cpu)).join(exe_name),
        // Legacy installs (pre-1.5.0-rc2) extracted to "whisper-cpp/" without a variant suffix.
        base.join("whisper-cpp").join(exe_name),
    ];
    candidates.into_iter().find(|p| p.is_file())
}

/// Download Deno into the managed bin dir so yt-dlp can use it as JS runtime.
/// Windows: `deno.exe` from GitHub release zip. macOS: `deno` from release zip.
pub async fn install_deno_managed(
    _app: &AppHandle,
) -> Result<InstalledDeno, String> {
    #[cfg(any(windows, target_os = "macos"))]
    {
        return install_deno_inner(_app).await;
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        Err(
            "Automatic Deno download is available on Windows and macOS only. Install via your package manager (e.g. `curl -fsSL https://deno.land/install.sh | sh`) and set JS runtimes to 'deno'."
                .to_string(),
        )
    }
}

#[cfg(any(windows, target_os = "macos"))]
async fn install_deno_inner(app: &AppHandle) -> Result<InstalledDeno, String> {
    let base = managed_bin_dir(app)?;
    std::fs::create_dir_all(&base).map_err(|e| format!("create bin dir: {e}"))?;

    let deno_exe_name = if cfg!(windows) { "deno.exe" } else { "deno" };
    let deno_dest = base.join(deno_exe_name);

    let zip_url = if cfg!(windows) {
        "https://github.com/denoland/deno/releases/latest/download/deno-x86_64-pc-windows-msvc.zip"
    } else if cfg!(target_arch = "aarch64") {
        "https://github.com/denoland/deno/releases/latest/download/deno-aarch64-apple-darwin.zip"
    } else {
        "https://github.com/denoland/deno/releases/latest/download/deno-x86_64-apple-darwin.zip"
    };

    let tmp_zip = std::env::temp_dir().join(format!(
        "v2t-deno-{}.zip",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    ));

    emit(
        app,
        ToolDownloadProgress {
            tool: "deno".to_string(),
            phase: "downloading".to_string(),
            bytes_received: 0,
            total_bytes: None,
            message: "Downloading Deno runtime from GitHub…".to_string(),
        },
    );

    // No SHA check — using HTTPS redirect to latest release.
    download_file_streaming(app, zip_url, &tmp_zip, "deno", "").await?;

    emit(
        app,
        ToolDownloadProgress {
            tool: "deno".to_string(),
            phase: "extracting".to_string(),
            bytes_received: tmp_zip.metadata().map(|m| m.len()).unwrap_or(0),
            total_bytes: None,
            message: format!("Extracting {deno_exe_name}…"),
        },
    );

    let dest = deno_dest.clone();
    let zip_path = tmp_zip.clone();
    let exe_name = deno_exe_name.to_string();
    tokio::task::spawn_blocking(move || {
        use std::fs::File;
        use zip::ZipArchive;

        let f = File::open(&zip_path).map_err(|e| format!("open zip: {e}"))?;
        let mut archive = ZipArchive::new(f).map_err(|e| format!("read zip: {e}"))?;

        for i in 0..archive.len() {
            let mut file = archive.by_index(i).map_err(|e| format!("zip entry: {e}"))?;
            let name = file.name().replace('\\', "/");
            // The zip contains just "deno.exe" (Windows) or "deno" (macOS) at the root.
            let basename = name.rsplit('/').next().unwrap_or(&name);
            if basename.eq_ignore_ascii_case(&exe_name) {
                let mut out = File::create(&dest)
                    .map_err(|e| format!("create {}: {e}", dest.display()))?;
                std::io::copy(&mut file, &mut out)
                    .map_err(|e| format!("write {}: {e}", dest.display()))?;

                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o755));
                }

                return Ok(());
            }
        }
        Err(format!("{exe_name} not found inside the Deno release zip"))
    })
    .await
    .map_err(|e| format!("extract task: {e}"))??;

    let _ = std::fs::remove_file(&tmp_zip);

    if !deno_dest.is_file() {
        return Err(format!("{deno_exe_name} missing after extract"));
    }

    emit(
        app,
        ToolDownloadProgress {
            tool: "deno".to_string(),
            phase: "done".to_string(),
            bytes_received: deno_dest.metadata().map(|m| m.len()).unwrap_or(0),
            total_bytes: None,
            message: format!("Deno installed to {}", deno_dest.display()),
        },
    );

    Ok(InstalledDeno {
        js_runtimes: "deno".to_string(),
    })
}

#[cfg(windows)]
async fn download_whisper_cli_windows(
    app: &AppHandle,
    acceleration: WhisperAcceleration,
) -> Result<DownloadedWhisperCli, String> {
    let base = managed_bin_dir(app)?;
    std::fs::create_dir_all(&base).map_err(|e| format!("create bin dir: {e}"))?;
    let subdir = whisper_variant_subdir(acceleration);
    let dest_dir = base.join(subdir);
    if dest_dir.exists() {
        let _ = std::fs::remove_dir_all(&dest_dir);
    }
    std::fs::create_dir_all(&dest_dir).map_err(|e| format!("create {subdir} dir: {e}"))?;

    let tmp_zip = std::env::temp_dir().join(format!(
        "v2t-whisper-{subdir}-{}.zip",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    ));

    let m = tools_manifest();
    let (entry, label) = match acceleration {
        WhisperAcceleration::Cuda | WhisperAcceleration::Auto => {
            (&m.windows_whisper_zip_cublas, "CUDA / cuBLAS")
        }
        WhisperAcceleration::Vulkan => (&m.windows_whisper_zip_vulkan, "Vulkan"),
        WhisperAcceleration::Cpu => (&m.windows_whisper_zip_cpu, "CPU"),
    };

    emit(
        app,
        ToolDownloadProgress {
            tool: "whisper-cli".to_string(),
            phase: "downloading".to_string(),
            bytes_received: 0,
            total_bytes: None,
            message: format!(
                "Downloading whisper.cpp Windows bundle ({label}, ggml-org/whisper.cpp, MIT)…"
            ),
        },
    );

    download_file_streaming(
        app,
        &entry.url,
        &tmp_zip,
        "whisper-cli",
        &entry.sha256,
    )
    .await?;

    emit(
        app,
        ToolDownloadProgress {
            tool: "whisper-cli".to_string(),
            phase: "extracting".to_string(),
            bytes_received: tmp_zip.metadata().map(|m| m.len()).unwrap_or(0),
            total_bytes: None,
            message: "Extracting whisper-cli.exe and DLLs…".to_string(),
        },
    );

    let zip_path = tmp_zip.clone();
    let dest = dest_dir.clone();
    tokio::task::spawn_blocking(move || extract_whisper_cpp_windows_zip(&zip_path, &dest))
        .await
        .map_err(|e| format!("extract task: {e}"))??;

    let _ = std::fs::remove_file(&tmp_zip);

    let exe = dest_dir.join("whisper-cli.exe");
    if !exe.is_file() {
        return Err(
            "whisper-cli.exe missing after extract (upstream zip layout may have changed)".to_string(),
        );
    }

    emit(
        app,
        ToolDownloadProgress {
            tool: "whisper-cli".to_string(),
            phase: "done".to_string(),
            bytes_received: exe.metadata().map(|m| m.len()).unwrap_or(0),
            total_bytes: None,
            message: "whisper-cli ready (keep DLLs in the same folder)".to_string(),
        },
    );

    Ok(DownloadedWhisperCli {
        whisper_cli_path: exe.to_string_lossy().into_owned(),
    })
}

#[cfg(windows)]
fn extract_whisper_cpp_windows_zip(zip_path: &Path, dest_dir: &Path) -> Result<(), String> {
    use std::fs::File;
    use zip::ZipArchive;

    let f = File::open(zip_path).map_err(|e| format!("open zip: {e}"))?;
    let mut archive = ZipArchive::new(f).map_err(|e| format!("read zip: {e}"))?;

    for i in 0..archive.len() {
        let mut file = archive.by_index(i).map_err(|e| format!("zip entry: {e}"))?;
        let raw = file.name().replace('\\', "/");
        if raw.ends_with('/') {
            continue;
        }
        let Some(rel) = raw
            .strip_prefix("Release/")
            .or_else(|| raw.strip_prefix("release/"))
        else {
            continue;
        };
        if rel.is_empty() {
            continue;
        }

        let out_path = dest_dir.join(rel);
        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("mkdir: {e}"))?;
        }
        let mut out = std::fs::File::create(&out_path)
            .map_err(|e| format!("create {}: {e}", out_path.display()))?;
        std::io::copy(&mut file, &mut out).map_err(|e| format!("write {}: {e}", rel))?;
    }

    Ok(())
}

#[cfg(target_os = "macos")]
async fn download_whisper_cli_macos_bottle_then_fallback(
    app: &AppHandle,
) -> Result<DownloadedWhisperCli, String> {
    let base = managed_bin_dir(app)?;
    let dest_dir = base.join("whisper-cpp");
    if dest_dir.exists() {
        let _ = std::fs::remove_dir_all(&dest_dir);
    }
    std::fs::create_dir_all(&dest_dir).map_err(|e| format!("create whisper-cpp dir: {e}"))?;

    match crate::whisper_bottle_macos::download_whisper_cli_from_homebrew_bottle(app, &dest_dir).await
    {
        Ok(p) => Ok(DownloadedWhisperCli {
            whisper_cli_path: p.to_string_lossy().into_owned(),
        }),
        Err(bottle_err) => match locate_whisper_cli_macos(app) {
            Ok(p) => Ok(p),
            Err(find_err) => Err(format!(
                "Could not install whisper-cli from Homebrew bottle:\n{bottle_err}\n\nTried PATH/Homebrew search:\n{find_err}"
            )),
        },
    }
}

#[cfg(target_os = "macos")]
fn locate_whisper_cli_macos(app: &AppHandle) -> Result<DownloadedWhisperCli, String> {
    emit(
        app,
        ToolDownloadProgress {
            tool: "whisper-cli".to_string(),
            phase: "searching".to_string(),
            bytes_received: 0,
            total_bytes: None,
            message: "Searching PATH (which whisper-cli, whisper, main) and Homebrew layouts…".to_string(),
        },
    );

    let Some(found) = crate::deps::macos_search_whisper_cli_in_path() else {
        return Err(
            "whisper-cli not found. Checked: /usr/bin/which whisper-cli, whisper, main; \
             /opt/homebrew and /usr/local bin + whisper-cpp keg paths; Linuxbrew ~/.linuxbrew. \
             Install: brew install whisper-cpp — then use this button again or Pick file…"
                .to_string(),
        );
    };

    let path_str = found.to_string_lossy().into_owned();
    emit(
        app,
        ToolDownloadProgress {
            tool: "whisper-cli".to_string(),
            phase: "done".to_string(),
            bytes_received: found.metadata().map(|m| m.len()).unwrap_or(0),
            total_bytes: None,
            message: format!("Found {}", path_str),
        },
    );

    Ok(DownloadedWhisperCli {
        whisper_cli_path: path_str,
    })
}

#[cfg(windows)]
async fn download_media_tools_inner(app: &AppHandle) -> Result<DownloadedMediaTools, String> {
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

    let m = tools_manifest();
    download_file_streaming(
        app,
        &m.windows_yt_dlp_exe.url,
        &yt_dest,
        "yt-dlp",
        &m.windows_yt_dlp_exe.sha256,
    )
    .await?;

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

    download_file_streaming(
        app,
        &m.windows_ffmpeg_zip.url,
        &tmp_zip,
        "ffmpeg",
        &m.windows_ffmpeg_zip.sha256,
    )
    .await?;

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

#[cfg(target_os = "macos")]
async fn download_media_tools_inner(app: &AppHandle) -> Result<DownloadedMediaTools, String> {
    let dir = managed_bin_dir(app)?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("create bin dir: {e}"))?;

    let yt_dest = dir.join("yt-dlp");
    let ff_dest = dir.join("ffmpeg");
    let m = tools_manifest();
    let ff_entry = match std::env::consts::ARCH {
        "aarch64" => &m.macos_ffmpeg_darwin_arm64,
        "x86_64" => &m.macos_ffmpeg_darwin_x64,
        other => {
            return Err(format!(
                "Unsupported macOS CPU architecture for bundled FFmpeg: {other}"
            ));
        }
    };

    emit(
        app,
        ToolDownloadProgress {
            tool: "yt-dlp".to_string(),
            phase: "downloading".to_string(),
            bytes_received: 0,
            total_bytes: None,
            message: "Downloading yt-dlp (macOS)…".to_string(),
        },
    );

    download_file_streaming(
        app,
        &m.macos_yt_dlp.url,
        &yt_dest,
        "yt-dlp",
        &m.macos_yt_dlp.sha256,
    )
    .await?;
    make_executable(&yt_dest)?;

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

    emit(
        app,
        ToolDownloadProgress {
            tool: "ffmpeg".to_string(),
            phase: "downloading".to_string(),
            bytes_received: 0,
            total_bytes: None,
            message: "Downloading FFmpeg (static build, may take a while)…".to_string(),
        },
    );

    download_file_streaming(
        app,
        &ff_entry.url,
        &ff_dest,
        "ffmpeg",
        &ff_entry.sha256,
    )
    .await?;
    make_executable(&ff_dest)?;

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

#[cfg(unix)]
fn make_executable(path: &Path) -> Result<(), String> {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    let mut perms = fs::metadata(path)
        .map_err(|e| format!("stat {}: {e}", path.display()))?
        .permissions();
    perms.set_mode(0o755);
    fs::set_permissions(path, perms).map_err(|e| format!("chmod {}: {e}", path.display()))
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

#[cfg(any(windows, target_os = "macos"))]
async fn download_file_streaming(
    app: &AppHandle,
    url: &str,
    dest: &Path,
    tool_label: &str,
    sha256_hex: &str,
) -> Result<(), String> {
    use tokio::fs::File;
    use tokio::io::AsyncWriteExt;

    let expect = sha256_hex.trim().to_ascii_lowercase();
    let mut hasher = if expect.is_empty() {
        None
    } else {
        Some(sha2::Sha256::new())
    };

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
        if let Some(ref mut h) = hasher {
            h.update(&chunk);
        }
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

    if let Some(h) = hasher {
        let got = hex::encode(h.finalize());
        if got != expect {
            let _ = tokio::fs::remove_file(dest).await;
            return Err(format!(
                "{tool_label}: SHA-256 mismatch (expected {expect}, got {got}). Remove partial file and retry, or update tools.manifest.json if upstream changed."
            ));
        }
    }

    Ok(())
}
