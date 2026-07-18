## Video to Text v2.0.10

Usage **profiles** so newcomers stay simple and power users keep full control.

### Highlights

- **Simple / Quality / Power** presets (+ **Custom** when you edit)
- Onboarding asks *how you’ll use v2t* before tools / STT setup
- Preferences switcher with confirm — **keys, output folder, and tool paths stay**
- Simple keeps **Tools** (ffmpeg / yt-dlp / cookies); hides Vision, speakers, REST, recursive scan
- **Quality** ≈ meeting WebVTT defaults (`large-v3` / `whisper-1`, timed VTT, keep audio)
- **Power** adds keep video, SRT, recursive folders, full advanced surface

### From v2.0.9 (still relevant)

- Timed `.txt` / `.vtt` from one segment source; smarter cues
- Local model catalog includes **`large-v3`**
- ASR benchmark vs Teams / YouTube — prefer `whisper-1` or local `large-v3` for quality VTT

### Recommendations — high-quality WebVTT

1. Pick **Quality** (or **Power**) in Preferences / Setup guide
2. Enable WebVTT; set speech language when known
3. Local: **`large-v3`** + GPU · Cloud: **`whisper-1`** + keychain API key
4. Speakers = Person 1/2 only (tinydiarize); not Teams-style names

### Installers

| OS | Assets |
|----|--------|
| **Windows** | NSIS `.exe`, MSI `.msi` (x64) |
| **macOS** | DMG: **aarch64** = Apple Silicon, **x86_64** = Intel |
| **Linux** | `.deb` and AppImage (Ubuntu 22.04) |

Close the running app before reinstalling on Windows.

Full changelog: [CHANGELOG.md](https://github.com/vglu/v2t/blob/main/CHANGELOG.md#2010---2026-07-18)
