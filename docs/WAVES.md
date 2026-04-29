# v2t — Wave Execution Plan

> **Дата плана:** 2026-04-29
> **Целевой релиз:** v1.5.0 (Waves 1-4) → v1.6.0 (Wave 5)
> **Текущая версия:** 1.4.0 (см. `package.json`, `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json`)

---

## 0. Как пользоваться этим документом

Этот файл — **исполняемый план**. Каждая волна (Wave 1..5) — самодостаточный блок работы, содержащий всё, что нужно одному агенту/разработчику в одной чистой сессии. Никаких отсылок к «предыдущему обсуждению» — только к коду и `docs/PLAN_NEXT.md`.

**Что прочитать до старта любой волны:**
1. `CLAUDE.md` (если присутствует) — соглашения проекта.
2. Этот файл — выбрать волну, проверить pre-flight.
3. `docs/PLAN_NEXT.md` — детальная спецификация задач G/H/I/J1-J4/K/L (этот файл — оркестрация, тот — спецификация).
4. Целевые файлы исходников из секции **Files** выбранной волны.

**Универсальный pre-flight перед любой волной:**
```bash
# 1. Чистое рабочее дерево
git status                           # уверенность, что нет конфликтов

# 2. Зелёная база
npm install
npm run test:run                     # 19 vitest тестов
cd src-tauri && cargo test --no-default-features --lib && cd ..  # 41 rust тест

# 3. Сборка
cd src-tauri && cargo build --no-default-features && cd ..

# 4. (Опционально) запуск UI для ручного теста
npm run tauri dev
```

**Ничего из этого не должно падать.** Если падает — это блокер, не лезть в волну, разобраться сначала.

---

## 1. Критические PO-решения (зафиксированы 2026-04-29, не переспрашивать)

Эти решения приняты product-owner'ом и не должны обсуждаться повторно. Источник: согласовано в `docs/PLAN_NEXT.md`.

| Тема | Решение |
|---|---|
| **Skipped status** (resume в `job.rs:362-381`) | Отображать `⏭ skipped (already done)` серым, **не как ошибка**. |
| **Retry per-item** (J3) | Re-enqueue одной ссылкой с эффективным `--no-playlist`. **Не** replay внутри плейлиста. |
| **Имена локальных файлов** (J4) | `basename` достаточно. Без родительского каталога. |
| **Persisted state очереди** | **Не делать** в этом цикле. Каждый запуск с чистой очереди. |
| **Параллельная транскрипция** | **Не делать** — отдельная задача TASK-11 в `docs/TASKS.md`. |

---

## 2. Соглашения проекта

**Стек:** Tauri 2 + React 19 + TypeScript + Vite. Rust crate в `src-tauri/`. Sidecars (`ffmpeg`, `yt-dlp`, `whisper-cli`) — рядом с приложением или путь в Settings. Транскрипция: HTTP API / local whisper.cpp / browser-WASM (transformers.js).

**Стиль коммитов** (см. `git log` верхом):
- `feat: <subject>` / `fix: <subject>` / `chore: release vX.Y.Z — <one-liner>`
- Именованный scope при необходимости: `fix(macos): …`, `feat(pipeline): …`
- Релиз: одновременно бампятся версии в `package.json`, `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json`. Запись в `CHANGELOG.md`.

**Sidecar-runner pattern.** Все внешние процессы (yt-dlp, ffmpeg, whisper-cli) запускаются через **per-line streaming + heartbeat watchdog** — паттерн `pipeline.rs::run_yt_dlp_streaming` (yt-dlp) и `whisper_local.rs::run_whisper_cli_with_progress` (whisper). `run_cmd` остался только для ffmpeg-нормализации (короткая операция).

**События для UI** (Tauri `app.emit`):
- `queue-job-progress` — payload `{ jobId, phase, message }` (см. `job.rs:50-56`, listener `QueuePanel.tsx:144-151`).
- `pipeline-log` — payload `{ jobId, label, message }` (то же место).

**Cancellation.** `CancellationToken` пробрасывается во все долгие операции; `JobCancelRegistry` (`src-tauri/src/cancel_registry.rs`) хранит токены по job-id.

**Cleanup work_dir.** Каждая `prepare_media_audio` создаёт `temp_dir().join("v2t-work-{nanos}")`. Папка удаляется на ряде путей (`pipeline.rs`), плюс **периодически** на старте (`temp_cleanup::run_cleanup`, 24-часовой порог) — это safety net.

---

## 3. Дерево зависимостей волн

```
Wave 1 (стабилизация скачивания) ─┬─→ Wave 2 (GPU acceleration)
                                  │
                                  ├─→ Wave 3 (structured progress UI) ─→ Wave 4 (names + links)
                                  │
                                  └─→ Wave 5 (subtitles fast-path)
```

