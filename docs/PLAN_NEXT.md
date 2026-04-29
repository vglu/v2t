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

---

## Новые задачи (2026-04-29) — выявлены при ручном тестировании v1.4.0

### G — Очистка осиротевших `v2t-work-*` в TEMP (приоритет: средний)

**Симптом.** На машине разработки в `%TEMP%` обнаружены 4 папки `v2t-work-*` от прошлых сессий, **суммарно ~9 ГБ** (174 МБ + 2.77 ГБ + 2.95 ГБ + активная). Без ручной чистки `%TEMP%` распухает.

**Причина.** `prepare_media_audio` (`src-tauri/src/pipeline.rs:339`) создаёт `std::env::temp_dir().join("v2t-work-{nanos}")` и удаляет её только на ряде путей выхода (`pipeline.rs:375, 387` и т.п.). При жёстком закрытии приложения / panic / kill процесса / некоторых ошибках на поздних стадиях — папка остаётся.

**Что сделать.**
1. **Cleanup на старте.** Новый модуль `src-tauri/src/temp_cleanup.rs`: при инициализации `lib.rs` сканирует `temp_dir()` на `v2t-work-*` старше 24 ч и `remove_dir_all` каждой. Без блокировки UI (spawn в tokio).
2. **RAII guard для активной папки.** Структура с `Drop`, которая в `drop` делает `remove_dir_all` (best-effort, ошибки логируются). Кладём в `prepare_media_audio` сразу после создания папки — тогда даже panic в pipeline освобождает место.
3. **Кнопка в Settings** (опционально): «Clean temp files» с предварительным показом «найдено N папок, X МБ».

**Файлы:** `src-tauri/src/pipeline.rs`, новый `src-tauri/src/temp_cleanup.rs`, `src-tauri/src/lib.rs` (регистрация + вызов на старте), при желании `src/components/SettingsPanel.tsx`.

**Acceptance.** После повторных аварийных закрытий v2t суммарный размер `v2t-work-*` в TEMP не превышает размер одной активной задачи.

---

### H — Признаки жизни при скачивании плейлиста (yt-dlp progress) (приоритет: высокий)

**Симптом.** Запустил один публичный плейлист (`youtube.com/playlist?list=PL…`, ~39 роликов). В UI лога только три строки и потом — тишина на много минут:
```
[21:26:22] Added 1 URL(s)
[21:26:24] Starting queue (1 job(s))
[21:26:24] Run: https://www.youtube.com/playlist?list=PLaAm-u6DCW06Pxzn…
[21:26:24] [prepare] Preparing audio (yt-dlp / ffmpeg)…
```
Реально yt-dlp последовательно качал ролики (на момент проверки 18 файлов / 1.1 ГБ), но из UI этого не видно. Пользователь не отличает «работает» от «зависло».

**Причина.** `prepare_media_audio` запускает yt-dlp через `run_cmd(...)` (синхронно, с буферизацией всего stderr — `pipeline.rs:373`). Лог в UI эмитится **один раз** после завершения процесса (`pipeline.rs:382-384` → `emit_pipeline_log`). Построчного стриминга нет.

**Что сделать.**
1. **Streaming-вариант запуска yt-dlp.** Аналогично тому, что уже сделано для whisper (`whisper_local.rs`, см. пункт F в сводной таблице): `tokio::process::Command` + `BufReader` по `stderr` (yt-dlp пишет прогресс в stderr) построчно, без блокировки.
2. **Парсер строк yt-dlp** — новый модуль `src-tauri/src/yt_dlp_progress.rs`. Распознаём:
   - `[download] Downloading item N of M` → `{ index: N, total: M }`
   - `[download]   12.3% of   45.67MiB at  2.34MiB/s ETA 00:25` → `{ percent, speed, eta }`
   - `[youtube] <id>: Downloading webpage` / `Extracting URL` → можно показать как «Resolving…»
   - `[Merger] Merging formats into "..."` → фаза «merging»
3. **Событие `queue-job-progress`** с расширенным payload: `{ phase: "yt-dlp", item, total, percent, speed, eta, title? }`. Фаза `prepare` в UI остаётся как родительская.
4. **UI в `QueuePanel`** — под активной задачей показывать строку вида `yt-dlp · 5/39 · 12% · 2.3 MB/s · ETA 0:25`. Прогресс-бар тонкий, как у локального whisper.
5. (Бонус) **Heartbeat-пинг.** Если строки прогресса от yt-dlp молчат >10 сек, эмитить событие `pipeline-heartbeat` чтобы UI не казался мёртвым (например, при resolve видео или нестабильной сети).

