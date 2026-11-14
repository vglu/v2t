## Video to Text v2.0.21

Local Whisper WebVTT no longer dies on backward token clocks — and never throws away the `.txt` when cues fail.

### Fixed

- **Word cue rebuild:** whisper.cpp can mark a later word earlier than the previous one. v2t now repairs word clocks in spoken order and builds cue bounds from min/max across the group, so `end_ms must be greater than start_ms` no longer aborts the job.
- **Soft fallback:** if timed cues still cannot be built → segment-level cues → otherwise keep `.txt` and write a stub `.vtt` with a `VTT skipped` note (warning in the log). Resume treats that stub as complete.
- **Long files / chunk merge:** inverted cue ends after overlap merge are clamped instead of failing the merge.

### Who should upgrade

Anyone running Local Whisper with **Export WebVTT** (especially with **Label speakers**) on long English meetings that previously failed mid-save with `word cue rebuild (Invalid timed segment …)`.

### Installers

| OS | Assets |
|----|--------|
| **Windows** | NSIS `.exe`, MSI `.msi` (x64) |
| **macOS** | DMG: **aarch64** = Apple Silicon, **x86_64** = Intel |
| **Linux** | `.deb` and AppImage (Ubuntu 22.04) |

Close the running app before reinstalling on Windows.

Full changelog: [CHANGELOG.md](https://github.com/vglu/v2t/blob/main/CHANGELOG.md#2021---2026-11-14)