Wave 2 / 3 / 5 — параллельные ветки от Wave 1. Wave 4 — единственная зависящая от Wave 3.

**Рекомендуемый порядок: 1 → 2 → 3 → 4 → 5.**

Аргумент: Wave 2 (GPU) даёт 10-20× ускорение transcribe. Все последующие волны тестируются на реальных прогонах. **Wave 2 — мета-инвестиция в скорость разработки оставшихся волн** (тест-цикл сократится в 10×).

Альтернатива: 1 → 3 → 4 → 2 → 5 (UI-first), если приоритет — внешний вид к публичной beta.

---

# Wave 1 — Стабилизация скачивания

**Цель.** Длинные плейлисты (>15 минут общего времени) докачиваются до конца без падений по wall-clock-таймауту, видны признаки жизни в логе, осиротевшие work-dir не копятся в TEMP.

**Зависимости.** Никаких — это база.

**Состояние на 2026-04-29.** Большая часть уже закоммичена (G/H/I и `--newline`). Осталась одна правка («I+ encoding-fix») и финальный smoke-test.

## 1.1. Что уже сделано (проверить в коде, не переделывать)

| ID | Задача | Где смотреть |
|---|---|---|
| **G** | `temp_cleanup.rs` — sweep `v2t-work-*` старше 24 ч на старте | `src-tauri/src/temp_cleanup.rs`, вызов в `lib.rs::run::setup` |
| **H** | Streaming-парсер прогресса yt-dlp + emit `queue-job-progress` фазы `yt-dlp`/`yt-dlp-video` | `src-tauri/src/yt_dlp_progress.rs`, `pipeline.rs::run_yt_dlp_streaming`, `pipeline.rs::spawn_pipe_reader` |
| **I** | Heartbeat-таймаут 120 сек вместо wall-clock 900 сек | `pipeline.rs::YT_DLP_HEARTBEAT`, `run_yt_dlp_streaming` select-loop |
| **+** | `--newline` флаг в обоих yt-dlp-вызовах (audio + video pass) | `pipeline.rs` |
| **+** | Cleanup work_dir на error-path yt-dlp | `pipeline.rs::prepare_media_audio` (audio pass) |

Тесты: 7 в `yt_dlp_progress`, 3 в `temp_cleanup`. Все зелёные.

## 1.2. Что осталось сделать (одна правка)

**I+ — encoding-fix для Windows.** Симптом из последнего теста (плейлист с UA/RU роликами):
```
Could not save video: yt-dlp video download failed (exit 120):
ERROR: Unable to download video: [Errno 22] Invalid argument
Exception ignored in: <_io.TextIOWrapper name='<stdout>' mode='w' encoding='cp1252'>
OSError: [Errno 22] Invalid argument
```

Причина: при `Stdio::piped()` Windows назначает stdout кодовую страницу `cp1252`. Python внутри yt-dlp не может закодировать кириллический заголовок ролика → OSError на flush, exit 120.

**Изменения:**

1. В `src-tauri/src/pipeline.rs::run_yt_dlp_streaming`, рядом с тем местом, где уже выставляется PATH:
   ```rust
   if let Some(parent) = program.parent() {
       let cur_path = std::env::var("PATH").unwrap_or_default();
       let sep = if cfg!(windows) { ";" } else { ":" };
       let new_path = format!("{}{sep}{cur_path}", parent.display());
       cmd.env("PATH", new_path);
   }
   // ↓ ДОБАВИТЬ
   cmd.env("PYTHONIOENCODING", "utf-8");
   ```

2. В обоих args-сборках yt-dlp (audio pass — `prepare_media_audio`, video pass — `download_best_video_mp4`) добавить флаг `--encoding utf-8` рядом с `--newline`. Это второй пояс — собственное решение yt-dlp той же проблемы.

## 1.3. Тест-план Wave 1

**Юнит-тесты:**
```bash
cd src-tauri && cargo test --no-default-features --lib
# Ожидание: 41 passed
cd .. && npm run test:run
# Ожидание: 19 passed
```

**Smoke-тест в UI (`npm run tauri dev`):**
1. В Settings — указать output dir, выбрать transcription mode (любой; для быстроты `httpApi` если есть ключ, иначе `localWhisper` с моделью `tiny` или `base`).
2. Опционально: включить «Save downloaded video» (это активирует video-pass, где упадёт без encoding-fix).
3. В Queue вставить URL: `https://www.youtube.com/playlist?list=PLaAm-u6DCW06PxznLqNTktdpa0BqhFHdi` (или любой UA/RU плейлист >12 видео).
4. Нажать Run.
5. **Ожидание:**
   - Через ≤5 сек в логе `[yt-dlp] item 1/N`.
   - Каждые ~5-10 сек строка вида `[yt-dlp] X% of YY.YYMiB at Z.ZZMiB/s ETA HH:MM` (X кратно 5).
   - На каждом видео последовательность `item N/M → 0% → 5% → … → 100% → extracting audio → item N+1/M`.
   - Через 15+ минут — НЕ падение по таймауту.
   - При video-pass (если включён) — отсутствие `OSError: [Errno 22]`.
