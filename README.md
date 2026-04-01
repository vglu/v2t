# v2t — Video to Text

A portable desktop application (Tauri 2) that converts **video and audio** → text transcript.
Supports local files, folders, and URLs — including **YouTube, TikTok, and hundreds of other sites**.

[Demo video](https://youtu.be/cInVpU3ErlQ)

---

## How it works

1. **Add sources** — drag & drop files/folders, or paste one URL per line (YouTube, TikTok, …).
2. **Configure** — pick transcription mode, output folder, and language in **Settings**.
3. **Run** — click **Start queue**. The app downloads (if URL), extracts audio via **ffmpeg**, and transcribes. Each job runs sequentially so slow operations (model download, long video) do not block the UI.
4. **Get text** — `.txt` files appear in the output folder you chose.

> **Tip:** Use the queue — add all your sources first, then start processing. The queue can be stopped at any time; already-completed jobs keep their results.

---

## Supported URL sources

Paste one URL per line. Powered by **[yt-dlp](https://github.com/yt-dlp/yt-dlp)**, which supports 1000+ sites, including:

- **YouTube** — videos, Shorts, Live recordings, playlists
- **TikTok** — public videos (`tiktok.com/@user/video/…`) and short links (`vm.tiktok.com/…`)
- **Twitter / X, Instagram, Facebook, Vimeo, Twitch VODs** — and many more
- Any direct link to an audio or video file (`https://…/file.mp3`)

For playlists one queue item may produce multiple transcripts (one `.txt` per track).

> **Note:** TikTok and some platforms may require you to be logged in. Use **Settings → Browser cookies
> source** to let yt-dlp read cookies directly from your browser (see [Browser cookies](#browser-cookies-youtube--tiktok) below).

---

## Browser cookies (YouTube / TikTok)

Some videos require a logged-in session — age-restricted YouTube content, private TikToks, etc.
The app passes `--cookies-from-browser` to yt-dlp so it reads cookies directly from your browser.

**Configure:** Settings → *I'll install ffmpeg / yt-dlp myself* → **Cookies source for yt-dlp**
(also available on the ffmpeg & yt-dlp step of the Setup guide).

| Option | Browser used |
|--------|-------------|
| **Auto** (default) | Edge on Windows · Chrome on macOS · Firefox on Linux |
| Chrome | Google Chrome |
| Brave | Brave Browser |
| Edge | Microsoft Edge |
| Firefox | Mozilla Firefox |
| Disabled | No cookies passed |

**Before using:** make sure you are already **logged in** to YouTube / TikTok in the chosen browser.

> ⚠️ **Chrome, Brave and Edge have two known limitations on Windows:**
>
> 1. **Database lock** — the cookie file is locked while the browser is running. Close it completely before starting the queue ([yt-dlp #7271](https://github.com/yt-dlp/yt-dlp/issues/7271)).
> 2. **App-bound encryption (Chrome 127+)** — since mid-2024 Chromium-based browsers encrypt cookies in a way only the browser process itself can decrypt. yt-dlp cannot read them even when the browser is closed. Error: `Failed to decrypt with DPAPI` ([yt-dlp #10927](https://github.com/yt-dlp/yt-dlp/issues/10927)).
>
> **Firefox does not have either of these limitations** and is the most reliable choice on Windows.
> Log in to YouTube / TikTok in Firefox, then select **Firefox** in the cookies setting.

---

## Transcription modes

Open **Settings → Transcription** to choose:

### HTTP API (cloud) — default

Sends audio to any OpenAI-compatible `/v1/audio/transcriptions` endpoint.

| Setting | Default | Notes |
|---------|---------|-------|
| API base URL | `https://api.openai.com/v1` | Change for Azure OpenAI, Groq, local servers, etc. |
| Model | `whisper-1` | Any model accepted by the endpoint |
| API key | — | Stored in OS credential store — never written to disk |
| Language | auto-detect | ISO 639-1 code, e.g. `en`, `uk`, `de` |

Files larger than ~22 MiB are automatically split by ffmpeg and the text concatenated.

**Getting an API key (OpenAI):** sign up at [platform.openai.com](https://platform.openai.com/) → [API keys](https://platform.openai.com/api-keys) → **Create new secret key**.
Other providers (Azure OpenAI, Groq, etc.) — obtain the key and base URL from their dashboard.

### Local Whisper (whisper.cpp) — offline, no API key

Runs transcription entirely on your machine using **[whisper.cpp](https://github.com/ggml-org/whisper.cpp)**.

**Setup:**
1. Select **Transcription → Local Whisper (whisper.cpp CLI)** in Settings.
2. Place `whisper-cli` next to `v2t.exe`, or enter the full path in Settings.
3. Choose a model and click **Download / verify model** — the file is fetched from Hugging Face and its SHA-1 checksum is verified.

**Available Whisper models:**

| Model | Size | Speed | Quality | Recommended for |
|-------|------|-------|---------|-----------------|
| `tiny` | ~75 MB | Fastest | Basic | Quick drafts, weak hardware |
| `base` | ~142 MB | Fast | Good | General use on older machines |
| `small` | ~466 MB | Moderate | Better | Good balance on modern CPUs |
| `medium` | ~1.5 GB | Slow | High | When accuracy matters |
| `large-v3-turbo` | ~1.5 GB | Fast (GPU) | Best | GPU systems, highest accuracy |

Model files (`ggml-*.bin`) are stored in `app_data_dir/models` by default (configurable in Settings).
After the first download the app works **fully offline**.

### Browser Whisper (WebAssembly)

Runs the Whisper model directly in the WebView — no binary needed. Suitable for short clips; slower than whisper.cpp for long files.

---

## Getting started

### Prerequisites — two external tools

| Tool | How to get |
|------|-----------|
| **ffmpeg** | Click **Settings → Download ffmpeg & yt-dlp for me** (automatic), or download from [ffmpeg.org](https://ffmpeg.org/download.html) and place next to `v2t.exe` |
| **yt-dlp** | Same button, or download from [github.com/yt-dlp/yt-dlp](https://github.com/yt-dlp/yt-dlp/releases) and place next to `v2t.exe` |

### Portable layout (manual)

Place the binaries **next to the app executable**:

| Platform | App | ffmpeg | yt-dlp |
|----------|-----|--------|--------|
| Windows | `v2t.exe` | `ffmpeg.exe` | `yt-dlp.exe` |
| macOS / Linux | `v2t` | `ffmpeg` | `yt-dlp` |

A `bin/` subfolder next to the executable is also recognized. Paths set in **Settings** take priority over auto-discovery.

### Workflow step by step

1. **Launch** `v2t`.
2. **Settings tab** — set output folder, transcription mode, API key (cloud) or whisper-cli path (local), language.
3. **Queue tab** — add sources:
   - Drop files or folders onto the queue area.
   - Paste URLs in the text box (one per line: YouTube, TikTok, …) and press **Add**.
4. Click **Start queue** — jobs process sequentially; progress is shown per job.
5. When done, find your `.txt` files in the output folder.

> **Recommendation:** for long videos or large playlists, add everything to the queue first and let it run unattended. The queue handles slow operations (download, audio extraction, transcription) without blocking the UI, and you can stop/resume at any time.

---

## Limitations

- **File size (cloud API):** OpenAI `whisper-1` has a 25 MB per-request limit. The app automatically splits large files and joins the text.
- **Playlists:** one URL item can produce multiple `.txt` files; use `{track}` in the filename template.
- **Stop queue:** cancels current and pending jobs cleanly — kills ffmpeg/yt-dlp child processes and aborts HTTP requests.

---

## Development

### Prerequisites

- [Node.js](https://nodejs.org/) LTS
- [Rust](https://rustup.rs/) stable
- Windows: [Microsoft C++ Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/)

### Run in dev mode

```bash
npm install
npm run tauri dev
```

Place `ffmpeg.exe` and `yt-dlp.exe` in `src-tauri/target/debug/` for the dev build.

### Production build

```bash
npm run tauri build
```

| Output | Path |
|--------|------|
| Standalone EXE | `src-tauri/target/release/v2t.exe` |
| Windows installer (NSIS) | `src-tauri/target/release/bundle/nsis/` |
| Windows installer (MSI) | `src-tauri/target/release/bundle/msi/` |
| macOS DMG | `src-tauri/target/release/bundle/dmg/` |
| Linux .deb / AppImage | `src-tauri/target/release/bundle/deb/` and `bundle/appimage/` |

### Tests

```bash
npm run test:run          # Vitest + Testing Library
cd src-tauri && cargo test  # Rust unit tests
npm run e2e               # Playwright (starts dev server)
```

---

## Releases & CI (GitHub Actions)

| Workflow | Trigger | What it does |
|----------|---------|--------------|
| **CI** | push / PR to `main` | Build, Vitest, cargo test |
| **Release** | push of a `v*` tag | Builds Windows (NSIS/MSI), macOS (aarch64 + x86_64 DMG), Linux (.deb + AppImage) → uploads to GitHub Releases |

**How to release:**

1. Update version in `package.json`, `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json`.
2. Add an entry to `CHANGELOG.md`.
3. Commit, then: `git tag v1.3.0 && git push origin v1.3.0`.

If the release job fails with *Resource not accessible by integration*: GitHub → **Settings → Actions → General → Workflow permissions** → **Read and write permissions**.

See also: [`docs/RELEASE.md`](docs/RELEASE.md) · [`CHANGELOG.md`](CHANGELOG.md)
