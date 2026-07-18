## Video to Text v2.0.12

Reliability fixes for YouTube downloads and in-app Whisper.

### Fixes

- **yt-dlp cookies:** on Chrome/Edge cookie DB failures, automatically **retry without cookies**; clearer guidance to use Firefox or Disabled
- **Auto cookies (Windows):** default browser is **Firefox** (Chrome/Edge encryption breaks `--cookies-from-browser`)
- **In-app WASM:** `large-v3` / turbo removed from the in-app model list (too heavy for WebView2 / OrtRun); use **On this computer** + whisper-cli for large-v3 quality

### From v2.0.11

- Light Setup guide with profile-branched paths
- Preferences profile bar / scroll layout fixes

### Installers

| OS | Assets |
|----|--------|
| **Windows** | NSIS `.exe`, MSI `.msi` (x64) |
| **macOS** | DMG: **aarch64** = Apple Silicon, **x86_64** = Intel |
| **Linux** | `.deb` and AppImage (Ubuntu 22.04) |

Close the running app before reinstalling on Windows.

Full changelog: [CHANGELOG.md](https://github.com/vglu/v2t/blob/main/CHANGELOG.md#2012---2026-07-18)
