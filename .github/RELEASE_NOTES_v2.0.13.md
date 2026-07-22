## Video to Text v2.0.13

Playlist video save fix and sensible defaults for kept downloads.

### Fixes

- **Keep downloaded video + playlists:** every playlist entry is saved as its own `.mp4` (no more overwrite into a single path)
- Playlist videos go under **`output/<playlist title>/`**

### Changes

- Kept videos prefer **≤720p** instead of unrestricted “best”

### From v2.0.12

- yt-dlp cookie retry without browser cookies when Chrome/Edge DB fails
- Auto cookies on Windows prefer Firefox
- In-app WASM capped away from `large-v3` OrtRun crashes

### Installers

| OS | Assets |
|----|--------|
| **Windows** | NSIS `.exe`, MSI `.msi` (x64) |
| **macOS** | DMG: **aarch64** = Apple Silicon, **x86_64** = Intel |
| **Linux** | `.deb` and AppImage (Ubuntu 22.04) |

Close the running app before reinstalling on Windows.

Full changelog: [CHANGELOG.md](https://github.com/vglu/v2t/blob/main/CHANGELOG.md#2013---2026-07-22)