6. Прерывание: можно отменить job (Cancel) — проверить, что папка `%TEMP%\v2t-work-*` удалилась.

## 1.4. Acceptance

- [ ] 41 rust + 19 vitest тестов проходят.
- [ ] Плейлист на 30+ роликов докачивается без падений по таймауту.
- [ ] В логе видны структурированные `[yt-dlp] …` строки прогресса.
- [ ] Video-pass на UA/RU контенте работает без `OSError: [Errno 22]`.
- [ ] Папки `v2t-work-*` чистятся (как минимум при successful job; orphan-cleanup срабатывает при следующем запуске).

## 1.5. Коммит и релиз

```bash
git add src-tauri/src/pipeline.rs src-tauri/src/yt_dlp_progress.rs src-tauri/src/temp_cleanup.rs src-tauri/src/lib.rs docs/PLAN_NEXT.md docs/WAVES.md
# Бамп версий: package.json, src-tauri/Cargo.toml, src-tauri/tauri.conf.json → 1.5.0-rc1
# Запись в CHANGELOG.md

git commit -m "$(cat <<'EOF'
feat(pipeline): heartbeat-streaming yt-dlp + temp cleanup + encoding fix

Replace 900s wall-clock yt-dlp timeout with 120s heartbeat watchdog so
playlists can download for arbitrary duration. Stream stdout+stderr
line-by-line, parse progress via yt_dlp_progress::parse_yt_dlp_line,
emit queue-job-progress events with phase yt-dlp/yt-dlp-video.

Add temp_cleanup module that sweeps v2t-work-* dirs older than 24h on
app startup, catching orphans from kill -9 / panic / delete_audio_after=false.

Set PYTHONIOENCODING=utf-8 + --encoding utf-8 to fix cp1252 encoding
crash in yt-dlp video-pass on Windows with UA/RU titles.

Release v1.5.0-rc1.
EOF
)"
```

**Reference:** детальные спеки G, H, I, I+ в `docs/PLAN_NEXT.md` (секции «G/H/I» и «Encoding-fix»).

---

# Wave 2 — GPU acceleration

**Цель.** local Whisper использует CUDA на NVIDIA / Vulkan на AMD-Intel. На разработческой машине с RTX 3060 Ti — 10-20× ускорение `large-v3-turbo` (~5 мин на 41-минутный ролик вместо ~25).

**Зависимости.** Wave 1 (стабильное скачивание для тестов).

**Объём.** ~1-1.5 рабочих дня.

## 2.1. Контекст

Сейчас `tools.manifest.json:11-15` качает `whisper-bin-x64.zip` (CPU-only) из релиза whisper.cpp v1.8.4. В коде нет ни одного упоминания `cuda`/`cublas`/`vulkan`/`gpu`. Backend выбора не существует.

В апстриме v1.8.4 рядом с CPU-zip есть:
- `whisper-cublas-12.4.0-bin-x64.zip` (~150 MB) — CUDA, для NVIDIA ⭐
- `whisper-vulkan-bin-x64.zip` (~80 MB) — Vulkan, для NVIDIA/AMD/Intel
- `whisper-blas-bin-x64.zip` — OpenBLAS, CPU-оптимизация

Бинарник `whisper-cli.exe` идентичен — отличаются только сопровождающие DLL.

## 2.2. Что сделать

### L1 — GPU autodetect + Settings UI
- Новый модуль `src-tauri/src/gpu_detect.rs`. Под Windows — WMI-запрос (`Win32_VideoController`), классификация: `Nvidia` / `Amd` / `Intel` / `None`. Под macOS / Linux — best-effort, default `None`.
- В `settings.rs` поле `whisper_acceleration: WhisperAcceleration` (enum: `Auto` | `Cuda` | `Vulkan` | `Cpu`, default `Auto`).
- Tauri-команда `detect_gpu` → возвращает обнаруженный тип GPU для UI.
- В `SettingsPanel.tsx` секция «Whisper acceleration» с radio (Auto/CUDA/Vulkan/CPU) + хинт по обнаруженному GPU.
- В `OnboardingWizard.tsx` — ненавязчивая подсказка «Found NVIDIA GPU — enable CUDA?» с включением по дефолту.

