# PLAN_NEXT.md — v2t следующая итерация

Дата: 2026-03-27. Базируется на анализе текущего кода + опыте тестирования на macOS/Linux.

**Сделано (итерация Анатолий):** **A1** — `macos_search_whisper_cli_in_path` в `deps.rs` (which + keg-пути), чеклист без кнопки; `locate_whisper_cli_macos` в `tool_download.rs` использует то же. **A2** — Linux: блок инструкций в `SettingsPanel` / `OnboardingWizard`, кнопки скачивания whisper по-прежнему только Win/macOS; `src/lib/platform.ts` (`isProbablyLinux`). **F** — `whisper_local.rs`: stderr построчно, парсинг `N%`, событие `queue-job-progress` с фазой `whisper`. **B** — `keep_downloaded_video` в настройках; второй проход `yt-dlp` (`download_best_video_mp4`) после аудио; имя `.mp4` из шаблона транскрипта (`video_filename_from_transcript_template`). **E** — `transcribe.rs`: до 3 попыток HTTP-транскрипции при 429/5xx и ретраебельных сетевых ошибках `reqwest`, паузы 1s и 2s между попытками, учёт отмены между паузами; **resume**: чекпоинты `v2t-api-{fp}-chunk-{i}.txt` при сплите WAV (fingerprint mtime+size исходного WAV), очистка после успеха; в `whisper_local.rs` — `v2t-whisper-{fp}-chunk-{i}.txt`; в `job.rs` — если целевой `.txt` транскрипта уже есть и не пустой, трек пропускается. **C** — `session_log.rs`: файл на сессию в `app_data_dir/logs/`, ротация 5 `.log`; запись из `job.rs`, `pipeline.rs`, `whisper_local.rs` и UI (`session_log_append_ui`); кнопка «Open log file» в `QueuePanel`. **D** — `tools.manifest.json` + `tool_manifest.rs` (`include_str!`): закреплённые URL (yt-dlp `2026.03.17`, whisper.cpp zip `v1.8.4`, ffmpeg-static macOS `b6.1.1`; Windows FFmpeg zip BtbN `latest` без хэша); потоковая проверка SHA-256 после скачивания, если поле `sha256` не пустое.

### Сводная таблица: сделано / не сделано

| Пункт | Тема | Статус | Комментарий |
|-------|------|--------|-------------|
| **A1** | Поиск `whisper-cli` на macOS (which, `whisper`/`main`, keg Homebrew и т.д.) | **Сделано** | П.5 плана A1 (отдельный отладочный блок в UI «что искали») не выносился — в ошибке уже перечислены проверки. |
| **A2** | Linux: инструкции в настройках / онбординге вместо бесполезной кнопки | **Сделано** | |
| **A3** | Скачка `whisper-cli` на macOS без zip релиза | **Сделано** | Обход: бутылка Homebrew (GHCR) в `whisper_bottle_macos.rs` + fallback «Find whisper-cli»; zip с GitHub по-прежнему нет. |
| **B** | Опция «сохранить скачанное видео» + второй проход yt-dlp | **Сделано** | |
| **B+** | Плейсхолдер `{ext}` в шаблоне имён файлов | **Сделано** | `output_template.rs` / `outputTemplate.ts`; дефолт шаблона `{title}_{date}.{ext}`; без `{ext}` для `.mp4` — прежняя подмена суффикса `.txt` → `.mp4`. |
| **C** | Персистентный лог, ротация, «Open log file» | **Сделано** | |
| **D** | Манифест URL + проверка SHA-256 при скачивании | **Сделано** | Для zip FFmpeg под Windows хэш намеренно пустой (BtbN `latest` меняется). |
| **E** | Retry HTTP API + resume по чанкам и по треку | **Сделано** | |
| **F** | Прогресс локального Whisper (`N%` из stderr → UI) | **Сделано** | |
| **WASM** | Режим «In-app Whisper» (Transformers.js) параллельно CLI/API | **Сделано** | `transcriptionMode: browserWhisper`: prepare в Rust → `browserPrepared` → WASM в webview → `browser_queue_job_finish`; CLI и облако не удалялись. |

Ниже — прежние разделы с диагнозом и деталями; таблица выше — краткая сводка по статусу.

---

## Проблема A — Whisper CLI на macOS и Linux

### Диагноз

**macOS:**
- `locate_whisper_cli_macos` (`tool_download.rs:229`) проверяет только два пути:
  `/opt/homebrew/bin/whisper-cli` и `/usr/local/bin/whisper-cli`
