## Video to Text v2.0.20

In-app Help — common cases, speakers, and where every control lives.

### Added

- **Help** in the header (Справка / Help): eight sections — overview, common cases, who spoke (Person 1 / Person 2), transcription modes, what gets saved, YouTube/TikTok cookies, Preferences map, limits
- Open **Preferences** or **Setup guide** from Help without hunting the menu
- Locales: full **English, Russian, Ukrainian**; other UI languages use English help text for now
- README updated with the same case table and control map

### Tip — speaker labels still empty?

1. WebVTT on → Default video language **English** (not Auto) for built-in tinydiarize  
2. Tools & advanced → Label speakers → **Save**  
3. Set **English** on the queue row too, then re-run (old `.vtt` is not rewritten)  
4. For Russian / Ukrainian / any language: use **Speakers (pyannote diarize)** instead  

Details are in Help → Who spoke.

### Installers

| OS | Assets |
|----|--------|
| **Windows** | NSIS `.exe`, MSI `.msi` (x64) |
| **macOS** | DMG: **aarch64** = Apple Silicon, **x86_64** = Intel |
| **Linux** | `.deb` and AppImage (Ubuntu 22.04) |

Close the running app before reinstalling on Windows.

Full changelog: [CHANGELOG.md](https://github.com/vglu/v2t/blob/main/CHANGELOG.md#2020---2026-09-30)