### L2 — Manifest + download
- Расширить `tools.manifest.json` (Windows-секция):
  ```json
  "whisper_zip_cpu":    { "url": "https://github.com/ggml-org/whisper.cpp/releases/download/v1.8.4/whisper-bin-x64.zip", "sha256": "74f973345cb52ef5ba3ec9e7e7af8e48cc8c71722d1528603b80588a11f82e3e" },
  "whisper_zip_cublas": { "url": "https://github.com/ggml-org/whisper.cpp/releases/download/v1.8.4/whisper-cublas-12.4.0-bin-x64.zip", "sha256": "<вычислить SHA-256 после первого download>" },
  "whisper_zip_vulkan": { "url": "https://github.com/ggml-org/whisper.cpp/releases/download/v1.8.4/whisper-vulkan-bin-x64.zip", "sha256": "<вычислить>" }
  ```
  - SHA-256 для cublas/vulkan zip — посчитать локально через `Get-FileHash -Algorithm SHA256` после первого скачивания, вписать в манифест.
- `tool_manifest.rs` (через `include_str!`) — загрузка/валидация структуры.
- В `tool_download.rs::download_whisper_cli_managed` (или новой функции) — выбор URL по `settings.whisper_acceleration` с fallback на `Auto` → `Cuda`-если-NVIDIA, иначе CPU.
- При смене `whisper_acceleration` в Settings — кнопка «Re-download whisper» (ручная, не автоматическая, чтобы не тратить трафик).

### L3 — Fallback на CPU при ошибке инициализации
- В `whisper_local.rs::run_whisper_cli_with_progress`: если stderr содержит `cudaGetDeviceCount`, `failed to initialize Vulkan`, `CUDA error`, или process завершился ненулевым кодом с известной паттерн-ошибкой — поймать, эмитить пользователю `[whisper] GPU init failed, falling back to CPU` и автоматически повторить транскрипцию текущего трека на CPU-варианте (если он скачан).
- Опционально: записать `last_known_good_acceleration` в settings — чтобы при следующих запусках не повторять smoke-test.

## 2.3. Файлы

- Новый: `src-tauri/src/gpu_detect.rs`
- Правка: `src-tauri/src/settings.rs`, `src/types/settings.ts`, `src-tauri/tools.manifest.json`, `src-tauri/src/tool_manifest.rs`, `src-tauri/src/tool_download.rs`, `src-tauri/src/whisper_local.rs`, `src-tauri/src/lib.rs` (новая команда `detect_gpu`).
- UI: `src/components/SettingsPanel.tsx`, `src/components/OnboardingWizard.tsx`.

## 2.4. Тест-план Wave 2

**На разработческой машине (NVIDIA RTX 3060 Ti, Win11):**
1. `cargo test --no-default-features --lib` — все тесты зелёные, новые тесты для `gpu_detect` (мокать WMI ответ через trait или просто проверить классификатор по строке).
2. `npm run tauri dev`. Settings → Whisper acceleration → Auto. В UI должен быть хинт «Detected NVIDIA — using CUDA».
3. Settings → Re-download Whisper. Скачивает cuBLAS-zip (~150 MB).
4. После распаковки — проверить, что в папке `%APPDATA%\com.v2t.app\bin\whisper\` лежат `cudart.dll`, `cublas64_*.dll`, `whisper-cli.exe`.
5. Транскрипция тестового ролика 5 мин с моделью `large-v3-turbo`:
   - На CPU baseline ≈ 3 минуты.
   - На CUDA должно быть ≤30 секунд.
6. Проверить fallback: вручную удалить `cudart.dll`, попробовать транскрипцию — ожидание: ошибка обнаружена, делается одна попытка, появляется сообщение про fallback, транскрипция продолжается на CPU.

**На машине без NVIDIA (если доступна для теста):**
- Auto должен выбрать Vulkan если есть совместимый GPU, иначе CPU.

## 2.5. Acceptance

- [ ] WMI/детект корректно различает Nvidia / Amd / Intel / None.
- [ ] Settings → Whisper acceleration работает и сохраняется между запусками.
- [ ] Скачивание выбирает правильный zip с проверкой SHA-256.
- [ ] На NVIDIA RTX 3060 Ti `large-v3-turbo` транскрипция 5-минутного аудио ≤30 сек (vs 3 мин CPU).
- [ ] При искусственно сломанной CUDA-инсталляции — graceful fallback на CPU без зависания, с понятным сообщением в логе.

## 2.6. Коммит

```
feat(whisper): GPU acceleration (CUDA / Vulkan) with autodetect and fallback

Add WhisperAcceleration setting (Auto/CUDA/Vulkan/CPU) and gpu_detect
module. Extend tools.manifest.json with cuBLAS and Vulkan whisper.cpp
builds. Wire up Settings UI radio + onboarding hint. Implement CPU
fallback in whisper_local on GPU init failure.

