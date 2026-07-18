## Video to Text v2.0.9

Timed WebVTT quality pass, local **`large-v3`**, and a clearer workbench UI.

### Highlights

- **`.txt` and `.vtt` stay in sync** — plain text is built from the same timed segments as WebVTT
- **Smarter cues** — better wrapping, fewer orphan weak tails, cleaner speaker breaks
- **Local model `large-v3`** (~2.9 GiB) in Preferences (alongside `medium` / `large-v3-turbo`)
- **Workbench UX** — batch-first flow, Preferences sheet, format badges, language overrides

### Transcription quality (benchmark)

~22.6 min English meeting vs Microsoft Teams captions and YouTube auto-captions.  
Overlap = Jaccard on unique word tokens (higher is closer).

| Engine | vs Teams | vs YouTube | Notes |
|--------|----------|------------|-------|
| OpenAI `whisper-1` (HTTP) | **0.90** | **0.90** | ~50 s · ~$0.14 (`$0.006`/min) |
| Local **`large-v3`** (CUDA) | **0.90** | **0.92** | ~1.6 min on RTX 3060 Ti · $0 |
| YouTube auto-captions | 0.89 | — | External reference |
| Local `medium` | 0.83 | 0.85 | Good when disk/VRAM is limited |
| Local `large-v3-turbo` | 0.64 | 0.65 | Faster; more hallucinations on this sample |

Spot-check: Teams / OpenAI / `large-v3` → **bulk upload**; `medium` often → **Bork**.  
Tinydiarize gives Person 1 / Person 2 — not Teams-style real names.

> Prefer **`whisper-1`** (full `verbose_json` segments) over `gpt-4o-transcribe` / `gpt-4o-mini-transcribe` for long meetings — those models hit output-token caps unless you chunk audio yourself.

### Recommendations — high-quality WebVTT

1. Enable **Export timed transcript as WebVTT**; set language when known (`en`).
2. **Best local:** Whisper **`large-v3`** + GPU whisper-cli (CUDA/Vulkan).
3. **Best cloud:** HTTP API → `https://api.openai.com/v1`, model **`whisper-1`**, key in Preferences (OS keychain).
4. Use **`medium`** if disk/VRAM is tight; verify **`large-v3-turbo`** on your own audio before relying on it for long calls.
5. **Label speakers** only when you need turn markers (`small.en-tdrz`, English).
6. Stronger ASR first — don’t use a general LLM as primary STT.
7. After export, `.txt` and `.vtt` wording should match (by design in this release).

### Installers

| OS | Assets |
|----|--------|
| **Windows** | NSIS `.exe`, MSI `.msi` (x64) |
| **macOS** | DMG: **aarch64** = Apple Silicon, **x86_64** = Intel |
| **Linux** | `.deb` and AppImage (Ubuntu 22.04) |

Close the running app before reinstalling on Windows. Put `ffmpeg` / `yt-dlp` next to the executable or use in-app download.

Full changelog: [CHANGELOG.md](https://github.com/vglu/v2t/blob/main/CHANGELOG.md#209---2026-07-18)
