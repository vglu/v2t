# Changelog

Формат основан на [Keep a Changelog](https://keepachangelog.com/ru/1.1.0/).

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

[1.1.2]: https://github.com/vglu/v2t/releases/tag/v1.1.2
[1.1.1]: https://github.com/vglu/v2t/releases/tag/v1.1.1
[1.1.0]: https://github.com/vglu/v2t/releases/tag/v1.1.0
[1.0.0]: https://github.com/vglu/v2t/releases/tag/v1.0.0