Release v1.5.0-rc2.
```

**Reference:** детальная спека L1/L2/L3 в `docs/PLAN_NEXT.md` (секция «L — GPU acceleration»).

---

# Wave 3 — Структурный progress UI + локальные имена файлов  ✅ done (1.5.0-rc3, 2026-04-29)

**Цель.** Под каждой active-job-карточкой — прогресс-бар, счётчик `N/M`, ETA, краткая фаза. Лог становится collapsible, шумные `[download] X%` строки скрыты по дефолту. Локальные файлы показываются с именами (`basename`).

**Зависимости.** Wave 1.

**Объём.** ~1.5-2 дня.

## 3.1. Контекст

Сейчас `queue-job-progress` payload — `{ jobId, phase, message }` (`job.rs:50-56`). Listener в `QueuePanel.tsx:144-151` рендерит как `[phase] message`. Это плоский лог; нет ни прогресс-баров, ни структуры.

Парсер `yt_dlp_progress::parse_yt_dlp_line` в текущей реализации возвращает `Option<String>` — структуру теряем сразу же. Надо вернуть structured event.

## 3.2. Что сделать

### J1.1 — Backend: structured event
В `yt_dlp_progress.rs` заменить возвращаемое значение на enum:
```rust
pub enum YtDlpEvent {
    Item { n: u32, total: u32 },
    Progress { percent_bucket: u8, raw_tail: String },  // tail = "of 47.85MiB at 2.34MiB/s ETA 00:25"
    ExtractAudio,
    Merger,
}
pub fn parse_yt_dlp_line(line: &str) -> Option<YtDlpEvent> { ... }
```
Перетестировать существующие тесты под новый shape.

### J1.2 — Backend: расширить payload
В `pipeline.rs::spawn_pipe_reader` (внутри `run_yt_dlp_streaming`) при матче события эмитить:
```rust
struct QueueJobProgressEmit {
    job_id: String,
    phase: String,            // "yt-dlp" | "yt-dlp-video"
    message: String,          // human-friendly fallback
    subtask_index: Option<u32>,
    subtask_total: Option<u32>,
    subtask_percent: Option<u8>,
}
```
Обновлять running state: при `Item { n, total }` запоминать текущие `n`/`total` в локальной переменной задачи; при `Progress { percent_bucket, … }` использовать ранее запомненные `n`/`total`. Передавать в payload.

### J1.3 — Frontend: рендер прогресса
В `QueuePanel.tsx` — изменить листенер `queue-job-progress`:
- Поверх `appendLog(...)` (оставить для полноты лога) — обновлять состояние активного job: `currentSubtask`, `currentPercent`, `totalSubtasks`.
- Под job-карточкой рендерить:
  ```
  [▓▓▓▓░░░░░░] 4/132 · 47% · 2.34 MiB/s · ETA 0:18  [Show log ▾]
  ```
- Прогресс-бар: HTML `<progress>` или div со width%.
- ETA — приближённо `(total - n) / progress_per_sec` (если есть `n` и время с начала job).

### J1.4 — Лог становится collapsible
По дефолту лог-блок свёрнут. Переключатель «Show log ▾ / Hide log ▴». Чекбокс «Show download percentages» (default off) — фильтрует строки `[yt-dlp] N% of …` из видимой части лога (но не удаляет из state).

### J4 — Имена локальных файлов
В `QueuePanel.tsx` (или `lib/queueUtils.ts::shortLabel`) — для job с `kind: "file"` показывать `basename(source)` без расширения. Без родительского каталога. Помещается в существующее место `displayLabel`.

## 3.3. Файлы

- Правка: `src-tauri/src/yt_dlp_progress.rs` (enum + tests), `src-tauri/src/pipeline.rs` (использование enum, state в reader-task), `src-tauri/src/job.rs` (если расширяется payload — обновить тип `QueueJobProgress`).
- Правка: `src/components/QueuePanel.tsx`, `src/lib/queueUtils.ts` (для J4).
- Возможно: новый компонент `src/components/JobProgressBar.tsx`.
- Тесты: обновить `yt_dlp_progress::tests` под новый enum, добавить smoke в `QueuePanel.test.tsx` (если он существует).

## 3.4. Тест-план Wave 3

1. `cargo test --no-default-features --lib` — все зелёные, новые ассерты на `YtDlpEvent::Item`/`Progress`.
2. `npm run test:run` — все зелёные.
3. `npm run tauri dev`. В Queue — плейлист на ≥30 видео.
4. **Ожидание:** под job-карточкой виден прогресс-бар, обновляется счётчик «N/M», текущий процент, ETA. Лог свёрнут. По кнопке «Show log» — открывается, без `X%` строк (если чекбокс не включён). С чекбоксом — все строки.
5. Локальная папка с 5 файлами: каждая job-карточка показывает basename без расширения.

## 3.5. Acceptance

- [ ] `YtDlpEvent` enum + parser возвращают structured события; 7+ тестов зелёные.
- [ ] Payload `queue-job-progress` содержит `subtask_index/total/percent`.
- [ ] UI рендерит прогресс-бар в реальном времени, плавно обновляется на больших плейлистах (без stutter).
- [ ] Лог collapsible, фильтр работает.
- [ ] Локальные файлы показываются с basename без расширения.

## 3.6. Коммит

```
feat(ui): structured per-item progress, ETA, collapsible log

