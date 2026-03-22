# Changelog

Формат основан на [Keep a Changelog](https://keepachangelog.com/ru/1.1.0/).

## [1.0.0] - 2026-03-22

### Первый публичный релиз (дистрибуция)

- Сборки **GitHub Actions** для **Windows** (NSIS + MSI), **macOS** (отдельно Apple Silicon `aarch64` и Intel `x86_64`), **Linux** (.deb + AppImage на Ubuntu 22.04).
- Метаданные пакета для установщиков: издатель, краткое и полное описание, категория.
- Версия приложения унифицирована в `package.json`, `src-tauri/Cargo.toml` и `src-tauri/tauri.conf.json`.

Функциональность соответствует накопленным возможностям до релиза: очередь файлов/URL, ffmpeg / yt-dlp, облачный API и локальный whisper.cpp, загрузка инструментов из настроек (где поддерживается).

[1.0.0]: https://github.com/YOUR_ORG/YOUR_REPO/releases/tag/v1.0.0
