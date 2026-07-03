# Changelog

Формат основан на [Keep a Changelog](https://keepachangelog.com/ru/1.1.0/).

## [2.0.6] - 2026-07-03

### Изменено

- **YouTube: `watch?v=…&list=…` скачивает весь плейлист.** Раньше для ссылок из браузера принудительно ставился `--no-playlist` и обрабатывалось только одно видео из `v=`, хотя UI показывал полный список. Теперь поведение совпадает с `playlist?list=…` — все ролики в очереди и в выводе.

### Исправлено

- **Чеклист после «Загрузить ffmpeg и yt-dlp».** Список готовности обновляется сразу после установки медиа-инструментов, без перезапуска приложения.

## [2.0.5] - 2026-06-17

### Исправлено

- **TikTok: корневая причина video-only.** yt-dlp по умолчанию брал HEVC (`bytevc1_1080p`, ~8.35 MiB) — в скачанном MP4 нет воспроизводимой аудиодорожки, хотя в списке форматов `acodec=aac`. Для TikTok-URL теперь сразу используется селектор H.264 + AAC (`b[vcodec^=avc1]/…/download/b`). Локально проверено на `https://vt.tiktok.com/ZSQXqHkn8` — WAV ~4.3 MB / ~134 s.

## [2.0.4] - 2026-06-17

### Исправлено

- **TikTok / DASH: merge-retry теперь срабатывает без ffprobe.** Если рядом с `ffmpeg` нет `ffprobe.exe`, проверка аудиодорожки идёт через `ffmpeg -i` (разбор stderr). При video-only файле после `ba/b` автоматически запускается повторная загрузка `bv*+ba/b`.

## [2.0.3] - 2026-06-17

### Исправлено

- **TikTok / DASH: video-only fallback больше не ломает нормализацию.** Если `yt-dlp -x` упал на постпроцессинге, повторная загрузка теперь идёт как **best audio** (`-f ba/b`). Если источник отдаёт раздельные потоки и получается файл без аудиодорожки — выполняется второй retry с **merge video+audio** (`-f bv*+ba/b --merge-output-format mp4`). Дополнительно ffmpeg-нормализация теперь явно маппит аудио (`-vn -map 0:a:0`) и даёт понятную ошибку, если аудио отсутствует.

## [2.0.2] - 2026-05-26

### Исправлено

- **TikTok URL больше не падают на шаге `yt-dlp -x`, если постпроцессор не может определить аудиокодек через `ffprobe`.** При ошибке `unable to obtain file audio codec with ffprobe` pipeline теперь автоматически повторяет загрузку без `-x` и извлекает аудио уже своим `ffmpeg`, поэтому транскрибация продолжается вместо фатального `yt-dlp failed (exit 1)`.
- **Сохранение скачанного аудио после fallback.** Когда URL пришлось догружать как исходное медиа без `-x`, шаг сохранения аудио теперь корректно различает уже-готовый аудиофайл и сырой video/media input, чтобы не ломать экспорт рядом с успешной транскрипцией.

## [2.0.1] - 2026-05-22

### Исправлено

- **API-ключи не сохранялись после перезапуска.** Crate `keyring` подключался без платформенного бэкенда (`keyring = "3"`), из-за чего в 3.x использовалось mock-хранилище в памяти: запись «успешна», но в никуда. После рестарта ключ читался как пустой. Включены нативные бэкенды (`features = ["windows-native", "apple-native"]`) → ключи теперь реально пишутся в Windows Credential Manager и macOS Keychain. Чинит сразу оба ключа: транскрипционный (`api_key`) и Vision/OCR Gemini (`gemini_api_key`).

> После обновления ключ нужно ввести заново один раз — ранее «потерянные» ключи физически нигде не сохранялись.

## [1.9.0] - 2026-05-21

### Добавлено

- **UI-панель управления REST API** (Settings → секция «REST API»): тумблер вкл/выкл, поле порта, показ/скрытие и копирование bearer-токена (моноширинный), кнопка регенерации токена, статус-пип (running/stopped) со ссылкой на Swagger UI, кнопка «Apply / restart server». Больше не нужно править `settings.json` руками. Новая Tauri-команда `api_server_regenerate_token`.
- **M4 — SQLite-персистентность API** (`app_data/v2t-api.db`, WAL): джобы и батчи пишутся через write-through и переживают рестарт v2t. При следующем старте история восстанавливается; задачи, бывшие в работе на момент остановки, помечаются новым статусом **`interrupted`**. До открытия БД (и в тестах) реестр работает чисто в памяти — деградация безопасная.

### Изменено

- `ApiJobStatus` получил вариант `interrupted` (в агрегате батча считается вместе с `failed`, чтобы суммы сходились).

## [1.8.2] - 2026-05-21

### Изменено — UI polish pass

Косметический проход без изменения функциональности: добавлены микровзаимодействия и наведён порядок в шапке. Линза — design-critique + принципы Эмиля Ковальски (Sonner/Vaul) + базовый WCAG-контраст.

- **Motion**: нажатие кнопок даёт отклик (`:active` scale 0.97) + плавные ховеры; клавиатурные фокус-кольца (`:focus-visible`) на всех контролах; мягкое появление модалки онбординга (scale-in + fade) и затемнения фона; въезд тоста; подсветка drop-zone на наведении; плавное подчёркивание вкладок. Всё уважает `prefers-reduced-motion`.
- **Readiness-панель сворачивается** в одну зелёную строку, когда все проверки пройдены (клик — развернуть). Освобождает ~половину окна 800×600, поднимая очередь/`Start queue` ближе к верху.
- **Бейдж версии/API** переехал в один ряд с кнопками шапки (раньше занимал отдельную строку); статус API — цветной пип, читаемый контраст.
- **Контраст**: подняты слишком тёмные серые тексты (`#6b7080` → `#8b8fa3`) до WCAG AA.

## [1.8.1] - 2026-05-19

### Добавлено

- **Бейдж версии в header** — внизу шапки показывается `v<версия>` (читается из бандла через `getVersion()`, рантайм-источник правды о запущенной сборке) и статус REST API: `API :8788 ●` зелёным, когда сервер слушает, либо `API off`. Тултип на активном бейдже ведёт на `/v1/docs`. Способ глазами убедиться, какая именно сборка запущена и поднялся ли API.

### Изменено / исправлено

- **Честный статус API-сервера**: bind сокета теперь синхронный в `apply_settings` — занятый порт (или любая ошибка bind) сразу возвращается в `api_server_apply` и виден в UI/логе, а флаг «running» выставляется только после успешного bind. Раньше при занятом порте бейдж/статус мог соврать «running», пока сервер на деле не слушал.
- TS-тип `AppSettings` дополнен секцией `apiServer` — round-trip сохранения настроек из UI больше гарантированно не теряет конфиг сервера.

## [1.8.0] - 2026-05-19

### Добавлено

- **Локальный REST API** — пока v2t запущен, поднимается HTTP-сервер на `127.0.0.1:<port>` (по умолчанию `8788`). Внешний сервис ставит задачи по URL или локальному файлу, опрашивает статус, забирает текст транскрипта, получает webhook на завершение. Подробная инструкция и примеры (curl, Python, sseclient, Flask-приёмник вебхуков): [`docs/API.md`](docs/API.md). Включается в `settings.json` секцией `apiServer: { enabled: true }` (или через Tauri-команду `api_server_apply`) — bearer-токен (32 байта hex) генерируется автоматически при первом enable.
- **Эндпоинты v1**: `GET /v1/health`, `POST /v1/jobs`, `GET /v1/jobs/{id}`, `GET /v1/jobs/{id}/transcript`, `POST /v1/jobs/{id}/cancel`, `GET /v1/jobs/{id}/events` (SSE), `POST /v1/batches` (до 1000 items), `GET /v1/batches/{id}` (агрегированный статус). Атомарная валидация батча: ошибка в N-м item откатывает уже зарегистрированные. Глобальный лимит конкуррентных запусков — 2 (хардкод M3, далее настраивается).
- **Webhook delivery** — POST на `callback.url` при достижении терминального статуса. Идемпотентный `X-V2T-Delivery-Id` (UUID), HMAC-SHA256 в `X-V2T-Signature` по `callback.secret`. 3 попытки с экспоненциальной задержкой, 4xx не ретраится.
- **Swagger UI и OpenAPI 3.1** — без auth по `/v1/docs` и `/v1/openapi.json`. Кнопка Authorize в UI принимает bearer-токен; спека пригодна для client-codegen через `openapi-generator`.
- **`ProgressSink` trait** — все события прогресса теперь идут через абстракцию (`progress.rs`). Реализации: `TauriSink` (webview + session log, прежнее поведение), `ApiJobSink` (REST registry + broadcast канал для SSE). Открыл возможность подключать произвольные приёмники без правки бизнес-кода.

### Изменено

- `job::run_process_queue_item` принимает `sink: SinkHandle` параметром (раньше внутри строил `TauriSink` из `AppHandle`).
- Tauri-команды `process_queue_item`, `download_whisper_model`, `download_media_tools`, `download_whisper_cli`, `install_deno`, `browser_queue_job_finish` теперь конструируют `TauriSink` в `lib.rs` и пробрасывают вниз — wire-формат событий в JS остаётся прежним.
- `AppSettings` получил секцию `apiServer: { enabled, port, bearerToken }` (всё с serde defaults — старые `settings.json` подгрузятся без правок).

### Известные ограничения / scope M2-M3.5

- **In-memory state** — батчи и история джобов живут только пока v2t запущен; рестарт = очистка. SQLite-персистентность запланирована на M4.
- **Только localhost** — bind строго на `127.0.0.1`, без LAN-доступа и без CORS. Для удалённого потребителя нужен ваш собственный backend в качестве прокси.
- **Hardcoded concurrency = 2** — больше нагружать локальную машину параллельным yt-dlp / ffmpeg / whisper рискованно. Настройка появится в M4.
- **`browserWhisper` режим** недоступен через REST (нужен webview).

## [1.7.0] - 2026-04-30

### Добавлено

- **Локализация UI на 8 языках** (Wave 6 / M1-M7). Полный i18n-стек на `react-i18next` с типизированными ключами (`t("typo")` падает в TS), Vite glob-loader для авто-подхвата новых каталогов, custom detector завязанный на Tauri-backed `settings.json` (один источник правды). Каталоги — per-component: `src/locales/{en,uk,ru,de,es,fr,pl,pt}/{common,onboarding,settings,queue,readiness}.json` (8 локалей × 5 namespaces = 40 файлов, **344 ключа** на локаль = **2752 переведённых строк**).
- **Двойной переключатель языка**: компактный `<select>` в header (~80px, флаг + ISO 2-буквы) и полный в Settings → секция «Language» (Українська, Polski, Português, …). Auto-режим (default) читает `navigator.language` и подбирает локаль; на непокрытом языке — fallback на EN. Поле `uiLanguage: UiLanguage` в `AppSettings` (Rust enum + TS union, 9 вариантов с auto), persist мгновенный без Save.
- **Translation-bot, переиспользованный из NumbersM** (`scripts/translation-bot/`). Local Ollama (Qwen2.5-Coder:7b на RTX 3060 Ti, ~1.7s/строка) с adaptированным под v2t prompt'ом (Tauri desktop transcriber, technical audience, formal-you), v2t-glossary (yt-dlp, ffmpeg, Whisper, CUDA, mp3, …), валидация PLACEHOLDER_DRIFT / GLOSSARY_LOST / LENGTH_OUT_OF_BAND / CHECK_REQUESTED. Bot за 1ч13м (на 8GB VRAM RTX 3060 Ti) перевёл все 7 целевых локалей одной overnight-сессией; merge-script (`scripts/merge-i18n-drafts.mjs`) расфасовал flat-keys драфты в per-namespace catalogs. Драфты остаются в `output/drafts/` (gitignored) для повторного merge после правок.
- **CI-gate `npm run check:i18n`** (`scripts/check-i18n-keys.mjs`) — проверяет, что каждый ключ из `en/*.json` присутствует и непуст в каждой целевой локали. Default — advisory; `CHECK_I18N_STRICT=1` блокирует build при пропусках.
- Все 8 UI-компонентов (`OnboardingWizard` 1071 LOC, `SettingsPanel` 1055 LOC, `QueuePanel` 857 LOC, плюс `App.tsx`, `ReadinessPanel`, `DependencyBar`, `SubtaskRow`, `JobProgressBar`) переведены на `t("key")` / `<Trans i18nKey="..." components={{strong, code, a}}>`. Inline JSX-литералов длиннее двух слов в коде нет.

### Изменено

- Vitest setup (`src/test/setup.ts`) импортирует `./i18n` до component render — без этого `useTranslation()` возвращал raw keys и text-matchers ломались.
- Все vitest-тесты переведены на `data-testid` / `data-attr` matchers — language-independent. Новые testid: `cloud-credential-store-hint`, `readiness-open-settings`, `queue-panel`, `data-queue-running` атрибут на queue-panel.
- Header — табы получили `data-testid="tab-queue"` / `tab-settings` для устойчивых e2e-проверок.

### Известные ограничения

- Bot оставил ~428 advisory warnings в драфтах (CHECK_REQUESTED 25-29% в UA/PL — модель самопометила context-poor короткие ключи). UA + RU прошли human review; **DE/ES/FR/PL/PT** ждут bug-репортов от пользователей, которые там работают (MIT-проект, ограниченный bandwidth ревьюеров).
- Backend (Rust) error messages и log-строки `[yt-dlp] X% …` / `[ffmpeg] …` намеренно остаются английскими — это технический поток, идёт в support-тикеты.

## [1.6.0] - 2026-04-29

### Добавлено

- **Subtitles fast-path для YouTube** (Wave 5 / K). Новые настройки `useSubtitlesWhenAvailable` (default `false`), `subtitlePriorityLangs` (default `["uk", "ru", "en"]`), `keepSrt` (default `false`). Когда настройка включена и у одиночного видео есть **ручные** субтитры в одном из приоритетных языков, pipeline пропускает скачивание + Whisper и сразу забирает `.srt` через `yt-dlp --write-subs --skip-download --convert-subs srt --no-write-auto-subs`, конвертирует в plain text и сохраняет как обычный транскрипт. Auto-generated captions намеренно игнорируются (на UA/RU они стабильно хуже Whisper-medium). Plain-playlist URL'ы (`youtube.com/playlist?list=...`) пропускают fast-path и идут обычным маршрутом — пер-видео probe для 100-элементного плейлиста не оправдан.
- Новый модуль `subs.rs`: `probe_subs` (`yt-dlp --skip-download --dump-json`), `pick_priority_lang` (exact + regional prefix matching), `download_srt`, `srt_to_plain_text` (вырезает индексы, тайминги, inline `<i>`/`{\an8}` теги; склеивает многострочные cue'ы пробелом, разделяет cue'ы переводом строки). Любая ошибка fast-path логируется в фазе `subs` и pipeline продолжается обычным путём — для пользователя это просто медленнее, не fatal.
- В UI: секция Settings → "Subtitles fast-path" с тогглом, инпутом приоритетных языков и опцией keep-srt. В `SubtaskRow` для статуса `done` с reason `from subs (<lang>)` иконка ✓ заменяется на 📝 — видно, какие ролики транскрибированы из субтитров без Whisper.
- Тесты: 16 новых rust-кейсов в `subs::tests` (probe parser, выбор языка с приоритетами и regional prefix, srt parser с CRLF / Unicode / inline-тегами / пустыми cue'ами, `is_pure_playlist_url` детектор), 1 новый settings-test (default langs при отсутствии поля), 1 новый vitest для секции Settings.

## [1.5.0] - 2026-04-29

### Добавлено

- **Имена плейлиста и список subtasks под джобой** (Wave 4 / J2). Перед скачиванием запускается pre-resolve `yt-dlp --flat-playlist --dump-single-json --encoding utf-8 -- <url>` (новый модуль `yt_dlp_metadata`); полученные `title` / `entries` уходят в UI новым событием `playlist-resolved`. В заголовке джобы отображается `<playlist title> (N videos)`, под прогресс-баром — список роликов с реальными именами вместо id. Pre-resolve полностью best-effort: ошибка (приватный плейлист, single video, нет интернета, yt-dlp version mismatch) логируется в фазе `yt-dlp-meta` и не прерывает основной pipeline.
- **Per-subtask статус-иконки** (J3): `⏸ pending` / `▶ running` / `✓ done` / `⏭ skipped` / `✗ error`. Бэкенд эмитит новое событие `subtask-status` (поля `subtaskIndex`, `status`, `reason`) на старте/успехе/ошибке транскрипции каждого ролика, плюс отдельно при resume (`skipped`, reason `already done` — серым, не как ошибка). Активный по `queue-job-progress.subtaskIndex` ролик подсвечивается ▶ даже если бэк ещё не прислал явный `running`.
- **Кликабельные ссылки на ролики**: заголовок subtask открывается в системном браузере через `tauri-plugin-opener::openUrl`.
- **Retry per-item**: на упавшем ролике появляется кнопка `↻ retry`, которая ставит в очередь новую URL-джобу с очищенным от `list=` / `index=` / `start_radio=` watch-URL — текущий плейлист не тревожится. Хелпер `stripPlaylistParams` повторяет защиту, которую `youtube_watch_url_should_use_no_playlist` уже даёт на бэкенде (defense in depth).
- Новые компоненты `SubtaskList.tsx`, `SubtaskRow.tsx`, CSS-блок `.subtask-list` / `.subtask-row*` (collapsible-ready под ≥132 ролика, max-height со скроллом).
- Тесты: 6 новых rust-кейсов для `yt_dlp_metadata` (playlist с entries, single-video shape, fallback на watch-URL и id, авто-индекс по позиции, broken JSON).

## [1.5.0-rc3] - 2026-04-29

### Добавлено

- **Структурный per-item progress в Queue** (Wave 3 / J1). Под карточкой работающей джобы — компактная строка `<phase> N/M  PERCENT%  <speed/ETA>` и линейный прогресс-бар, обновляются в реальном времени по `queue-job-progress`. Парсер yt-dlp теперь возвращает структурированный enum `YtDlpEvent` (`Item { n, total }`, `Progress { percent_bucket, tail }`, `ExtractAudio`, `Merger`); payload `queue-job-progress` расширен полями `subtaskIndex` / `subtaskTotal` / `subtaskPercent`. Stdout- и stderr-readers yt-dlp используют общий `SubtaskState`, поэтому индекс и общее число роликов корректно подтягиваются к каждому % события.
- **Collapsible log + фильтр шума.** Лог-блок свёрнут по умолчанию (Show log ▾ / Hide log ▴). Чекбокс «Show download %» (default off) скрывает шумные `[yt-dlp] N% …` строки из видимой части лога — сами строки остаются в state и попадают в Open log file / Copy. Полный лог + sr-only-копия сохраняются для скрин-ридеров.
- **Локальные файлы — basename без расширения** (J4). Drop, Add files, Add folder и folder-scan теперь показывают только имя файла без родительских каталогов и расширения; полный путь — в `title`. Новый хелпер `fileBasenameNoExt` обрабатывает Windows- и Unix-разделители, сохраняет dot-файлы (`.env`).
- Новый компонент `JobProgressBar.tsx` (HTML `<progress>` со стилями), новый CSS-блок `.job-progress*` / `.queue-progress-row`.
- Тесты: 5 новых vitest-кейсов для `fileBasenameNoExt`, 2 новых rust-теста для `YtDlpEvent::short_message` и обновлённые ассерты под новый shape парсера.

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