Replace string-based parser output with YtDlpEvent enum. Extend
queue-job-progress payload with subtask_index/total/percent so UI can
render a progress bar and ETA per job. Default-hide noisy [download] %
lines in the log; toggle to show. For file-kind jobs, show basename
without extension instead of full path.

Release v1.5.0-rc3.
```

**Reference:** детальная спека J1, J4 в `docs/PLAN_NEXT.md` (секция «Структурный прогресс UI»).

---

# Wave 4 — Имена плейлиста, кликабельные ссылки, retry per-item  ✅ done (1.5.0, 2026-04-29)

**Цель.** Заголовок job — настоящее имя YouTube-плейлиста. Subtask-список с человеческими именами роликов, статус-иконками, кликабельными ссылками, кнопкой retry на упавших.

**Зависимости.** Wave 3 (subtask state в UI).

**Объём.** ~1.5 дня.

## 4.1. Что сделать

### J2.1 — Pre-resolve через yt-dlp metadata
В `pipeline.rs` перед `prepare_media_audio` (только для URL-jobs) — новый шаг pre-resolve:
```bash
yt-dlp --flat-playlist --dump-single-json --encoding utf-8 <url>
```
Возвращает JSON: `{ title, entries: [{ id, title, url, playlist_index }, …] }`.

Запускать через `run_yt_dlp_streaming` с heartbeat 60s. Best-effort — при ошибке (приватный плейлист, нет cookies, сеть, single video URL) тихо пропустить, дать pipeline идти дальше с пустым subtask-списком.

Десериализация — новый модуль `src-tauri/src/yt_dlp_metadata.rs`:
```rust
#[derive(Deserialize)]
pub struct PlaylistInfo {
    pub title: Option<String>,
    pub entries: Option<Vec<PlaylistEntry>>,
}
#[derive(Deserialize)]
pub struct PlaylistEntry {
    pub id: String,
    pub title: Option<String>,
    pub url: Option<String>,
    pub playlist_index: Option<u32>,
}
```

### J2.2 — Событие playlist-resolved
Новое событие Tauri:
```typescript
{ jobId, playlistTitle, subtasks: [{ id, index, title, originalUrl }] }
```
Эмитится из `job.rs::run_process_queue_item` после успешного pre-resolve, до `prepare_media_audio`.

### J2.3 — UI: заголовок + список subtasks
В `QueuePanel.tsx` listener `playlist-resolved`:
- Обновить displayLabel job на `<playlistTitle> (N видео)`.
- Заполнить `subtasks` массив в state job.

Под job-карточкой (после прогресс-бара из Wave 3) — список subtasks. Изначально все `pending`. По мере прогресса (см. ниже) меняются статусы.

### J3.1 — Статус-иконки subtasks
Каждый subtask отрисован как строка:
```
✓ 1. Урок 1: Введение                   [link]
✓ 2. Урок 2: Основи                     [link]
⏭ 3. Урок 3: Практика  (already done)   [link]
✗ 4. Урок 4: Дебаг  (download error)    [link][↻ retry]
▶ 5. Урок 5: Розбір  ▓▓▓░░ 47%  ETA 0:18  [link]
⏸ 6-132. (pending)
```
- `⏸ pending` (default)
- `▶ running` (обновляется по `subtaskIndex`)
- `✓ done`
- `⏭ skipped` (см. ниже)
- `✗ error`

### J3.2 — Subtask skipped (resume → grey)
В `job.rs::run_process_queue_item`, в ветке resume (`job.rs:362-381` — где транскрипт уже существует), эмитить новое событие:
```typescript
{ jobId, subtaskIndex, status: "skipped", reason: "already done" }
```
UI рендерит ⏭ серым, **не как ошибку**.

### J3.3 — Кликабельные ссылки
В UI subtask заголовок — `<a>` с `originalUrl`. Клик — открывает через `tauri-plugin-opener` (`open_url` команда; плагин уже подключён в `lib.rs:239`).

```tsx
import { open } from "@tauri-apps/plugin-opener";
<a onClick={() => open(subtask.originalUrl)}>{subtask.title}</a>
```

### J3.4 — Retry per-item
Кнопка `↻` рядом с subtask в статусе `error`. Нажатие → re-enqueue новой URL-job на одну ссылку (`subtask.originalUrl`).

URL вида `https://www.youtube.com/watch?v={id}` уже сам по себе single-video в `youtube_watch_url_should_use_no_playlist` (`pipeline.rs:70-90`) — `--no-playlist` не нужен, добавится автоматически когда мы детектим `watch?` без `list=`. Безопаснее всего на UI-стороне: при retry конструировать URL без `list=`, не передавать оригинальный playlist URL.

