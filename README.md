# v2t (Video to Text)

Портативное десктоп-приложение (Tauri 2): видео/аудио (файл, папка, URL) → текст через **ffmpeg**, **yt-dlp** и либо **HTTP API** в стиле OpenAI (`/v1/audio/transcriptions`), либо локально **[whisper.cpp](https://github.com/ggml-org/whisper.cpp)** (`whisper-cli` / `main`) с ggml-моделями. В установщике и заголовке окна — **Video to Text**; исполняемый файл — **`v2t.exe`** (Windows) / **`v2t`** (macOS/Linux).

## Требования для разработки

- [Node.js](https://nodejs.org/) (LTS)
- [Rust](https://rustup.rs/) stable
- На Windows: при сборке обычно нужны [Microsoft C++ Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/)

## Сборка и запуск

```bash
npm install
npm run tauri dev
```

Продакшен-сборка:

```bash
npm run tauri build
```

### Куда класть артефакты и sidecars после сборки

После успешной сборки:

| Что | Где искать (от корня репозитория) |
|-----|-----------------------------------|
| Готовый **EXE** (без установщика) | `src-tauri/target/release/v2t.exe` (Windows) или `src-tauri/target/release/v2t` (macOS/Linux) |
| **Установщик Windows (NSIS)** | `src-tauri/target/release/bundle/nsis/Video to Text_0.2.0_x64-setup.exe` (имя зависит от `version` в `tauri.conf.json`) |
| **Установщик Windows (MSI)** | `src-tauri/target/release/bundle/msi/Video to Text_0.2.0_x64_en-US.msi` |
| **macOS** | `src-tauri/target/release/bundle/dmg/` или `bundle/macos/` (зависит от targets в `tauri.conf.json`) |

**Важно:** резолвер путей (`src-tauri/src/deps.rs`) смотрит каталог **`parent(current_exe)`** — то есть папку, где лежит запущенный **`v2t.exe`** / **`v2t`**, а не исходники проекта.

- **Установка через NSIS/MSI:** обычно программа оказывается в каталоге вроде `C:\Program Files\Video to Text\` (имя папки совпадает с `productName` в `tauri.conf.json`). Положите туда же **`ffmpeg.exe`** и **`yt-dlp.exe`** (или создайте подпапку **`bin\`** с теми же именами).
- **Запуск «портативно» из `target/release`:** положите **`ffmpeg.exe`** и **`yt-dlp.exe`** в **`src-tauri/target/release/`** рядом с **`v2t.exe`**, затем запускайте `v2t.exe` оттуда.
- **Разработка (`npm run tauri dev`):** sidecars кладите рядом с **`v2t.exe`** в **`src-tauri/target/debug/`** (после первой сборки dev).

При необходимости задайте полные пути к инструментам в **Settings** — они имеют приоритет над поиском рядом с EXE.

## Портативная раскладка (sidecars)

Положите бинарники **рядом с исполняемым файлом приложения** (как ожидает резолвер в Rust):

| Платформа | приложение | ffmpeg | yt-dlp |
|-----------|------------|--------|--------|
| Windows   | `v2t.exe` | `ffmpeg.exe` | `yt-dlp.exe` |
| macOS / Linux | `v2t` | `ffmpeg` | `yt-dlp` |

Допустима подпапка **`bin/`** (рядом с EXE) с теми же именами (см. `src-tauri/src/deps.rs`). При необходимости укажите полные пути в **Settings**.

То же правило «рядом с приложением» относится к **`whisper-cli`** (или устаревшему бинарнику **`main`**) в **локальном** режиме транскрипции.

**Windows:** в **Settings** — **Download ffmpeg & yt-dlp for me**: `yt-dlp.exe` с GitHub и **FFmpeg** из zip [BtbN/FFmpeg-Builds](https://github.com/BtbN/FFmpeg-Builds) (GPL), в каталог данных приложения; пути подставляются в настройки.

**macOS:** та же кнопка: `yt-dlp_macos` с GitHub и статический `ffmpeg` для вашей архитектуры (Apple Silicon / Intel) из релизов [eugeneware/ffmpeg-static](https://github.com/eugeneware/ffmpeg-static). После загрузки при предупреждении Gatekeeper может понадобиться разрешить в **Системные настройки → Конфиденциальность и безопасность** или снять quarantine (`xattr`). Либо установите через **Homebrew** и укажите пути вручную.

Во всех случаях можно не использовать кнопку и указать пути в **I’ll install … myself**.

## Локальный режим (whisper.cpp)

1. В **Settings** выберите **Transcription → Local Whisper (whisper.cpp CLI)**.
2. Соберите или скачайте **`whisper-cli`** (см. [whisper.cpp](https://github.com/ggml-org/whisper.cpp)) и положите его рядом с `v2t.exe` **или** укажите полный путь в настройках.
3. Выберите модель (**tiny**, **base**, **small**, **medium**, **large-v3-turbo**). Нажмите **Download / verify model** или запустите очередь: при отсутствии файла модель **скачается** с Hugging Face (`ggerganov/whisper.cpp`), затем проверяется **SHA-1** по таблице из [models/README.md](https://github.com/ggml-org/whisper.cpp/blob/master/models/README.md) whisper.cpp (в коде v2t зашиты URL и ожидаемый хеш; если upstream заменит файл, каталог в приложении нужно обновить).
4. Файлы **`ggml-*.bin`** по умолчанию хранятся в **`app_data_dir/models`** (можно сменить папку в настройках).
5. После первой загрузки модели **API key не нужен**; интернет нужен только для докачки модели.

Транскрипция идёт через subprocess `whisper-cli` (`-m`, `-f`, `-of`, `-otxt`, `-nt`, `-l`); для длинных WAV используется та же нарезка **ffmpeg**, что и для облачного API.

## Настройки и API key (облачный режим)

1. Откройте **Settings** в приложении.
2. Укажите **папку вывода** (куда сохраняются `.txt`).
3. Оставьте **Transcription → HTTP API** и вставьте **API key**; при необходимости измените **API base URL** и **model** (по умолчанию OpenAI-совместимый endpoint).
4. Ключ сохраняется в **хранилище учётных данных ОС** (Windows Credential Manager, macOS Keychain, Secret Service на Linux); в JSON настроек ключ не записывается.

Совместимые провайдеры: любой endpoint с multipart `POST …/audio/transcriptions` и JSON-ответом `{"text":"…"}` (как у OpenAI).

### Где взять API key (облако)

- **Общее:** ключ выдаёт **сайт выбранного провайдера** в разделе вроде «API keys» / «Credentials». В v2t нужен сервис, который принимает тот же формат запроса, что и OpenAI для транскрипции (см. выше).
- **OpenAI (типичный пример):** зарегистрируйтесь на [platform.openai.com](https://platform.openai.com/), при необходимости пополните биллинг, затем раздел [API keys](https://platform.openai.com/api-keys) → **Create new secret key**. В приложении: **API base URL** `https://api.openai.com/v1`, **model** например `whisper-1`. Документация: [Speech to text](https://platform.openai.com/docs/guides/speech-to-text).
- **Другие облака (Azure OpenAI, Groq и т.д.):** ключ и точный **base URL** / **model** берите из портала этого провайдера и его документации — они отличаются от OpenAI.
- В окне **Settings** есть раскрывающаяся справка **«Where do I get an API key?»** (на английском, как и остальной UI).
- **Безопасность:** не публикуйте ключ в репозитории, скриншотах и чатах; при утечке отзовите ключ в кабинете провайдера и создайте новый.

## Ограничения

- **Размер файла для API:** у OpenAI для `whisper-1` действует лимит **25 МБ** на запрос; приложение при превышении порога (~22 MiB сырого WAV) **нарезает** файл через `ffmpeg` на сегменты и склеивает текст.
- **Плейлисты URL:** один элемент очереди может дать **несколько** нормализованных WAV и отдельных `.txt` (плейсхолдер `{track}` в шаблоне имени).
- **Stop queue:** отменяет текущий `job_id` в бэкенде (`cancel_queue_job` + `CancellationToken`): по возможности завершаются дочерние процессы (`taskkill /T` на Windows, `kill -9` на Unix) и прерывается ожидание HTTP; оставшиеся в этом запуске задачи помечаются как отменённые.

## Тесты

```bash
npm run test:run    # Vitest
cd src-tauri && cargo test
npm run e2e         # Playwright (поднимает dev-сервер)
```

## Документация по процессу

- План фаз: [`docs/PLAN.md`](docs/PLAN.md)
- Заметки для агентов: [`AGENTS.md`](AGENTS.md)