**Файлы:** `src-tauri/src/pipeline.rs` (extract `run_cmd_streaming`), новый `src-tauri/src/yt_dlp_progress.rs` + тесты, `src-tauri/src/job.rs` (если нужно расширить payload), `src/components/QueuePanel.tsx`, типы в `src/types/`.

**Acceptance.** При старте плейлиста на 30+ роликов первая строка прогресса появляется в UI ≤ 5 сек; счётчик «N/M» и проценты текущего ролика видимо обновляются; нет 30-секундных «тишин» (проверяется визуально или e2e-таймером на отсутствие events).

---

### I — Заменить wall-clock `YT_DLP_TIMEOUT` на heartbeat-таймаут (приоритет: высокий, **связана с H**)

**Симптом (зафиксирован 2026-04-29).** Запуск публичного плейлиста на 39 роликов упал через ровно 15:01 с `Error: Process timed out after 900s`. На момент таймаута было скачано 25 из 39 файлов (~1.6 ГБ), процесс yt-dlp активно работал — никакого зависания не было.

**Причина.** `pipeline.rs:40` — `const YT_DLP_TIMEOUT: Duration = Duration::from_secs(900)`. Этот таймаут — глобальный wall-clock на **весь** вызов yt-dlp (`pipeline.rs:298, 373`). Для одного ролика 15 мин — щедро, для плейлиста — катастрофически мало. Любой плейлист >~12 роликов гарантированно не успевает.

**Что сделать.** **Heartbeat-таймаут** вместо wall-clock: убивать yt-dlp только если в его stderr не было новой строки в течение N секунд (предложение: 120с). Реализация естественно ложится на H (streaming stderr): каждая прочитанная строка сбрасывает таймер «последней активности»; отдельная задача `tokio::select!` с `tokio::time::sleep(heartbeat)` киляет процесс при истечении.

**Преимущества.**
- Плейлист в 100 роликов докачается за столько, сколько нужно — таймер сбрасывается при каждом проценте прогресса.
- Реально зависший yt-dlp (DNS не резолвится, сеть упала) — убивается за 2 минуты, а не за 15.
- Один и тот же механизм работает для одиночного ролика и плейлиста — не нужно ветвить логику.

**Файлы:** `src-tauri/src/pipeline.rs` (заменить `run_cmd` на `run_cmd_streaming` с heartbeat в обеих точках вызова yt-dlp; константу `YT_DLP_TIMEOUT` либо удалить, либо переименовать в `YT_DLP_HEARTBEAT = 120s`).

**Acceptance.** Плейлист на 39 роликов докачивается до конца. Симулированное «зависание» (yt-dlp с `sleep 300` без stderr) убивается через ~120 секунд с понятным сообщением `yt-dlp stalled (no output for 120s)`.

---

## Структурный прогресс UI (2026-04-29) — задачи J1/J2/J3/J4

После реализации H+I логи прогресса наконец долетают, но это всё ещё **wall-of-text**: на плейлисте 132 видео получаются тысячи строк, и пользователь не может за 1 секунду ответить на четыре вопроса:
- где я (item N/M)
- что упало (нужно grep по `Error:`)
- сколько ещё ждать (никак)
- как открыть проблемный ролик в браузере (никак)

Решение — структурированный progress UI поверх существующих событий. Лог сохраняем как опциональный collapsible-блок («Show raw log»). Модель данных:

```
Job (одна строка ввода — URL или путь)
 ├ kind, source, displayLabel, status, phase, progress
 ├ playlistTitle?, errorMessage?
 └ subtasks: Subtask[]                  ← пуст для одиночек

Subtask (видео в плейлисте / файл в папке)
 ├ id (yt-dlp video id или basename)
 ├ index (1-based), title, originalUrl?
 ├ status: pending | running | done | skipped | error
 ├ skipReason?, errorMessage?
 ├ progress: 0..1, phase
 └ transcriptPath?
```

**Зафиксированные ответы (PO 2026-04-29):**
- **Skipped** — это resume (transcript-файл уже есть; см. `job.rs:362-381`). Отображать `⏭ skipped (already done)` серым, **не как ошибка**.
- **Retry per-item** в J3 — re-enqueue одной ссылкой с `--no-playlist` (новая независимая job на одно видео). Не «replay» внутри плейлиста.
- **Имена файлов из папки** — `basename` достаточно (без родительского каталога).
- **Persistence прогресса между перезапусками** — отложено, пока не делаем.

