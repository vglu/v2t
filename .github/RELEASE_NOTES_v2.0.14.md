## Video to Text v2.0.14

Russian (and any language) WebVTT with Person 1 / Person 2 — without forcing English ASR.

### Added

- **Post-ASR pyannote diarize (opt-in):** Whisper keeps your language (`-l ru`); then pyannote assigns Person tags on the same timestamps
- Preferences: HF token (OS keychain), Python path to a pyannote venv, fail policy (default = keep transcript without Person tags)
- Sidecar script: `tools/diarize/run_diarize.py`

### Fixed / safety

- **tinydiarize only when language = English** — no more `(speaking in foreign language)` garbage on Russian jobs
- Preferences **Save** also starts/stops the local REST API server

### Setup for diarize (Windows + CUDA)

1. Install pyannote into a venv — see `tools/diarize/README.md`
2. Accept HF model terms: [pyannote/speaker-diarization-3.1](https://huggingface.co/pyannote/speaker-diarization-3.1)
3. Preferences → Export WebVTT + Speakers (pyannote) → token + Python path → enable

### Installers

| OS | Assets |
|----|--------|
| **Windows** | NSIS `.exe`, MSI `.msi` (x64) |
| **macOS** | DMG: **aarch64** = Apple Silicon, **x86_64** = Intel |
| **Linux** | `.deb` and AppImage (Ubuntu 22.04) |

Close the running app before reinstalling on Windows.

Full changelog: [CHANGELOG.md](https://github.com/vglu/v2t/blob/main/CHANGELOG.md#2014---2026-07-25)
