# Changelog

Формат основан на [Keep a Changelog](https://keepachangelog.com/ru/1.1.0/).

## [1.5.0-rc2] - 2026-04-29

### Добавлено

- **GPU acceleration для local Whisper** (Windows): новая настройка `whisperAcceleration` (`Auto` / `CUDA` / `Vulkan` / `CPU`). На NVIDIA RTX-class GPU `large-v3-turbo` работает ~10-20× быстрее CPU. Расширен `tools.manifest.json` (whisper.cpp v1.8.4 cuBLAS и Vulkan-сборки), скачивание выбирает нужный zip по выбранному backend и распаковывает в per-variant директорию (`whisper-cpp-cublas`, `whisper-cpp-vulkan`, `whisper-cpp-cpu`) — варианты могут сосуществовать.
- **Авто-детект GPU** (`gpu_detect.rs`): WMI-запрос `Win32_VideoController` через PowerShell, классификация Nvidia/Amd/Intel/None, новая Tauri-команда `detect_gpu`. На macOS / Linux команда возвращает `None` (фолбэк на CPU). Хинт показывается в Settings и в шаге Local Whisper мастера первого запуска.
- **CPU fallback** в `whisper_local.rs`: если `whisper-cli` падает с типичной ошибкой инициализации GPU (`cudaGetDeviceCount`, `cuBLAS`, `failed to initialize Vulkan`, …) и установлен CPU-вариант — текущий трек автоматически перезапускается на CPU-build, в лог уходит сообщение `[whisper] GPU init failed, falling back to CPU`.
- В мастере первого запуска при найденной NVIDIA — ненавязчивая отметка «Enable CUDA acceleration (recommended)» в шаге Local Whisper.

### Изменено

- В манифесте `whisper_zip` → `whisper_zip_cpu` (плюс новые `whisper_zip_cublas` / `whisper_zip_vulkan`); `schema_version` поднят до `2`. Для CPU-bundle хэш SHA-256 проверяется как раньше; для cuBLAS / Vulkan upstream-zip пока без хэша (можно вписать позже как для FFmpeg-BtbN).

## [1.5.0-rc1] - 2026-04-29

### Добавлено

- **Heartbeat-watchdog для yt-dlp** (120 сек) вместо wall-clock-таймаута 900 сек: длинные плейлисты докачиваются произвольное время, реально зависший yt-dlp убивается за ~2 мин. Реализация — `pipeline::run_yt_dlp_streaming` с per-line streaming stdout+stderr.
- **Streaming-парсер прогресса yt-dlp** (`yt_dlp_progress.rs`): распознаёт `Downloading item N of M`, `[download] X% of …`, `[ExtractAudio]`, `[Merger]`; эмитит `queue-job-progress` с фазой `yt-dlp` / `yt-dlp-video` (5%-bucket для уменьшения шума).
- **Сборка мусора во временной папке**: модуль `temp_cleanup` сканирует `%TEMP%` на старте приложения и удаляет осиротевшие `v2t-work-*` старше 24 часов (после kill -9 / panic / `delete_audio_after=false`).
- Флаг `--newline` в обоих yt-dlp-вызовах (audio-pass + video-pass) — даёт построчные progress-updates вместо `\r`-перезаписи одной линии.

### Исправлено

- **Windows / video-pass: `OSError: [Errno 22]` на UA/RU заголовках** (`I+ encoding-fix`). Под `Stdio::piped()` Windows назначает stdout кодовую страницу `cp1252`, и Python внутри yt-dlp падает при flush кириллических metadata-строк с exit 120. Теперь у дочернего процесса `PYTHONIOENCODING=utf-8` + флаг yt-dlp `--encoding utf-8` (двойной пояс).
- Cleanup `work_dir` на error-path первого прохода yt-dlp — больше не оставляет недокачанные части в `%TEMP%`.

## [1.4.0] - 2026-04-15

### Добавлено

- Настройка **Save downloaded audio**: сохранение извлечённого аудио в папку вывода — симметрично опции «Save downloaded video». Работает для URL-джоб (копия из первого прохода yt-dlp, без повторной закачки) и локальных видеофайлов (извлечение через ffmpeg). Локальные аудиофайлы пропускаются.
- Селектор **Audio format**: `Original` (без перекодирования — bestaudio / `-c:a copy`; контейнер определяется ffprobe: aac→m4a, opus→.opus, vorbis→.ogg, flac→.flac, fallback AAC/m4a), `m4a` (AAC) и `mp3`.
- Новый модуль `audio_save` с `copy_downloaded_audio` и `extract_audio_from_local_video`. Ошибки сохранения аудио не прерывают транскрипцию, а уходят в лог канала `audio-save`.

### Изменено

- `PrepareAudioResult` теперь отдаёт `source_media_files`, index-aligned с `wav_paths` — джоба использует это, чтобы забрать исходное аудио до нормализации в 16 kHz WAV.
- `output_template::format_output_filename` обобщён: для legacy-шаблонов без `{ext}` суффикс `.txt` переписывается в любой целевой extension (не только `.mp4`).

## [1.3.0] - 2026-04-01

### Добавлено

- Настройка **Browser cookies source** для yt-dlp: прямое чтение cookies из Chrome, Brave, Edge или Firefox через `--cookies-from-browser` для YouTube и TikTok.
- Быстрые чипы для **yt-dlp JS runtimes** (`deno`, `nodejs`, `node`) в настройках и в мастере первого запуска.
- Кнопка **Download & install Deno for me**: приложение скачивает Deno в managed tools и автоматически выставляет `ytDlpJsRuntimes=deno`.

### Изменено

- В pipeline yt-dlp добавлена передача browser cookies и поиск sibling-бинарников через PATH, чтобы yt-dlp видел установленный рядом `deno`.
- README и подсказки UI обновлены с объяснением ограничений Chromium cookies на Windows и рекомендацией использовать Firefox.

## [1.2.0] - 2026-04-01

### Изменено

- **README** переведён на английский язык; добавлены разделы: поддерживаемые URL-источники (YouTube, TikTok, …), таблица Whisper-моделей, пошаговый workflow, рекомендации по использованию очереди.
- TikTok-ссылки поддерживаются «из коробки» через yt-dlp (публичные видео и короткие ссылки `vm.tiktok.com`).

## [1.1.3] - 2026-03-29

### Исправлено

- **macOS (CI):** оставался один литерал `ToolDownloadProgress { … }` в `whisper_bottle_macos.rs` (GHCR token) → снова **E0451**; заменён на `ToolDownloadProgress::new`.

## [1.1.2] - 2026-03-29

### Исправлено

- **Сборка Rust (macOS / кросс-CI):** `whisper_bottle_macos` создавал `ToolDownloadProgress` литералом из другого модуля при приватных полях структуры → **E0451**. Добавлен конструктор `ToolDownloadProgress::new`; убраны лишние предупреждения компилятора (`apply_win_no_window`, `process_kill`, импорты `tool_download` на Linux).
- **GitHub Actions (Release, macOS):** у `Swatinem/rust-cache` для матрицы был один и тот же ключ для двух job на `macos-latest` (aarch64 и x86_64), из‑за чего кэш `target/` смешивал разные triple и `tauri build` мог падать. Для каждой строки матрицы задан свой `rust_cache_key`.

## [1.1.1] - 2026-03-29

### Исправлено

- **GitHub Actions:** пересобран `package-lock.json` с записями optional-пакетов для Linux/macOS/Windows (`@tauri-apps/cli-*`, `@esbuild/*`, `@rollup/*` и др.), чтобы шаг **`npm ci`** проходил на всех раннерах; релиз **v1.1.0** из-за старого lock-файла не собирался.

### Изменено

- В `docs/RELEASE.md` добавлена памятка на случай ошибки *Missing … from lock file* при `npm ci` в CI.

## [1.1.0] - 2026-03-28

### Добавлено

- Вкладки **Queue** / **Settings** вместо выпадающей панели настроек.
- Режим транскрипции **Browser Whisper** (Transformers.js / ONNX в webview) без внешнего whisper-cli.
- Настройка **yt-dlp JS runtimes** (`--js-runtimes`) для сценариев YouTube с EJS (при установленном Deno/Node и т.п.).
- Быстрый выбор ISO-кода языка в настройках: примеры **ru**, **uk**, **en** и подсказка про другие коды ISO 639-1.

### Изменено

- Улучшения UI настроек, подсказок и логирования для браузерного режима; синхронизация зависимостей и CI с upstream.

## [1.0.0] - 2026-03-22

### Первый публичный релиз (дистрибуция)

- Сборки **GitHub Actions** для **Windows** (NSIS + MSI), **macOS** (отдельно Apple Silicon `aarch64` и Intel `x86_64`), **Linux** (.deb + AppImage на Ubuntu 22.04).
- Метаданные пакета для установщиков: издатель, краткое и полное описание, категория.
- Версия приложения унифицирована в `package.json`, `src-tauri/Cargo.toml` и `src-tauri/tauri.conf.json`.

Функциональность соответствует накопленным возможностям до релиза: очередь файлов/URL, ffmpeg / yt-dlp, облачный API и локальный whisper.cpp, загрузка инструментов из настроек (где поддерживается).

[1.3.0]: https://github.com/vglu/v2t/releases/tag/v1.3.0
[1.2.0]: https://github.com/vglu/v2t/releases/tag/v1.2.0
[1.1.3]: https://github.com/vglu/v2t/releases/tag/v1.1.3
[1.1.2]: https://github.com/vglu/v2t/releases/tag/v1.1.2
[1.1.1]: https://github.com/vglu/v2t/releases/tag/v1.1.1
[1.1.0]: https://github.com/vglu/v2t/releases/tag/v1.1.0
[1.0.0]: https://github.com/vglu/v2t/releases/tag/v1.0.0