## 4.2. Файлы

- Новый: `src-tauri/src/yt_dlp_metadata.rs`.
- Правка: `src-tauri/src/pipeline.rs` (вызов pre-resolve), `src-tauri/src/job.rs` (эмит `playlist-resolved`, `subtask-skipped`), `src-tauri/src/lib.rs` (если нужен новый event channel).
- UI: `src/components/QueuePanel.tsx`, новый `src/components/SubtaskRow.tsx`, `src/components/SubtaskList.tsx`.
- Тесты: `yt_dlp_metadata::tests` (десериализация JSON-фикстуры), UI-snapshot для SubtaskRow.

## 4.3. Тест-план Wave 4

1. Юнит-тесты: `cargo test --no-default-features --lib` + `npm run test:run` зелёные.
2. UI-тест: плейлист с известным заголовком (например, `https://www.youtube.com/playlist?list=PLBe1Ggo7wKDkLn...`) — заголовок job меняется на настоящий.
3. Список subtasks показывает 132 строки с именами вместо id.
4. По мере прогресса — статусы обновляются: `▶ running` на текущем, `✓ done` на завершённых.
5. Если транскрипт ролика 5 уже лежит в output_dir → видно `⏭ skipped (already done)` серым.
6. Клик по subtask-заголовку — открывается видео в системном браузере.
7. Симулировать ошибку (например, удалить yt-dlp на середине прогона; или подать URL приватного видео в плейлисте) → видна ✗, кнопка retry, нажатие создаёт новую job на одну ссылку.

## 4.4. Acceptance

- [ ] На публичном плейлисте 132 видео заголовок job — настоящее имя плейлиста.
- [ ] Все subtasks показаны с человеческими именами (не id).
- [ ] Статусы обновляются в реальном времени.
- [ ] Resume отображается как ⏭ skipped серым.
- [ ] Клик по любому subtask-заголовку открывает видео в браузере.
- [ ] Retry на упавшем subtask создаёт новую job на одну ссылку, текущий плейлист не тревожится.
- [ ] Если pre-resolve упал (нет интернета / приватный) — pipeline продолжается, subtasks показаны с id вместо имён, без падения.

## 4.5. Коммит и релиз

```
feat(ui): playlist titles, named subtasks, clickable links, retry per-item

Pre-resolve playlist metadata via yt-dlp --flat-playlist --dump-single-json.
Emit playlist-resolved event before download. UI shows playlist title
and per-video subtask list with status icons (▶✓⏭✗⏸), clickable
links to open in browser, retry button for failed items that re-enqueues
one URL with --no-playlist semantics.

Resume detection in job.rs emits subtask-skipped event so UI can render
⏭ skipped (already done) in grey.

Release v1.5.0.
```

**Reference:** детальная спека J2, J3 в `docs/PLAN_NEXT.md` (секция «Структурный прогресс UI»).

---

# Wave 5 — Subtitles fast-path  ✅ done (1.6.0, 2026-04-29)

**Цель.** Если у YouTube-видео есть качественные manual subtitles на нужном языке — пропускать download+Whisper и взять готовый текст. Экономия — десятки раз для образовательного контента с проф. субтитрами.

**Зависимости.** Wave 1.

**Объём.** ~1 день.

## 5.1. Контекст

YouTube субтитры — два вида:
- **Manual** (загруженные автором): обычно качественные, есть пунктуация и тайминги.
- **Auto-generated** (YouTube ASR): на UA/RU заметно хуже Whisper-medium+; **по дефолту НЕ использовать**.

yt-dlp уже умеет всё это:
```bash
# Probe
yt-dlp --list-subs <url>

# Download manual UA, fallback EN
yt-dlp --write-subs --sub-langs uk,en --skip-download --convert-subs srt --no-write-auto-subs <url>
```

## 5.2. Что сделать

### K1 — Settings
Поле `use_subtitles_when_available: bool` (default false), `subtitle_priority_langs: Vec<String>` (default `["uk", "ru", "en"]`). UI в SettingsPanel — секция «Subtitles fast-path».