- Не делает `which whisper-cli` / `which whisper` — не смотрит в PATH
- Не проверяет альтернативные имена бинарника (`whisper`, `main`)
- Кнопка «Setup whisper-cli» никогда ничего не скачивает — только ищет
- Если пользователь ещё не установил Homebrew, кнопка всегда даёт ошибку

**Linux:**
- `download_whisper_cli_managed` возвращает ошибку сразу (`tool_download.rs:111`)
- В UI нет никаких инструкций для Linux — пользователь видит кнопку которая не работает
- `scan.rs` и `deps.rs` поддерживают Linux, но способа поставить CLI нет

### Что делать

#### A1 — Улучшить обнаружение на macOS (приоритет: высокий)

В `locate_whisper_cli_macos`:
1. Добавить `which whisper-cli` и `which whisper` через `std::process::Command`
2. Проверить имена-алиасы: `whisper-cli`, `whisper`, `main`
3. Добавить пути Intel Homebrew: `/usr/local/opt/whisper-cpp/bin/whisper-cli`
4. Добавить путь Nix-совместимого менеджера: `/home/linuxbrew/.linuxbrew/bin/`
5. Показывать в UI что именно искали и где — для отладки

#### A2 — Инструкции для Linux (приоритет: средний)

В `SettingsPanel.tsx` и `OnboardingWizard.tsx`:
- Если `isLinux()` — показывать статический блок с инструкциями вместо кнопки:
  ```
  Ubuntu/Debian: sudo apt install whisper-cpp
  Fedora: sudo dnf install whisper-cpp
  Arch: yay -S whisper-cpp
  Или: build from source → github.com/ggml-org/whisper.cpp
  ```
- Убрать кнопку «Download whisper-cli» на Linux (она всегда фейлит)

#### A3 — macOS без darwin zip в релизах whisper.cpp

**Статус:** zip с GitHub по-прежнему нет (404). Реализована **скачка бутылки** `whisper-cpp` с GHCR (анонимный pull-token + `formulae.brew.sh`), распаковка `tar.gz`, поиск `bin/whisper-cli` → `managed_bin_dir`. При сбое — прежний «Find whisper-cli» (PATH / Homebrew).

Если upstream начнёт публиковать darwin zip: можно добавить ветку zip как на Windows.

---

## Проблема B — Сохранение скачанного видео

### Описание

Сейчас `pipeline.rs` вызывает yt-dlp с `--extract-audio` — видеофайл не сохраняется.
Пользователь хочет иметь опцию: «сохранить видеофайл в папку вывода».

### Место в настройках

В `AppSettings` (TypeScript `src/types/settings.ts` + Rust `src-tauri/src/settings.rs`):
```typescript
keepDownloadedVideo: boolean  // default: false
```

В `SettingsPanel.tsx` — флажок в секции "Media Tools" или отдельной секции "Download":
```
☐ Save downloaded video to output folder
```

### Реализация в Rust (`pipeline.rs`)

**Подход: два прохода yt-dlp**

Проход 1 (всегда): `--extract-audio --audio-format wav` → temp WAV → транскрипция (существующий код)

Проход 2 (если `keep_downloaded_video = true`):
```
yt-dlp -f "bv*+ba/b" --merge-output-format mp4 -o "{output_dir}/{title}.mp4" URL
```
- Запускается параллельно или после основного пайплайна
- Сохраняет лучшее качество видео в папку вывода
- Эмитит отдельный прогресс-ивент или пишет в лог

**Альтернативный подход: один проход (эффективнее сетево)**

1. yt-dlp скачивает лучший формат видео в temp
2. ffmpeg извлекает аудио из скачанного файла (вместо `--extract-audio`)
3. Видеофайл копируется в папку вывода
4. Аудио → транскрипция → удаляется если `delete_audio_after`

Минус: yt-dlp при `best` может скачать огромный файл. Плюс: один сетевой запрос.

**Рекомендация:** два прохода — проще и безопаснее для UX. Второй проход асинхронный.

### Имя файла видео

Использовать существующий `output_template::format_output_filename` с расширением `.mp4`.
Добавить `{ext}` плейсхолдер в шаблон — пригодится для видео vs текста.

---

## Проблема C — Нет персистентного лога

### Диагноз

Логи живут только в UI (`QueuePanel.tsx`, max 200 строк). При закрытии/крэше — пропадают.
Для отладки проблем пользователи не могут предоставить полный лог.

### Решение

Добавить запись лога в файл в `app_data_dir/logs/` (**реализовано**):
- Один файл на сессию: `v2t-YYYY-MM-DD-HHMMSS.log`
- Ротация: хранить последние 5 файлов
- Формат: `[HH:MM:SS] [job_id] phase: message`
- Команда Tauri: `open_session_log` → открыть текущий лог системным приложением

