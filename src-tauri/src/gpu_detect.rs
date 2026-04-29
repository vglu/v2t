//! Best-effort GPU classification for Whisper acceleration auto-detect.
//!
//! On Windows we shell out to PowerShell for `Win32_VideoController` (no extra crate). On
//! macOS / Linux the helper returns no names — `Auto` then resolves to CPU. The classifier
//! itself is platform-agnostic and unit-tested.

use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum GpuKind {
    Nvidia,
    Amd,
    Intel,
    None,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuInfo {
    pub kind: GpuKind,
    pub names: Vec<String>,
}

/// Classify a single adapter name. Order of substrings matters because some boards
/// embed a vendor word for a different IP block (e.g. AMD Ryzen iGPUs may be reported
/// as "AMD Radeon Graphics").
pub fn classify_gpu_name(name: &str) -> GpuKind {
    let n = name.to_ascii_lowercase();
    if n.contains("nvidia")
        || n.contains("geforce")
        || n.contains("rtx")
        || n.contains("gtx")
        || n.contains("quadro")
        || n.contains("tesla")
    {
        return GpuKind::Nvidia;
    }
    if n.contains("amd")
        || n.contains("radeon")
        || n.contains("ryzen")
        || n.contains("instinct")
        || n.contains("rdna")
    {
        return GpuKind::Amd;
    }
    if n.contains("intel")
        || n.contains("iris")
        || n.contains("uhd graphics")
        || n.contains("hd graphics")
        || n.contains(" arc ")
        || n.starts_with("arc ")
    {
        return GpuKind::Intel;
    }
    GpuKind::None
}

/// When the system reports several adapters, prefer the strongest backend.
pub fn classify_gpu_set(names: &[String]) -> GpuKind {
    let mut have_nvidia = false;
    let mut have_amd = false;
    let mut have_intel = false;
    for n in names {
        match classify_gpu_name(n) {
            GpuKind::Nvidia => have_nvidia = true,
            GpuKind::Amd => have_amd = true,
            GpuKind::Intel => have_intel = true,
            GpuKind::None => {}
        }
    }
    if have_nvidia {
        GpuKind::Nvidia
    } else if have_amd {
        GpuKind::Amd
    } else if have_intel {
        GpuKind::Intel
    } else {
        GpuKind::None
    }
}

#[cfg(windows)]
fn detect_gpu_names_windows() -> Vec<String> {
    use std::os::windows::process::CommandExt;
    use std::process::Command;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    // PowerShell is present on every supported Windows. Avoids pulling in a WMI crate
    // (which would add COM/winapi dependencies) for a one-line query.
    let mut cmd = Command::new("powershell");
    cmd.args([
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        "Get-CimInstance Win32_VideoController | Select-Object -ExpandProperty Name",
    ]);
    cmd.creation_flags(CREATE_NO_WINDOW);
    let Ok(out) = cmd.output() else {
        return vec![];
    };
    if !out.status.success() {
        return vec![];
    }
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect()
}

pub fn detect_gpu() -> GpuInfo {
    #[cfg(windows)]
    let names = detect_gpu_names_windows();
    #[cfg(not(windows))]
    let names: Vec<String> = Vec::new();

    let kind = classify_gpu_set(&names);
    GpuInfo { kind, names }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_nvidia_geforce() {
        assert_eq!(
            classify_gpu_name("NVIDIA GeForce RTX 3060 Ti"),
            GpuKind::Nvidia
        );
        assert_eq!(classify_gpu_name("Quadro P2000"), GpuKind::Nvidia);
        assert_eq!(classify_gpu_name("Tesla T4"), GpuKind::Nvidia);
    }

    #[test]
    fn detects_amd_radeon() {
        assert_eq!(classify_gpu_name("AMD Radeon RX 7900 XT"), GpuKind::Amd);
        assert_eq!(
            classify_gpu_name("AMD Radeon(TM) Graphics"),
            GpuKind::Amd
        );
    }

    #[test]
    fn detects_intel_iris() {
        assert_eq!(
            classify_gpu_name("Intel(R) Iris(R) Xe Graphics"),
            GpuKind::Intel
        );
        assert_eq!(classify_gpu_name("Intel UHD Graphics 630"), GpuKind::Intel);
    }

    #[test]
    fn unknown_returns_none() {
        assert_eq!(
            classify_gpu_name("Microsoft Basic Display Adapter"),
            GpuKind::None
        );
        assert_eq!(classify_gpu_name(""), GpuKind::None);
    }

    #[test]
    fn nvidia_wins_over_intel_in_set() {
        let names: Vec<String> = vec![
            "Intel(R) UHD Graphics".to_string(),
            "NVIDIA GeForce RTX 3060 Ti".to_string(),
        ];
        assert_eq!(classify_gpu_set(&names), GpuKind::Nvidia);
    }

    #[test]
    fn empty_returns_none() {
        assert_eq!(classify_gpu_set(&[]), GpuKind::None);
    }

    #[test]
    fn detect_gpu_is_callable_on_all_platforms() {
        let info = detect_gpu();
        // We only assert the call returned something; platform-specific outputs are
        // covered by classify_* unit tests.
        let _ = info.kind;
    }
}