### K2 — Probe и решение в pipeline
В `job.rs::run_process_queue_item`, перед `prepare_media_audio` (для URL-jobs):
1. Если `use_subtitles_when_available = false` — пропустить шаг.
2. Иначе вызвать `yt-dlp --list-subs --print '%(subtitles)s'` (или `--dump-json`, который содержит `subtitles` поле).
3. Если есть **manual** subs на любом из приоритетных языков — пометить трек для sub-pipeline (см. K3) вместо обычного prepare→transcribe.
4. Если только **auto** subs или нет — обычный pipeline.

### K3 — Sub-pipeline
Для треков, выбравших sub-fast-path:
1. `yt-dlp --write-subs --sub-langs <lang> --skip-download --convert-subs srt --no-write-auto-subs <url>`
2. Прочитать .srt, конвертировать в plain text (вырезать тайминги и индексы), записать в `dest_path` (тот же шаблон имени, что и обычная транскрипция).
3. Эмитить событие `[subs] track N/M: from manual subs (<lang>)`.
4. UI subtask-row: показать `📝 from subs (uk)` вместо `🎤 transcribed`.

### K4 — Сохранение srt (опционально)
Если в Settings включено `keep_srt: bool` (default false) — рядом с .txt класть оригинальный .srt с теми же таймингами.

## 5.3. Файлы

- Новый: `src-tauri/src/subs.rs` (probe + sub-pipeline + srt→txt конверт).
- Правка: `src-tauri/src/settings.rs`, `src/types/settings.ts`, `src-tauri/src/job.rs` (decision branch), `src/components/SettingsPanel.tsx`, `src/components/SubtaskRow.tsx` (новая иконка).

## 5.4. Тест-план Wave 5

1. Юнит-тесты для srt-парсера (фикстура с заведомо известным .srt → ожидаемый plain text).
2. UI-тест на видео с известными manual UA-subs — Whisper не запускается, текст готов за <5 сек, в UI subtask `📝 from subs (uk)`.
3. UI-тест на видео без manual subs — обычный pipeline (`prepare → transcribe`).
4. UI-тест на ролике с только auto-subs — обычный pipeline (auto-subs не используются по дефолту).

## 5.5. Acceptance

- [ ] Settings toggle и priority langs работают и сохраняются.
- [ ] Probe не блокирует pipeline дольше 30 сек на одиночное видео.
- [ ] На ролике с manual UA-subs обработка занимает <10 сек (vs минуты на Whisper).
- [ ] srt → txt конверт корректно убирает тайминги и индексы, оставляет читаемый текст.
- [ ] Выбор сделан правильно: manual=use, auto=skip-by-default.

## 5.6. Коммит

```
feat(pipeline): YouTube subtitles fast-path

When manual subtitles exist in priority languages, skip download +
Whisper and use yt-dlp --write-subs to fetch and convert SRT to plain
text. Auto-generated subs are ignored by default (lower quality than
Whisper for non-English).

Adds use_subtitles_when_available setting and subtitle_priority_langs
list. Per-subtask UI marker 📝 from subs (lang) distinguishes from
🎤 transcribed.

Release v1.6.0.
```

**Reference:** детальная спека K в `docs/PLAN_NEXT.md` (секция «Subtitles fast-path» — добавить, если ещё нет).

---

## 4. Финальный чеклист агента (для каждой волны)

После завершения волны:
- [ ] Все юнит-тесты зелёные (`cargo test --no-default-features --lib`, `npm run test:run`).
- [ ] `cargo build --no-default-features` без warnings.
- [ ] Smoke-тест в `npm run tauri dev` пройден (см. тест-план волны).
- [ ] `CHANGELOG.md` обновлён.
- [ ] Версии в `package.json`, `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json` синхронизированы.
- [ ] Один коммит с сообщением по шаблону волны.
- [ ] `docs/PLAN_NEXT.md` обновлён — задачи помечены как done в сводной таблице.
- [ ] `docs/WAVES.md` (этот файл) — статус волны помечен как done.

## 5. Итоговая таблица волн

| Wave | Задачи | Версия | Статус | Зависит от |
|---|---|---|---|---|
| 1 | G + H + I + I+ | v1.5.0-rc1 | ✅ done | — |
| 2 | L1 + L2 + L3 | v1.5.0-rc2 | ✅ done | 1 |
| 3 | J1 + J4 | v1.5.0-rc3 | ✅ done | 1 |
| 4 | J2 + J3 | v1.5.0 | ✅ done | 3 |
| 5 | K | v1.6.0 | ✅ done | 1 |

Полный путь от 1.4.0 до 1.6.0: ~1 рабочая неделя сосредоточенного кодинга.