В UI в панели лога — кнопка «Open log file» рядом с «Copy».

---

## Проблема D — Hardcoded URLs для инструментов

### Диагноз

URL для скачки ffmpeg, yt-dlp, whisper-cli зашиты в `tool_download.rs` как константы.
Если upstream поменяет структуру zip или URL — нужна пересборка приложения.

### Решение (минимальное) — **реализовано**

Файл `src-tauri/tools.manifest.json` вкомпилирован через `include_str!` в `tool_manifest.rs`.
Секции `windows` / `macos`: для каждого артефакта `url` и опционально `sha256` (пустая строка = не проверять).
`tool_download.rs`: при скачивании считает SHA-256 по мере приёма потока и сравнивает с манифестом.

**Особенности:** zip FFmpeg под Windows по-прежнему с URL BtbN `…/latest/…` — сборки меняются ежедневно, поэтому `sha256` для него пустой (верификация отключена). Остальные записи с хэшем привязаны к конкретным тегам релизов; при обновлении версий править манифест и пересобирать приложение.

**Приоритет: низкий** — выполнено для всего, где стабилен артефакт и известен digest (GitHub API / релизы).

---

## Проблема E — Нет retry при ошибке API

### Диагноз

Если API временно недоступен или вернул 5xx в середине чанкинга — задача падает целиком.
Повторный запуск начинает с нуля включая скачку.

### Решение (минимальное)

В `transcribe.rs` (**сделано**):
- 3 попытки с паузами 1s и 2s для HTTP 429 / 5xx и ретраебельных ошибок `reqwest` (таймаут, connect и т.п.)
- Не ретраить 4xx кроме 429 (в т.ч. 400 / 401 / 403)
- При сплите большого WAV — чекпоинты по чанкам (`v2t-api-{mtime-size}-chunk-{i}.txt`), после полного успеха удаляются

В `whisper_local.rs` (**сделано**): те же чекпоинты для локального сплита (`v2t-whisper-{fp}-chunk-{i}.txt`).

В `job.rs` (**сделано**):
- Если итоговый файл транскрипта для трека уже существует и не пустой — трек не транскрибируется снова (resume на уровне трека)

**Приоритет: средний**

---

## Проблема F — Прогресс при локальном Whisper

### Диагноз

`whisper_local.rs` запускает CLI с таймаутом 7200s без промежуточного прогресса.
Пользователь видит «running» без изменений — непонятно, работает ли.

### Решение

whisper.cpp CLI пишет прогресс в stderr: `whisper_print_progress_callback: 10% done`
Читать stderr построчно (`BufReader`), парсить строки с `%`, эмитить `queue-job-progress`.

В UI показывать процент для локального режима.

**Приоритет: средний**

---

## Порядок реализации

Краткий статус «сделано / нет» — в **сводной таблице** в начале документа.

| # | Задача | Файлы | Приоритет |
|---|--------|-------|-----------|
| 1 | A1: Улучшить обнаружение whisper-cli на macOS | `tool_download.rs`, `deps.rs` | Высокий |
| 2 | A2: UI-инструкции для Linux | `SettingsPanel.tsx`, `OnboardingWizard.tsx` | Высокий |
| 3 | B: Сохранение видео — настройка + pipeline | `settings.rs`, `settings.ts`, `pipeline.rs`, `SettingsPanel.tsx` | Средний — **сделано** |
| 4 | F: Прогресс локального Whisper из stderr | `whisper_local.rs` | Средний — **сделано** |
| 5 | E: Retry + resume (HTTP чанки, whisper чанки, трек) | `transcribe.rs`, `whisper_local.rs`, `job.rs` | Средний — **сделано** |
| 6 | C: Персистентный лог в файл | `session_log.rs`, `lib.rs`, `job.rs`, `pipeline.rs`, `whisper_local.rs`, `QueuePanel.tsx` | Низкий — **сделано** |
| 7 | A3: Прямая скачка whisper-cli macOS | `tool_download.rs` | Низкий — **ожидает** darwin-zip в релизах upstream (см. §A3) |
| 8 | D: SHA-256 + манифест URL | `tools.manifest.json`, `tool_manifest.rs`, `tool_download.rs` | Низкий — **сделано** |

---

## Решение про TypeScript whisper-библиотеки

**Вывод:** текущий подход (CLI) — правильный для данного продукта.

`@huggingface/transformers` (ONNX WebAssembly) — единственная реальная нативная TS-альтернатива.
Подходит если: не нужен внешний CLI, допустима более низкая скорость, размер модели ≤ small.

Для v2t не менять — CLI даёт лучшую скорость, поддерживает все модели (до large-v3-turbo),
не блокирует main thread, правильно убивается через CancellationToken.