---

### J1 — Структурный per-item прогресс (приоритет: высокий, ~1-2 дня)

**Что сделать.**
1. **Backend.** В `yt_dlp_progress::parse_yt_dlp_line` возвращать не `Option<String>`, а структуру `Option<YtDlpEvent>` с разобранными полями:
   - `Item { n: u32, total: u32 }`
   - `Progress { percent_bucket: u8, raw: String }`
   - `ExtractAudio` / `Merger`
2. В `pipeline.rs::run_yt_dlp_streaming` эмитить расширенный payload `queue-job-progress`:
   ```rust
   { jobId, phase, message, subtaskIndex?, subtaskTotal?, subtaskPercent? }
   ```
3. **Frontend.** В `QueuePanel` под каждой active-job-карточкой:
   - Прогресс-бар верхнего уровня (по `subtaskIndex / subtaskTotal`).
   - Подстрочник: `Item 5/132 · 47% · 1.5 MiB/s · ETA 0:18` — рендерится из последнего полученного события.
   - Лог становится collapsible с дефолтом "collapsed" (кнопка `Show log` / `Hide log`).
4. **Фильтр шумных строк в логе.** В collapsed-логе по дефолту скрывать строки `[yt-dlp] X% of …` (оставлять только milestones `item N/M`, `extracting audio`, `merging`). Чекбокс «Show download percentages» включает их обратно.

**Файлы:** `src-tauri/src/yt_dlp_progress.rs`, `src-tauri/src/pipeline.rs`, `src-tauri/src/job.rs` (если потребуется новый event), `src/components/QueuePanel.tsx`, типы в `src/types/`.

**Acceptance.** На плейлисте 132 видео в любой момент времени видно: имя/url job, прогресс-бар, текущий N/M, % текущего ролика, ETA. Лог свёрнут, но доступен. После завершения job — прогресс-бар на 100%, summary `132 done · 0 errors`.

---

### J2 — Имена видео и заголовок плейлиста (приоритет: средний, ~1 день)

**Что сделать.**
1. **Pre-resolve вызов yt-dlp** в начале URL-job (только для playlist/channel URLs):
   ```
   yt-dlp --flat-playlist --dump-single-json <url>
   ```
   Возвращает JSON: `{ title, entries: [{ id, title, url, playlist_index }, …] }`. Один сетевой round-trip, без скачивания. Heartbeat-таймаут 60s; при ошибке (приватный плейлист, нет cookies, сеть) — best-effort, пропускаем без падения job.
2. Эмитим новое событие `playlist-resolved`:
   ```typescript
   { jobId, playlistTitle, subtasks: [{ id, index, title, originalUrl }] }
   ```
3. **UI.** Заголовок job меняется с короткого `PLaAm-…` на `<playlistTitle> (132 видео)`. Под job появляется список subtasks с заголовками — статус каждого `pending` пока не дошла очередь.
4. **Match строк прогресса с subtask-ами.** При получении `subtaskIndex` находим subtask по `index` (или по `id`, если backend проставит из `[youtube] <id>: ...` строк). Обновляем его `status` / `progress` / `phase`.

**Файлы:** `src-tauri/src/pipeline.rs` (новый шаг pre-resolve), `src-tauri/src/job.rs` (вызов pre-resolve до prepare_media_audio), новый модуль `src-tauri/src/yt_dlp_metadata.rs` (десериализация JSON).

**Acceptance.** На канал-handle / playlist-URL заголовок job — настоящее имя из YouTube. Subtask-список показан с человеческими именами роликов (не голыми id). Если pre-resolve упал — job всё равно идёт, subtasks показываются с id вместо title.

---

### J3 — Статусы, ссылки, retry (приоритет: средний, ~1 день)

