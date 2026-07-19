# v2t — Video to Text

Portable desktop app (**Tauri 2**) that turns **video / audio / links** into text transcripts (`.txt`, optional `.vtt` / `.srt`).

**Current release: [v2.0.12](https://github.com/vglu/v2t/releases/tag/v2.0.12)** (2026-07-18)

[Demo video](https://youtu.be/cInVpU3ErlQ) · [Changelog](CHANGELOG.md) · [Releases](https://github.com/vglu/v2t/releases)

---

## Status (what works today)

| Area | Status |
|------|--------|
| Queue: files, folders, YouTube / TikTok / playlists | ✅ |
| ffmpeg + yt-dlp (download from Preferences, or next to the app) | ✅ |
| Profiles **Simple / Quality / Power** (+ Custom) | ✅ |
| Light **Preferences** sheet + **Setup guide** (profile-branched) | ✅ |
| Cloud HTTP Whisper API / local whisper.cpp / in-app WASM | ✅ |
| Timed **WebVTT**, optional speakers (local tinydiarize) | ✅ |
| yt-dlp cookie DB failure → **auto-retry without cookies** | ✅ (v2.0.12) |
| In-app WASM **large-v3** | ❌ Not supported in webview — use **On this computer** |
| Dependabot auto-PRs | ❌ Disabled (manual updates only) |

---

## How it works

1. **Add sources** — paste links, or use **Files & folders** (drop zone).
2. **Preferences** — output folder, profile, transcription mode, tools/cookies.
3. **Start batch** — downloads (URLs), extracts audio with **ffmpeg**, transcribes.
4. **Results** — `.txt` (and optional siblings) in your output folder.

First launch: **Setup guide** (or reopen from the header). Profiles gate how long setup is:

| Profile | Intent | Setup length |
|---------|--------|--------------|
| **Simple** | Links → plain text, captions when available | Short; in-app Whisper by default |
| **Quality** | Meetings / timed WebVTT, strong model | Tools + engine setup |
| **Power** | Quality + keep media, folders, SRT, Deno/cookies | Full advanced tools |

API keys, output folder, and tool paths are **never overwritten** when you switch profiles.

---

## Supported URL sources

One URL per line. Powered by **[yt-dlp](https://github.com/yt-dlp/yt-dlp)** (1000+ sites):

- **YouTube** — videos, Shorts, playlists  
- **TikTok** — public videos and short links (`vm.tiktok.com`, …)  
- Twitter/X, Instagram, Facebook, Vimeo, Twitch VODs, …  
- Direct media URLs (`https://…/file.mp3`)

Playlists can produce multiple transcripts (one `.txt` per track). Use `{track}` in the filename template if needed.

---

## Browser cookies (YouTube / TikTok) — troubleshooting

Some videos need a logged-in session (age-gated YouTube, login walls, etc.).  
v2t passes yt-dlp `--cookies-from-browser` when cookies are enabled.

### Where to set it

**Preferences → Tools & advanced** → scroll **below** “Download ffmpeg & yt-dlp” →  
**Cookies source for yt-dlp (YouTube / TikTok age-gate)** → **Save changes**.

(Also on the Power Setup guide tools step.)

### Options

| Option | What yt-dlp uses |
|--------|------------------|
| **Auto** (default) | **Firefox** on Windows/Linux · Chrome on macOS |
| Chrome / Brave / Edge | That Chromium browser |
| **Firefox** | Mozilla Firefox (recommended on Windows) |
| **Disabled** | No `--cookies-from-browser` |

### Recommended setup on Windows (Firefox)

1. Install [Firefox](https://www.mozilla.org/firefox/).
2. Open Firefox → log in to **YouTube** (and TikTok if needed) in a normal window.
3. Optionally **fully quit Firefox** before a long batch (avoids rare DB locks while copying `cookies.sqlite`).
4. In v2t: Preferences → Tools & advanced → cookies → **Firefox** → Save.
5. Run the queue again.

Firefox stores cookies in a plain SQLite DB that yt-dlp can read. That is why it works when Chrome does not.

### Why Chrome / Edge / Brave fail on Windows

Two separate problems (you may see either):

1. **Cookie database lock** while the browser is open  
   Error often looks like:  
   `Could not copy Chrome cookie database`  
   See [yt-dlp #7271](https://github.com/yt-dlp/yt-dlp/issues/7271).  
   Closing the browser sometimes helps for the *lock* only — it does **not** fix encryption below.

2. **App-bound encryption (Chrome 127+, mid‑2024+)**  
   Cookies are encrypted so only Chrome/Edge itself can decrypt them.  
   yt-dlp **cannot** read them even with the browser closed.  
   Typical errors: `Failed to decrypt with DPAPI`, or copy/decrypt failures.  
   See [yt-dlp #10927](https://github.com/yt-dlp/yt-dlp/issues/10927).

**There is no reliable “just close Chrome” fix** for (2). Use Firefox, export a cookies file for yt-dlp manually, or disable cookies for public videos.

### What v2t does automatically (v2.0.12+)

If the first yt-dlp pass fails with a cookie-database / decrypt error, the app **retries the same download without cookies** and logs something like:

`Browser cookies failed (Chrome/Edge lock or encryption). Retrying without cookies…`

- **Public** videos often succeed on that retry.  
- **Age-gated / login-required** videos still need working cookies (Firefox) or will fail again with a clearer message pointing to Preferences.

### Quick decision guide

| Symptom | Do this |
|---------|---------|
| `Could not copy Chrome cookie database` | Set cookies to **Firefox** or **Disabled**; Save; retry |
| Public video, cookies failing | **Disabled** is fine (or rely on auto-retry) |
| Age-restricted / “Sign in to confirm” | Firefox + logged-in YouTube session |
| Still failing after Firefox | Update yt-dlp (Preferences → Download ffmpeg & yt-dlp), confirm login in Firefox, try again |
| yt-dlp “older than 90 days” warning | Re-download yt-dlp from Preferences (optional but recommended) |

### Deno / JS runtimes (separate from cookies)

Some YouTube extractions paths need a JS runtime (`deno` / `node`).  
If you see *no supported JavaScript runtime*, install Deno and set **yt-dlp JS runtimes** to `deno` in the same Tools section (Power profile / advanced).

---

## Transcription modes

**Preferences → Transcription** (or General → transcription mode):

### Online service (HTTP API)

OpenAI-compatible `POST …/audio/transcriptions`. Key stored in the **OS credential store**.

| Setting | Typical default |
|---------|-----------------|
| API base URL | `https://api.openai.com/v1` |
| Model | `whisper-1` |
| Language | auto or ISO code (`en`, `uk`, …) |

Large files are split automatically when the provider has a size limit.

### On this computer (whisper.cpp / whisper-cli) — best quality

Fully local. For meeting-grade quality use model **`large-v3`** (+ GPU build when available).

| Model | Approx. size | Notes |
|-------|--------------|--------|
| `tiny` / `base` / `small` | small | Drafts / weak hardware |
| `medium` | ~1.5 GB | Solid CPU default |
| `large-v3` | ~2.9 GB | **Best local quality** (benchmarks ≈ cloud `whisper-1`) |
| `large-v3-turbo` | ~1.5 GB | Faster; verify on your audio (can hallucinate) |

Download whisper-cli + model from Preferences when on Windows/macOS.

### Inside the app (WASM / Transformers.js)

No whisper-cli. Fine for **short** clips.  
**Do not use `large-v3` here** — WebView2 / ONNX often crashes (`OrtRun error code = 1`).  
In-app list is capped at **medium** (v2.0.12+). For quality, switch to **On this computer**.

---

## Getting started

### Tools

| Tool | How |
|------|-----|
| **ffmpeg** + **yt-dlp** | Preferences → Tools & advanced → **Download ffmpeg & yt-dlp for me**, or place binaries next to `v2t.exe` |

### Typical workflow

1. Launch **v2t** (complete Setup guide if shown).  
2. Pick a **profile** (or leave Custom).  
3. Set output folder; choose transcription mode.  
4. Add URLs / files → **Start batch**.  
5. Open the output folder for `.txt` / `.vtt`.

---

## Limitations

- **Cloud file size:** providers like OpenAI `whisper-1` limit request size; the app splits and merges.  
- **Playlists:** one queue row → many transcripts.  
- **Stop batch:** cancels running/pending jobs and kills child processes.  
- **Chromium cookies on Windows:** unreliable; prefer Firefox (see above).  
- **In-app WASM:** not for large models or long heavy jobs.

---

## Development

### Prerequisites

- [Node.js](https://nodejs.org/) LTS  
- [Rust](https://rustup.rs/) stable  
- Windows: [MSVC Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/)

```bash
npm install
npm run tauri dev
```

Place `ffmpeg` / `yt-dlp` under `src-tauri/target/debug/` for URL tests in dev.

```bash
npm run tauri build
```

| Output | Path |
|--------|------|
| EXE | `src-tauri/target/release/v2t.exe` |
| NSIS / MSI | `src-tauri/target/release/bundle/nsis/` · `bundle/msi/` |
| macOS DMG | `src-tauri/target/release/bundle/dmg/` |
| Linux | `bundle/deb/` · `bundle/appimage/` |

### Tests

```bash
npm run test:run
cd src-tauri && cargo test
npm run e2e
```

---

## Releases & CI

| Workflow | Trigger | Role |
|----------|---------|------|
| **CI** | push / PR to `main` | Build + Vitest + cargo test |
| **Release** | tag `v*` | Windows / macOS / Linux installers → GitHub Release |

```bash
# after bumping version + CHANGELOG
git tag v2.0.12 && git push origin v2.0.12
```

See [`docs/RELEASE.md`](docs/RELEASE.md) · [`CHANGELOG.md`](CHANGELOG.md)
