## Video to Text v2.0.11

Polish pass on **Preferences** and the **Setup guide** after profiles landed in 2.0.10.

### Highlights

- **Setup guide** matches the light workbench / Preferences look (no more dark modal island)
- Profile chips drive a **branched** tour:
  - **Simple** — short path; in-app Whisper by default; skips Deno/cookies
  - **Quality / Power** — tools → mode → engine; Power keeps advanced Deno/cookies
- Escape closes the guide; **Finish later** marks setup complete
- Output step auto-fills **Documents** when empty

### Fixes

- Preferences: profile bar no longer overlaps the General / Transcription / Tools tabs
- Preferences: settings scroll works again at the real 800×600 window size

### From v2.0.10

- Simple / Quality / Power (+ Custom) presets in Preferences
- Keys, output folder, and tool paths preserved when switching profiles

### Installers

| OS | Assets |
|----|--------|
| **Windows** | NSIS `.exe`, MSI `.msi` (x64) |
| **macOS** | DMG: **aarch64** = Apple Silicon, **x86_64** = Intel |
| **Linux** | `.deb` and AppImage (Ubuntu 22.04) |

Close the running app before reinstalling on Windows.

Full changelog: [CHANGELOG.md](https://github.com/vglu/v2t/blob/main/CHANGELOG.md#2011---2026-07-18)