**Что сделать.**
1. **Иконки статусов** subtask: `⏸ pending`, `▶ running`, `✓ done`, `⏭ skipped (already done)`, `✗ error`.
2. **Кликабельные ссылки.** Заголовок subtask — `<a>` с `originalUrl`; клик открывает через `tauri-plugin-opener` (`open_url`). Плагин уже подключён в `lib.rs`.
3. **Skipped — серым, не как ошибка.** Источник — pipeline уже делает resume в `job.rs:362-381` (если `dest_path` существует и непустой). Нужно прокинуть это решение наверх как отдельное событие `subtask-skipped { reason }`.
4. **Retry per-item.** Кнопка `↻` рядом с subtask в статусе `error`. Нажатие → re-enqueue новой URL-job на ОДНУ ссылку (`originalUrl`), с эффективным `--no-playlist` (наш парсер `youtube_watch_url_should_use_no_playlist` это уже даёт). Никакого replay внутри текущего плейлиста.
5. **Error message** — короткий tooltip / inline-текст под subtask. Не весь stderr, а tail (как `tail_stderr` уже делает).

**Файлы:** `src/components/QueuePanel.tsx` (списки subtasks, иконки, кнопки), `src/components/SubtaskRow.tsx` (новый), `src-tauri/src/job.rs` (event `subtask-skipped`).

**Acceptance.** На плейлисте, где 5 видео упало и 3 уже были транскрибированы ранее: видно `124 ✓ · 5 ✗ · 3 ⏭`. Можно кликнуть retry на любой ✗ — добавляется новая job на одну ссылку, основной плейлист не трогается. Можно кликнуть title — открывается видео в браузере.

---

### J4 — Имена для локальных файлов (приоритет: низкий, ~0.5 дня)

**Что сделать.** Когда job — это файл из папки (`scan_media_folder` → много `kind: "file"` jobs), показывать в title `basename` файла без расширения. Без родительских каталогов. Прогресс — те же фазы (`normalize`, `transcribe`), но без `subtaskIndex` (одна задача = один файл).

**Файлы:** `src/components/QueuePanel.tsx` (UI), возможно `src/lib/queueUtils.ts` (helper).

**Acceptance.** При запуске на папке с 30 mp4 — видно 30 строк, каждая с человеческим именем файла, статус-иконкой, прогресс-баром.

---

### Что НЕ делается в рамках этого набора

- Параллельная транскрипция (отдельная задача TASK-11 из старого плана).
- Persisted state между перезапусками приложения (отложено).
- Toast-уведомления при ошибках / завершении.
- Канвасы / waterfall-диаграммы / любая нестандартная визуализация — только обычный DOM-список.
- Subtitles fast-path (использовать YouTube subs вместо Whisper, когда они есть и хорошего качества) — это **отдельная задача K**, выходит за рамки UI-работ J.

---

## Encoding-fix (2026-04-29) — задача I+ (тривиальная, прицепом к I)

**Симптом.** При втором проходе yt-dlp (video-pass на плейлисте с UA/RU роликами):
```
Could not save video: yt-dlp video download failed (exit 120):
ERROR: Unable to download video: [Errno 22] Invalid argument
Exception ignored in: <_io.TextIOWrapper name='<stdout>' mode='w' encoding='cp1252'>
OSError: [Errno 22] Invalid argument
```
Audio-pass отрабатывает (его прогресс-строки ASCII-only), а video-pass пишет больше metadata-строк в stdout, в т.ч. кириллические заголовки ролика. Pipeline падает на `cp1252.encode()` с `Errno 22`.

**Причина.** Когда yt-dlp запущен с `Stdio::piped()`, Windows назначает stdout кодовой страницей ANSI (`cp1252`). Python внутри yt-dlp пытается напечатать non-ASCII → OSError. exit 120 = unhandled IOError при flush.

**Что сделать.**
- В `pipeline.rs::run_yt_dlp_streaming` добавить `cmd.env("PYTHONIOENCODING", "utf-8")` рядом с тем местом, где уже выставляется PATH.
- (Дополнительно) добавить флаг yt-dlp `--encoding utf-8` в обе arg-сборки — это его собственное решение той же проблемы (на случай Python с не-UTF-8 локалью).

**Acceptance.** На плейлисте с кириллическими заголовками video-pass отрабатывает без OSError. (Проверка: тот же playlist, который сегодня упал.)

---

## L — GPU acceleration для local Whisper (2026-04-29)

**Эпик от Алисы.** На текущем CPU-only билде whisper.cpp `medium` модель работает примерно в реальном времени, `large-v3` — в 2-2.5× медленнее реального времени. Для плейлиста на 65 часов аудио (132 × ~30 мин) это 3+ суток молотить. У целевой машины разработки — NVIDIA RTX 3060 Ti 8 ГБ — потенциал 10-20× ускорения через CUDA, не используется.

**Сейчас в коде.** `tools.manifest.json:13` качает `whisper-bin-x64.zip` (CPU-only). Никаких упоминаний `cuda`/`cublas`/`vulkan`/`gpu` нигде в `src-tauri` — backend выбора не существует.

**В апстриме (whisper.cpp v1.8.4) рядом с CPU-zip уже лежат:**
- `whisper-cublas-12.4.0-bin-x64.zip` — CUDA для NVIDIA (10-20× быстрее)
- `whisper-vulkan-bin-x64.zip` — Vulkan для NVIDIA / AMD / Intel (8-15×)
- `whisper-blas-bin-x64.zip` — OpenBLAS, CPU-оптимизация (1.5-3×)

Бинарник `whisper-cli.exe` — **тот же**, отличаются только сопровождающие DLL (`cudart.dll`, `cublas.dll`, `vulkan-1.dll`).

### L1 — Auto-detect GPU + Settings UI

**Что сделать.**
1. В `deps.rs` или новый модуль `gpu_detect.rs`: на Windows запросить через WMI имена видеокарт, классифицировать (`Nvidia` / `Amd` / `Intel` / `None`).
2. В `settings.ts` / `settings.rs` добавить поле `whisper_acceleration: "auto" | "cuda" | "vulkan" | "cpu"` (default `auto`).
3. В `SettingsPanel` новая секция «Whisper acceleration» с radio: Auto (рекомендуем CUDA если NVIDIA) / CUDA / Vulkan / CPU. Краткий хинт «На NVIDIA даёт 10-20× ускорение».
4. В `OnboardingWizard` — если детект нашёл NVIDIA, тонкая подсказка «Found GeForce RTX 3060 Ti — enable CUDA acceleration?» с кнопкой включения по дефолту.

**Файлы:** `src-tauri/src/gpu_detect.rs` (новый), `src-tauri/src/settings.rs`, `src/types/settings.ts`, `src/components/SettingsPanel.tsx`, `src/components/OnboardingWizard.tsx`.

**Acceptance.** На машине с NVIDIA выбор `Auto` приводит к скачиванию cuBLAS-zip; в UI видно, что использует GPU. На машине без NVIDIA `Auto` остаётся CPU.

---

### L2 — Manifest + download для CUDA / Vulkan zip

**Что сделать.**
1. Расширить `tools.manifest.json` для Windows:
   ```json
   "whisper_zip_cpu":    { "url": "...whisper-bin-x64.zip", "sha256": "..." },
   "whisper_zip_cublas": { "url": "...whisper-cublas-12.4.0-bin-x64.zip", "sha256": "..." },
   "whisper_zip_vulkan": { "url": "...whisper-vulkan-bin-x64.zip", "sha256": "..." }
   ```
2. В `tool_download.rs` при скачивании whisper выбирать `whisper_zip_*` по полю `settings.whisper_acceleration`.
3. Распаковка та же; binary тот же; whisper.cpp **сам** использует GPU при наличии нужных DLL рядом.
4. При смене `whisper_acceleration` в Settings — кнопка «Re-download whisper» в UI (не пересобирать автоматически — пользователь может не хотеть тратить трафик).

**Файлы:** `src-tauri/tools.manifest.json`, `src-tauri/src/tool_manifest.rs`, `src-tauri/src/tool_download.rs`, `src/components/SettingsPanel.tsx`.

**Acceptance.** При выборе CUDA скачивается ~150 МБ zip с cublas DLL, распаковка кладёт DLL рядом с whisper-cli.exe. На реальном ролике 41 мин с моделью `large-v3-turbo` транскрипция занимает <5 мин (vs ~25 мин CPU).

---

### L3 — Fallback на CPU при ошибке инициализации GPU

**Что сделать.**
1. Если `whisper-cli.exe` падает с явной CUDA/Vulkan ошибкой (`cudaGetDeviceCount returned X`, `failed to initialize Vulkan`) — поймать в `whisper_local.rs`, показать пользователю message «GPU init failed (driver mismatch?), falling back to CPU», и **повторить** транскрипцию с CPU-zip.
2. (Опционально) При первом успехе CUDA/Vulkan — записать `last_known_good_acceleration` в settings, чтобы при следующих запусках не делать smoke-test заново.

**Файлы:** `src-tauri/src/whisper_local.rs`, `src-tauri/src/settings.rs`.

**Acceptance.** Сборка с включённым CUDA, запущенная на машине без NVIDIA — корректно сообщает об ошибке и завершает транскрипцию через CPU-fallback, без зависаний.

