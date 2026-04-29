# TASKS.md — детальный план задач для агентов v2t

Дата: 2026-03-27. Для передачи исполняющим агентам (Cursor, copilot и т.п.).
Каждая задача независима. Агент берёт одну задачу, читает только указанные файлы, делает только указанные изменения.

**Статус (2026-03-28):** в коде уже есть вкладки Queue / Settings (TASK-01), глобальные стили форм и a11y для селектов (TASK-08), часть TASK-09 (лог WASM в сессию), `keep_downloaded_video` и второй проход yt-dlp. Добавлена настройка **`ytDlpJsRuntimes`** → yt-dlp `--js-runtimes` в `prepare_media_audio` и `download_best_video_mp4`. Ниже — исторические спецификации; при расхождении с репозиторием ориентироваться на код и `docs/PLAN_NEXT.md`.

**Новый пакет (2026-04-20):** TASK-11/12/13 — параллелизация pipeline, очереди и chunked-upload. Источник: BA-артефакт `.cursor/tasks/BA-20260420-180000-alice.md` (Алиса). Все 13 OQ закрыты PO. Стартовать с **TASK-11** (главный выигрыш для дефолтного `localWhisper` сценария).

---

## TASK-01 — Вкладки вместо выпадающей панели Settings

**Приоритет:** высокий
**Агент:** kieran-typescript-reviewer или general
**Файлы:** `src/App.tsx`, `src/App.css`

### Что сейчас

В `App.tsx` кнопка `Settings` в хедере переключает `showSettings` (boolean).
`<SettingsPanel>` рендерится/скрывается условно (`{showSettings ? <SettingsPanel .../> : null}`).
Это делает интерфейс запутанным: Settings выпадает в середину страницы поверх очереди.

### Что нужно

Добавить два таба: **Queue** (активный по умолчанию) и **Settings**.
Таб-бар расположен под хедером, над `ReadinessPanel`.
При переключении на Settings — очередь скрыта; при переключении на Queue — настройки скрыты.

### Изменения в `App.tsx`

1. Заменить `showSettings: boolean` на `activeTab: "queue" | "settings"` в useState.
2. Убрать кнопку `Settings` / `Close settings` из `app-header-actions`.
3. Добавить таб-бар между `</header>` и `<ReadinessPanel`:

```tsx
<div className="app-tabs" role="tablist">
  <button
    role="tab"
    className={activeTab === "queue" ? "app-tab app-tab--active" : "app-tab"}
    aria-selected={activeTab === "queue"}
    onClick={() => setActiveTab("queue")}
  >
    Queue
  </button>
  <button
    role="tab"
    className={activeTab === "settings" ? "app-tab app-tab--active" : "app-tab"}
    aria-selected={activeTab === "settings"}
    onClick={() => setActiveTab("settings")}
  >
    Settings
  </button>
</div>
```

4. Обернуть `<main className="main-workspace">` в `{activeTab === "queue" && ...}`.
5. Обернуть `<SettingsPanel .../>` в `{activeTab === "settings" && ...}` — убрать условие `showSettings`.
6. `ReadinessPanel` должен оставаться видимым на обоих табах.
7. Кнопка `onOpenSettings` в `ReadinessPanel` и `OnboardingWizard` → теперь вызывает `setActiveTab("settings")` вместо `setShowSettings(true)`.
8. Из `OnboardingWizard` убрать проп `onOpenSettings` если он только переключал настройки — теперь это `setActiveTab`.

### Изменения в `App.css`

Добавить стили таб-бара в конец файла:

```css
.app-tabs {
  display: flex;
  gap: 0;
  border-bottom: 1px solid #2a2e3a;
  margin-bottom: 0.85rem;
}

.app-tab {
  background: transparent;
  border: none;
  border-bottom: 2px solid transparent;
  border-radius: 0;
  color: #8b8fa3;
  font-size: 0.88rem;
  font-weight: 500;
  padding: 0.5rem 1rem 0.45rem;
  cursor: pointer;
  margin-bottom: -1px;
}

.app-tab:hover {
  color: #c5c8d4;
  border-bottom-color: #3d4456;
}

.app-tab--active {
  color: #e8e8ec;
  border-bottom-color: #4c6ef5;
  font-weight: 600;
}
```

Убрать стиль `.settings-panel` как плавающей карточки — теперь он занимает полную ширину таба.
В `.settings-panel` убрать `border`, `border-radius`, `padding`, `margin-bottom` — оставить только `background` и layout.
Вместо этого использовать `padding: 0` и дать дышать через внутренние отступы секций.

### Критерий готовности

- `npm run build` — без ошибок.
- `npm run test:run` — все тесты проходят (обновить моки если нужно).
- Таб Queue виден сразу при запуске.
- Таб Settings показывает все секции настроек.
- ReadinessPanel видна на обоих табах.

---

## TASK-02 — Авто-загрузка whisper-cli на macOS

**Приоритет:** высокий
**Агент:** rust-tauri-reviewer или general
**Файлы:** `src-tauri/src/tool_download.rs`

### Что сейчас

Функция `locate_whisper_cli_macos` (строки 229–270) только ищет по двум путям Homebrew.
Ничего не скачивает. Возвращает ошибку с советом `brew install whisper-cpp`.

### Что нужно

Скачивать официальный release-артефакт из `ggml-org/whisper.cpp` (тот же источник, что Windows).

### Предварительно проверить (ПЕРЕД РЕАЛИЗАЦИЕЙ)

Открыть в браузере:
```
https://github.com/ggml-org/whisper.cpp/releases/latest
```
Найти в Assets файлы для macOS. Типичные имена:
- `whisper-bin-arm64.zip` (Apple Silicon M1/M2/M3)
- `whisper-bin-x64.zip` или `whisper-bin-x86_64.zip` (Intel)

Если таких файлов НЕТ → использовать Approach B (сборка через Homebrew как сейчас, но с улучшенным поиском).

### Approach A — Скачка из GitHub Releases (если artifacts есть)

В `tool_download.rs`:

```rust
#[cfg(target_os = "macos")]
fn whisper_cpp_macos_zip_url() -> Result<&'static str, String> {
    match std::env::consts::ARCH {
        "aarch64" => Ok(
            "https://github.com/ggml-org/whisper.cpp/releases/latest/download/whisper-bin-arm64.zip",
        ),
        "x86_64" => Ok(
            "https://github.com/ggml-org/whisper.cpp/releases/latest/download/whisper-bin-x64.zip",
        ),
        other => Err(format!("Unsupported macOS CPU: {other}")),
    }
}

#[cfg(target_os = "macos")]
pub async fn download_whisper_cli_managed(app: &AppHandle) -> Result<DownloadedWhisperCli, String> {
    // 1. Сначала проверяем Homebrew (быстро, без скачки)
    let homebrew_result = locate_whisper_cli_homebrew(app);
    if homebrew_result.is_ok() {
        return homebrew_result;
    }

    // 2. Скачиваем zip
    let url = whisper_cpp_macos_zip_url()?;
    let base = managed_bin_dir(app)?;
    std::fs::create_dir_all(&base).map_err(|e| format!("create bin dir: {e}"))?;
    let dest_dir = base.join("whisper-cpp");
    // ... аналогично download_whisper_cli_windows
    // extract zip → найти whisper-cli (без .exe)
    // chmod 755 через make_executable
}
```

Структура zip для macOS аналогична Windows: `Release/whisper-cli` (без `.exe`).
После извлечения вызвать `make_executable(&exe)`.

**Важно для macOS:** скачанный бинарник может быть заблокирован Gatekeeper.
После успешного извлечения показать предупреждение в `phase: "done"` message:
```
"whisper-cli ready. If macOS blocks it, run: xattr -d com.apple.quarantine /path/to/whisper-cli"
```

### Approach B — Улучшенный поиск Homebrew (если artifacts нет)

Заменить `locate_whisper_cli_macos` на `locate_whisper_cli_homebrew`:

```rust
#[cfg(target_os = "macos")]
fn locate_whisper_cli_homebrew(app: &AppHandle) -> Result<DownloadedWhisperCli, String> {
    // Пробуем `which whisper-cli` и `which whisper`
    for cmd_name in ["whisper-cli", "whisper"] {
        if let Ok(out) = std::process::Command::new("which")
            .arg(cmd_name)
            .output()
        {
            if out.status.success() {
                let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if !path.is_empty() && std::path::Path::new(&path).is_file() {
                    emit(app, ToolDownloadProgress {
                        tool: "whisper-cli".to_string(),
                        phase: "done".to_string(),
                        bytes_received: 0,
                        total_bytes: None,
                        message: format!("Found {path}"),
                    });
                    return Ok(DownloadedWhisperCli { whisper_cli_path: path });
                }
            }
        }
    }

    // Hardcoded Homebrew paths (Apple Silicon + Intel + Nix-brew)
    let candidates = [
        "/opt/homebrew/bin/whisper-cli",
        "/opt/homebrew/bin/whisper",
        "/usr/local/bin/whisper-cli",
        "/usr/local/bin/whisper",
        "/opt/homebrew/opt/whisper-cpp/bin/whisper-cli",
        "/home/linuxbrew/.linuxbrew/bin/whisper-cli",
    ];
    for c in candidates {
        if std::path::Path::new(c).is_file() {
            emit(app, ToolDownloadProgress { ... });
            return Ok(DownloadedWhisperCli { whisper_cli_path: c.to_string() });
        }
    }

    Err("whisper-cli not found. Install: brew install whisper-cpp, then press Setup again.".to_string())
}
```

### Критерий готовности

- `cargo build` — без ошибок.
- `cargo test` — без регрессий.
- На macOS кнопка "Setup whisper-cli" либо находит Homebrew, либо скачивает zip.

---

## TASK-03 — Инструкции для whisper-cli на Linux

**Приоритет:** высокий
**Агент:** kieran-typescript-reviewer или general
**Файлы:** `src/components/SettingsPanel.tsx`, `src/components/OnboardingWizard.tsx`

### Что сейчас

`isProbablyLinux()` функции нет. `showManagedToolDownloads = isWin || isMac` скрывает кнопки на Linux.
На Linux пользователь вообще не видит никаких инструкций по whisper-cli.

### Изменения в `SettingsPanel.tsx`

1. Добавить функцию рядом с `isProbablyWindows` / `isProbablyMac`:
```typescript
function isProbablyLinux(): boolean {
  if (typeof navigator === "undefined") return false;
  return /Linux/i.test(navigator.userAgent) && !/Android/i.test(navigator.userAgent);
}
```

2. В секции "Local Whisper" рядом с кнопкой "Setup whisper-cli":
   Если `isProbablyLinux()` — вместо кнопки показать блок:

```tsx
{isProbablyLinux() ? (
  <div className="linux-install-hint">
    <p className="hint">Install whisper-cli via package manager:</p>
    <pre className="code-block">
      {`# Ubuntu / Debian\nsudo apt install whisper-cpp\n\n# Fedora\nsudo dnf install whisper-cpp\n\n# Arch\nyay -S whisper-cpp\n\n# Build from source\ngit clone https://github.com/ggml-org/whisper.cpp && cd whisper.cpp && cmake -B build && cmake --build build`}
    </pre>
    <p className="hint">Then set the path to <code>whisper-cli</code> in the field above.</p>
  </div>
) : (
  /* существующая кнопка Setup whisper-cli */
)}
```

3. В `App.css` добавить стиль для блока:
```css
.linux-install-hint {
  margin-top: 0.5rem;
}

.code-block {
  font-size: 0.75rem;
  line-height: 1.5;
  padding: 0.55rem 0.7rem;
  border-radius: 8px;
  background: #0d0f14;
  border: 1px solid #2a2e3a;
  color: #c5c8d4;
  white-space: pre-wrap;
  overflow-x: auto;
  margin: 0.35rem 0;
}
```

### Изменения в `OnboardingWizard.tsx`

Шаг установки whisper-cli (Step 5, локальный режим):
Добавить аналогичный блок инструкций для Linux — условно по `isProbablyLinux()`.

### Критерий готовности

- `npm run build` без ошибок.
- `npm run test:run` без регрессий.
- На Linux вместо кнопки Setup отображается блок с командами.

---

## TASK-04 — Опция "Сохранить скачанное видео"

**Приоритет:** средний
**Агент:** rust-tauri-reviewer + kieran-typescript-reviewer
**Файлы:** `src/types/settings.ts`, `src-tauri/src/settings.rs`, `src-tauri/src/pipeline.rs`, `src/components/SettingsPanel.tsx`

### Что нужно

Добавить настройку `keepDownloadedVideo: boolean` (default: false).
Когда включена и источник — URL: после транскрипции скачать видеофайл в outputDir.

### Шаг 1 — TypeScript types (`src/types/settings.ts`)

Добавить поле в `AppSettings`:
```typescript
keepDownloadedVideo: boolean;
```
Добавить в `defaultAppSettings`:
```typescript
keepDownloadedVideo: false,
```

### Шаг 2 — Rust settings (`src-tauri/src/settings.rs`)

В `AppSettings` struct добавить:
```rust
#[serde(default)]
pub keep_downloaded_video: bool,
```

### Шаг 3 — Pipeline (`src-tauri/src/pipeline.rs`)

В `PrepareAudioContext` (или параметры `prepare_media_audio`) добавить `keep_video: bool`.

В ветке обработки URL после нормализации аудио:
```rust
if keep_video && is_http_url(source) {
    emit_pipeline_log(app, job_id, "video-download", "Downloading video for storage...");
    let video_dest = output_dir.join(
        format!("video_{}.mp4", sanitize_for_filename(title_or_id))
    );
    let yt_args = vec![
        "-f".to_string(), "bv*+ba/b".to_string(),
        "--merge-output-format".to_string(), "mp4".to_string(),
        "-o".to_string(), video_dest.to_string_lossy().into_owned(),
        "--no-playlist".to_string(),
        source.to_string(),
    ];
    run_cmd(yt_dlp, &yt_args, Duration::from_secs(YT_DLP_TIMEOUT), &cancel).await?;
    emit_pipeline_log(app, job_id, "video-saved", &format!("Video saved: {}", video_dest.display()));
}
```

Прокинуть `keep_downloaded_video` из `job.rs` в `prepare_media_audio`.

### Шаг 4 — UI (`src/components/SettingsPanel.tsx`)

В секцию "Output" добавить чекбокс:
```tsx
<label className="field checkbox">
  <input
    type="checkbox"
    checked={settings.keepDownloadedVideo}
    onChange={(e) => onChange({ ...settings, keepDownloadedVideo: e.target.checked })}
  />
  <span>Save downloaded video to output folder (URLs only)</span>
</label>
```

### Критерий готовности

- `cargo build` и `npm run build` без ошибок.
- `cargo test` без регрессий.
- Настройка сохраняется между запусками.
- При включённой опции и обработке YouTube URL — в папке вывода появляется `.mp4` файл.

---

## TASK-05 — Прогресс локального Whisper из stderr

**Приоритет:** средний
**Агент:** rust-tauri-reviewer
**Файлы:** `src-tauri/src/whisper_local.rs`

### Что сейчас

`transcribe_one_wav` запускает whisper-cli и ждёт завершения без промежуточных сообщений.
Пользователь видит «running» без изменений до нескольких минут/часов.

### Что нужно

whisper.cpp выводит в stderr строки вида:
```
whisper_print_timings:   total time =  1234.56 ms
[00:00:00.000 --> 00:00:04.800]  Some transcribed text
whisper_progress_callback: 10% done
```

Читать stderr построчно, парсить прогресс, эмитить события `queue-job-progress`.

### Изменения в `whisper_local.rs`

Заменить `run_cmd` (который просто ждёт процесса) на ручной `tokio::process::Command` с построчным чтением stderr:

```rust
use tokio::io::{AsyncBufReadExt, BufReader};

// Запустить процесс с piped stderr
let mut child = tokio::process::Command::new(cli)
    .args(&args)
    .stderr(std::process::Stdio::piped())
    .stdout(std::process::Stdio::piped())
    .kill_on_drop(true)
    .spawn()
    .map_err(|e| format!("spawn whisper-cli: {e}"))?;

let stderr = child.stderr.take().expect("stderr piped");
let mut lines = BufReader::new(stderr).lines();

// Читать stderr в фоне, парсить прогресс
let app_clone = app.clone();
let job_id_clone = job_id.to_string();
tokio::spawn(async move {
    while let Ok(Some(line)) = lines.next_line().await {
        // Парсить "XX% done" или просто эмитить строку как лог
        let msg = if let Some(pct) = parse_whisper_progress(&line) {
            format!("Transcribing… {pct}%")
        } else {
            line.clone()
        };
        let _ = app_clone.emit("queue-job-progress", QueueJobProgressPayload {
            job_id: job_id_clone.clone(),
            phase: "transcribe".to_string(),
            message: msg,
        });
    }
});

// Ждать завершения
let status = child.wait().await.map_err(|e| format!("wait: {e}"))?;
```

Функция парсинга:
```rust
fn parse_whisper_progress(line: &str) -> Option<u32> {
    // "whisper_progress_callback: 45% done" или "progress = 45 %"
    let line = line.to_lowercase();
    if let Some(pos) = line.find('%') {
        let before = line[..pos].trim();
        let num_start = before.rfind(|c: char| !c.is_ascii_digit()).map(|i| i + 1).unwrap_or(0);
        before[num_start..].parse::<u32>().ok()
    } else {
        None
    }
}
```

### Критерий готовности

- `cargo build` без ошибок.
- При запуске локальной транскрипции в UI появляются сообщения о прогрессе.

---

## TASK-06 — Retry для HTTP API (429 / 5xx)

**Приоритет:** средний
**Агент:** rust-tauri-reviewer
**Файлы:** `src-tauri/src/transcribe.rs`

### Что нужно

В `transcribe_wav_file` добавить 3 попытки с backoff для сетевых ошибок и 429/5xx.

### Изменения в `transcribe.rs`

```rust
const MAX_RETRIES: u32 = 3;

async fn transcribe_wav_file_inner(...) -> Result<String, String> {
    // существующий код одной попытки
}

pub async fn transcribe_wav_file(
    wav_path: &Path, base_url: &str, model: &str,
    api_key: &str, language: Option<&str>, cancel: &CancellationToken,
) -> Result<String, String> {
    let mut last_err = String::new();
    for attempt in 1..=MAX_RETRIES {
        tokio::select! {
            _ = cancel.cancelled() => return Err(JOB_CANCELLED_MSG.to_string()),
            result = transcribe_wav_file_inner(wav_path, base_url, model, api_key, language) => {
                match result {
                    Ok(text) => return Ok(text),
                    Err(e) => {
                        last_err = e.clone();
                        // Не ретраить ошибки клиента (400, 401, 403)
                        if e.contains("HTTP 4") && !e.contains("HTTP 429") {
                            return Err(e);
                        }
                        if attempt < MAX_RETRIES {
                            let wait_secs = 2u64.pow(attempt - 1); // 1s, 2s, 4s
                            tokio::time::sleep(Duration::from_secs(wait_secs)).await;
                        }
                    }
                }
            }
        }
    }
    Err(format!("Failed after {MAX_RETRIES} attempts: {last_err}"))
}
```

### Критерий готовности

- `cargo build` без ошибок.
- `cargo test` без регрессий.
- При временной ошибке 429 задача делает паузу и повторяет попытку.

---

## TASK-07 — Улучшение обнаружения whisper-cli в `deps.rs`

**Приоритет:** средний
**Агент:** rust-tauri-reviewer
**Файлы:** `src-tauri/src/deps.rs`

### Что сейчас

`resolve_whisper_cli_path` проверяет только рядом с exe и в `bin/` подпапке.
Не ищет в managed bin dir и не проверяет PATH через `which`.

### Изменения

В `resolve_whisper_cli_path` добавить:
1. Поиск в managed bin dir (`app_data_dir/v2t/bin/whisper-cpp/whisper-cli`).
2. На Unix — попытку `which whisper-cli` и `which whisper`.

Сигнатуру нужно расширить: `resolve_whisper_cli_path(override_path: Option<&str>, app: &AppHandle)`.
Обновить вызов в `check_dependencies`.

```rust
pub fn resolve_whisper_cli_path(override_path: Option<&str>, app: &AppHandle) -> Option<PathBuf> {
    // 1. Override
    if let Some(p) = override_path { ... }

    // 2. Рядом с exe / bin/
    for stem in ["whisper-cli", "main"] { ... }

    // 3. Managed bin dir
    if let Ok(managed) = crate::tool_download::managed_bin_dir(app) {
        for stem in ["whisper-cli", "whisper"] {
            let exe = managed.join("whisper-cpp").join(exe_file_name(stem));
            if exe.is_file() { return Some(exe); }
        }
    }

    // 4. Unix: which
    #[cfg(unix)]
    for name in ["whisper-cli", "whisper"] {
        if let Ok(out) = std::process::Command::new("which").arg(name).output() {
            if out.status.success() {
                let p = PathBuf::from(String::from_utf8_lossy(&out.stdout).trim());
                if p.is_file() { return Some(p); }
            }
        }
    }

    None
}
```

### Критерий готовности

- `cargo build` без ошибок.
- `cargo test` без регрессий.
- После скачки whisper-cli через TASK-02 — `check_dependencies` находит его автоматически без перезапуска.

---

---

## TASK-08 — Дизайн: исправить найденные несоответствия

**Приоритет:** средний
**Агент:** ui-designer
**Файлы:** `src/App.css`, `src/components/SettingsPanel.tsx`, `src/App.tsx`

### Результаты дизайн-ревью (2026-03-27)

Найдено 23 проблемы. Ниже только те, что нужно исправить (high + важные medium).

### HIGH — исправить в первую очередь

**1. `<select>` не имеет стилей** — все `<select>` в SettingsPanel и OnboardingWizard используют браузерные дефолты.
Добавить в `App.css` после блока `input[type="text"]`:

```css
select {
  width: 100%;
  border-radius: 8px;
  border: 1px solid #2a2e3a;
  padding: 0.45rem 0.6rem;
  font-size: 0.9rem;
  font-family: inherit;
  background: #0d0f14;
  color: #e8e8ec;
  cursor: pointer;
}

select:focus {
  outline: none;
  border-color: #4c6ef5;
}
```

**2. Отсутствуют aria-атрибуты:**
- `App.tsx:137` — кнопка Setup guide: добавить `aria-label="Open setup guide"`
- `App.tsx:160` — toast div: добавить `aria-live="polite"` к существующему `role="status"`
- `SettingsPanel.tsx` — все `<select>`: добавить `aria-label="Transcription mode"`, `aria-label="Whisper model"` и т.п.
- Блок настроек транскрипции: добавить `role="group"` и `aria-label="Transcription settings"`

### MEDIUM — исправить в следующем спринте

**3. Inline styles → CSS классы.**
В `SettingsPanel.tsx` есть 7+ мест с `style={{ marginTop: "...", marginBottom: "..." }}`.
Добавить утилитарные классы в `App.css`:
```css
.mt-xs { margin-top: 0.35rem; }
.mt-sm { margin-top: 0.5rem; }
.mb-xs { margin-bottom: 0.35rem; }
.mb-sm { margin-bottom: 0.5rem; }
```
Заменить inline styles на эти классы в SettingsPanel.tsx.

**4. Несогласованный border-radius:**
- `App.css:116` — `.deps`: `6px` → `8px`
- `App.css:620` — `.download-progress-wrap progress`: `4px` → `6px`
- `App.css:834` — `button.queue-table-action-btn`: `6px` → `8px`

**5. Font size кнопки readiness-settings-btn:**
- `App.css:233` — `.readiness-settings-btn`: `0.82rem` → `0.9rem` (как стандартная кнопка)

### Критерий готовности

- `npm run build` без ошибок.
- `npm run test:run` без регрессий.
- Все `<select>` визуально согласованы с инпутами.
- Нет inline `style=` с отступами в SettingsPanel.tsx.

---

---

## TASK-09 — Исправить сбой BrowserWhisper (WASM) в WebView2

**Приоритет:** высокий
**Агент:** kieran-typescript-reviewer
**Файлы:** `src/components/QueuePanel.tsx`, `src/components/SettingsPanel.tsx`, `src-tauri/tauri.conf.json`

### Что сейчас

Режим "In-app — Whisper (WASM)" (`browserWhisper`) падает сразу после подготовки аудио.
Лог:
```
[browser] Prepared for in-app (WASM) transcription…
[ui] Error: <job source URL>
```

Реальная JS-ошибка из `transcribeBrowserTracks` не попадает в UI — виден лишь источник задания.
Пользователь не знает, что именно пошло не так.

### Корневые причины (предположительно)

1. **SharedArrayBuffer недоступен** — ONNX Runtime WASM требует `SharedArrayBuffer`.
   В WebView2 он заблокирован без HTTP-заголовков `COOP` / `COEP` (Cross-Origin Isolation).
2. **CSP блокирует WASM** — `Content-Security-Policy` в `tauri.conf.json` может запрещать `'wasm-unsafe-eval'`.
3. **Плохое логирование** — ошибка из `catch (e)` реброшена без сохранения стека.

### Шаг 1 — Улучшить логирование ошибки в `QueuePanel.tsx`

Найти блок (строки ~324–327):
```tsx
} catch (e) {
  void releaseQueueJobSlot(job.id);
  throw e;
}
```

Заменить на:
```tsx
} catch (e) {
  void releaseQueueJobSlot(job.id);
  const msg =
    e instanceof Error
      ? `${e.message}${e.cause ? ` (cause: ${e.cause})` : ""}`
      : String(e);
  appendLog(`[browser-error] ${msg}`);
  void sessionLogAppendUi(job.id, "browser-error", msg);
  throw e;
}
```

Это гарантирует, что фактическое сообщение об ошибке попадает в лог **до** того, как исключение поглотит внешний catch.

### Шаг 2 — Добавить COOP/COEP заголовки в `tauri.conf.json`

Открыть `src-tauri/tauri.conf.json`.
Найти секцию `"app"` → `"windows"` (или `"security"` / `"csp"`).

Добавить в объект `"app"`:
```json
"security": {
  "headers": {
    "Cross-Origin-Opener-Policy": "same-origin",
    "Cross-Origin-Embedder-Policy": "require-corp"
  }
}
```

Если секция `"security"` уже есть — добавить только `"headers"` внутрь неё, не удаляя существующие поля.

Если в `"security"` есть `"csp"` — добавить к его значению директиву `'wasm-unsafe-eval'` в `script-src`:
```
script-src 'self' 'wasm-unsafe-eval' https://cdn.jsdelivr.net;
```

> **Замечание:** Таури 2 поддерживает `security.headers` начиная с версии 2.1.
> Проверить наличие поля через `cargo tree | grep tauri` — если `tauri` < 2.1, заголовки надо задать через `custom_protocol` в Rust (`src-tauri/src/lib.rs`) с помощью `webview_builder.with_additional_browser_args`.
> В этом случае вместо изменения JSON добавить в `lib.rs`:
> ```rust
> .with_additional_browser_args("--enable-features=SharedArrayBuffer")
> ```
> (только для WebView2/Chromium)

### Шаг 3 — Показать предупреждение в `SettingsPanel.tsx`

В блоке `{useBrowser && (...)}` (после описания режима) добавить:

```tsx
{useBrowser && (
  <p className="hint hint--warn" role="alert">
    ⚠ Experimental: requires internet (model download ~150 MB) and may not
    work on all systems. Switch to Cloud API or Local Offline if transcription
    fails.
  </p>
)}
```

В `App.css` добавить модификатор:
```css
.hint--warn {
  color: #f59e0b;
}
```

### Критерий готовности

- `npm run build` и `cargo build` без ошибок.
- После включения режима BrowserWhisper и запуска задачи:
  - в логе видна конкретная JS-ошибка (не просто URL задания)
- Если WebView2 поддерживает SharedArrayBuffer с добавленными заголовками — транскрипция запускается без ошибки.
- В SettingsPanel виден оранжевый предупредительный текст при выборе `browserWhisper`.

---

---

## TASK-10 — Статистика завершённого задания в логе

**Приоритет:** средний
**Агент:** rust-tauri-reviewer + kieran-typescript-reviewer
**Файлы:** `src-tauri/src/job.rs`, `src/components/QueuePanel.tsx`

### Что нужно

После завершения каждого задания выводить в лог одну строку со статистикой:

```
✓ audio 5m 12s · 1 047 words · 23s elapsed · Local Whisper (base)
```

Для каждого режима (`httpApi`, `localWhisper`, `browserWhisper`) строка одинаковая по структуре.

### Метрики

| Метрика | Источник |
|---|---|
| Длительность аудио | WAV bytes / 32 000 (16kHz 16-bit mono = 32 000 B/s) |
| Кол-во слов | `text.split_whitespace().count()` в Rust, или `text.trim().split(/\s+/).length` в JS |
| Прошедшее время (elapsed) | `Instant::now()` в начале `run_process_queue_item`; `Date.now()` в JS для BrowserWhisper |
| Режим + модель | `settings.transcription_mode` + `settings.whisper_model` |

### Шаг 1 — Rust: добавить `JobStats` и enriched summary (`job.rs`)

В начале `run_process_queue_item`:
```rust
let job_started = std::time::Instant::now();
```

Добавить приватный хелпер (в конце файла, перед `#[cfg(test)]`):
```rust
fn audio_duration_secs(wav_paths: &[String]) -> f64 {
    const BYTES_PER_SEC: u64 = 32_000; // 16kHz * 2 bytes * 1 ch
    wav_paths
        .iter()
        .filter_map(|p| {
            fs::metadata(p).ok().map(|m| {
                m.len().saturating_sub(44) // skip WAV header
            })
        })
        .sum::<u64>() as f64
        / BYTES_PER_SEC as f64
}

fn format_duration(secs: f64) -> String {
    let s = secs.round() as u64;
    if s < 60 {
        format!("{s}s")
    } else {
        format!("{}m {:02}s", s / 60, s % 60)
    }
}

fn format_stats(
    wav_paths: &[String],
    total_words: usize,
    elapsed_secs: u64,
    mode: &TranscriptionMode,
    model: &str,
) -> String {
    let audio = format_duration(audio_duration_secs(wav_paths));
    let mode_str = match mode {
        TranscriptionMode::HttpApi => "Cloud API".to_string(),
        TranscriptionMode::LocalWhisper => format!("Local Whisper ({model})"),
        TranscriptionMode::BrowserWhisper => format!("In-app WASM ({model})"),
    };
    format!(
        "audio {audio} · {total_words} words · {elapsed_secs}s elapsed · {mode_str}"
    )
}
```

В конце `run_process_queue_item`, перед финальным `emit_progress` и `Ok(...)`:

Для `httpApi` / `localWhisper` ветки — собрать `total_words` суммированием по всем трекам,
затем:
```rust
let elapsed = job_started.elapsed().as_secs();
let stats = format_stats(
    &prep.wav_paths,
    total_words,
    elapsed,
    &settings.transcription_mode,
    &settings.whisper_model,
);
emit_progress(&app, &job_id, "stats", &stats);
```

Это добавляет строку с фазой `"stats"` в лог **перед** финальным `"done"`.

Для `BrowserWhisper` ветки (`BrowserPrepared` outcome) — elapsed и word count считаются на стороне JS (шаг 2).
В `BrowserPrepared` добавить поле `started_at_ms: u64`:
```rust
BrowserPrepared {
    tracks,
    work_dir: ...,
    delete_audio_after: ...,
    language: ...,
    whisper_model_id: ...,
    started_at_ms: std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0),
}
```

Обновить тип `ProcessQueueItemOutcome` в Rust (`job.rs`) и в TypeScript (`src/types/job.ts`).

### Шаг 2 — TypeScript: логирование для BrowserWhisper (`QueuePanel.tsx`)

После успешного завершения в ветке `outcome.kind === "browserPrepared"`, перед `setJobs(...)`:

```ts
const elapsedSec = Math.round((Date.now() - outcome.startedAtMs) / 1000);
const totalWords = texts.reduce((acc, t) => {
  const w = t.trim() ? t.trim().split(/\s+/).length : 0;
  return acc + w;
}, 0);
const audioDurSec = outcome.tracks.length; // placeholder — см. ниже
const statsLine = `audio ? · ${totalWords} words · ${elapsedSec}s elapsed · In-app WASM (${outcome.whisperModelId})`;
appendLog(`[stats] ${statsLine}`);
void sessionLogAppendUi(job.id, "stats", statsLine);
```

> Длительность аудио в JS не считается отдельно (нет доступа к WAV до очистки).
> Строку аудио-длительности оставить как `?` или вовсе пропустить поле, если `outcome.tracks` пусты.
> Если треков несколько — использовать `outcome.tracks.length` как `n tracks`.

Итоговая строка для BrowserWhisper:
```
[stats] 2 tracks · 3 201 words · 87s elapsed · In-app WASM (base)
```

### Шаг 3 — Обновить тип в TypeScript (`src/types/job.ts`)

```ts
export type ProcessQueueItemOutcome =
  | { kind: "done"; transcriptPath: string; summary: string }
  | {
      kind: "browserPrepared";
      tracks: BrowserTrackInfo[];
      workDir: string;
      deleteAudioAfter: boolean;
      language: string | null;
      whisperModelId: string;
      startedAtMs: number;   // <-- новое поле
    };
```

### Критерий готовности

- `cargo build` и `npm run build` без ошибок.
- `cargo test` и `npm run test:run` без регрессий.
- После завершения задания в сессионном логе есть строка с фазой `stats`:
  - `audio 3m 12s · 892 words · 18s elapsed · Local Whisper (base)`
  - или `2 tracks · 1 204 words · 44s elapsed · In-app WASM (tiny)` для BrowserWhisper
- Строка появляется и для успешных заданий, и отсутствует при отмене/ошибке.

---

---

## TASK-11 — Pipeline overlap внутри одной задачи (Epic 3)

**Приоритет:** высокий
**Агент:** v2t-pipeline-specialist
**Источник:** `.cursor/tasks/BA-20260420-180000-alice.md` (Epic 3)
**Файлы:** `src-tauri/src/pipeline.rs`, `src-tauri/src/whisper_local.rs`, `src-tauri/src/transcribe.rs`, `src-tauri/src/job.rs`

### Контекст

Главный выигрыш для пользователя в дефолтном сценарии (`localWhisper`, `maxConcurrentJobs=1`).
Без новых UI-настроек, без изменения семантики, без новых внешних зависимостей.

Целевые узкие места:

1. В `pipeline.rs::prepare_media_audio` ветка URL: `download_audio` (yt-dlp) → `download_best_video_mp4` (yt-dlp ещё раз) → `ffmpeg` нормализация **идут последовательно**. Шаг 2 и 3 независимы (видео-файл сохраняется отдельно от аудио-нормализации, см. TASK-04).
2. В `transcribe.rs::transcribe_wav_maybe_split` (HTTP API) и `whisper_local.rs::transcribe_wav_maybe_split_whisper` (Local Whisper) обработка чанков **строго последовательная**: ffmpeg режет чанк → транскрипция этого чанка → ffmpeg режет следующий. Шаги (cut chunk N+1) и (transcribe chunk N) независимы по файлам.

### Что нужно сделать

**Часть A — overlap «yt-dlp видео ‖ ffmpeg-нормализация» в `pipeline.rs`**

В ветке URL после успешного скачивания аудио:
- Если `keep_downloaded_video == true`, запускать `download_best_video_mp4` через `tokio::spawn` параллельно с `ffmpeg_normalize_input`.
- Дождаться обоих через `tokio::try_join!`. Ошибка ffmpeg-нормализации обязательно прерывает задачу (это путь к транскрипции). Ошибка скачивания видео — **только warning в session log**, не прерывает задачу (видео — опциональный артефакт).
- Cancellation: оба future должны слушать тот же `CancellationToken`, при отмене обе спавн-таски корректно завершаются (kill_process_tree обоих yt-dlp + ffmpeg).

**Часть B — overlap «ffmpeg-чанк N+1 ‖ транскрипция чанка N» в `transcribe.rs`**

Текущий цикл `while start < duration_sec - 0.05` в `transcribe_wav_maybe_split`:
```
loop:
  cut_chunk(i)         // ffmpeg
  transcribe_chunk(i)  // HTTP
  i += 1
```

Новая структура (channel-based prefetch с глубиной 1):
- Завести `tokio::sync::mpsc::channel::<(usize, PathBuf)>(1)` — слот на один pre-cut чанк.
- Producer task: цикл по чанкам, для каждого вызывает `pipeline::run_cmd` (ffmpeg) и шлёт `(idx, chunk_path)` в канал. Канал закрывается, когда чанков больше нет.
- Consumer (основная функция): принимает `(idx, chunk_path)`, вызывает `transcribe_wav_file` (или whisper-cli), обрабатывает результат, удаляет временный файл (если `delete_chunks_after`).
- При первой ошибке — отменить producer (`drop` receiver или explicit cancel), kill_process_tree оставшихся ffmpeg-процессов.
- Resume-логика (`v2t-api-{fp}-chunk-{i}.txt`): если для индекса `i` уже есть валидный чекпоинт, **producer пропускает ffmpeg-нарезку** (генерирует только событие skip и пустой path-marker). Consumer видит маркер и читает чекпоинт без транскрипции.

**Часть C — то же для `whisper_local.rs`**

Для `localWhisper` overlap имеет смысл **только при `maxConcurrentJobs == 1`**: ffmpeg на пустом ядре пока CPU занят whisper.cpp. При `maxConcurrentJobs > 1` всё CPU и так загружено — overlap не помогает, но и не мешает (просто ffmpeg ждёт планировщика).

Реализация — **симметричная** Части B, отдельной функцией `chunk_prefetch_loop` в общем модуле (создать `src-tauri/src/chunk_prefetch.rs` или положить в `pipeline.rs`), параметризованной transcribe-функцией.

### Что **не** делать

- Не менять API публичных Tauri-команд.
- Не добавлять новые поля в `AppSettings`.
- Не делать prefetch с глубиной > 1 (защита от RAM blowup на 4-часовых файлах).
- Не параллелить нарезку **внутри** одной задачи — параллельно работает максимум **один** ffmpeg чанк на задачу.

### Тесты (обязательно)

- Unit-тест в `transcribe.rs`: мокнутая transcribe-функция возвращает фиксированный текст с `tokio::time::sleep`; проверяем, что общее время выполнения ≈ max(N × cut_time, N × transcribe_time), а не sum.
- Тест на cancellation: после `cancel.cancel()` обе таски (producer + consumer) завершаются за < 2 секунды, никаких leak-процессов в `process_kill::test_helpers` (если есть).
- Тест на resume: при наличии чекпоинтов для chunks [0, 2] и отсутствии для [1, 3] — ffmpeg вызывается ровно для [1, 3].

### Критерий готовности

- `cargo build` и `cargo test` чисто.
- На реальном файле длительностью 1 час (предоставит PO в папке тестов): время транскрипции через `localWhisper` сокращается **не менее чем на 8%** относительно текущего sequential pipeline.
- Лог сессии содержит явные фазы `cut-chunk-{i}` и `transcribe-chunk-{i}` с временными метками, по которым видно перекрытие.
- Stop queue → процессы корректно убиваются за < 2 секунд.

---

## TASK-12 — Параллельная очередь jobs + инфраструктура семафоров (Epic 1)

**Приоритет:** средний
**Агенты:** kieran-typescript-reviewer + julik-frontend-races-reviewer (**обязательное ревью гонок**) + code-simplicity-reviewer (финальный проход)
**Источник:** `.cursor/tasks/BA-20260420-180000-alice.md` (Epic 1)
**Файлы:**
- Rust: `src-tauri/src/lib.rs`, `src-tauri/src/settings.rs`, `src-tauri/src/pipeline.rs`, `src-tauri/src/transcribe.rs`, `src-tauri/src/whisper_local.rs`, `src-tauri/src/job.rs`
- TS: `src/types/settings.ts`, `src/components/QueuePanel.tsx`, `src/components/SettingsPanel.tsx`, `src/App.tsx`, `src/App.css`

### Контекст

Сейчас `QueuePanel.tsx::startQueue` обрабатывает задачи строго последовательно через `for` + `await processQueueItem`. Глобальные единичные ref-ы (`runningRef`, `currentJobIdRef`, `stopRequestedRef`) предполагают одного worker-а — масштабирование напрямую вызовет гонки.

PO выбрал «yt-dlp в один поток» (см. BA-артефакт OQ-9), поэтому параллельность реализуется как **pipeline с тремя lane-ами с разными лимитами**: download=1, normalize=1, transcribe=N.

### Шаг 1 — Rust: новый модуль `concurrency.rs`

Создать `src-tauri/src/concurrency.rs`:

```rust
use std::sync::Arc;
use tokio::sync::Semaphore;

pub struct PipelineLanes {
    pub download: Arc<Semaphore>,    // capacity = 1
    pub normalize: Arc<Semaphore>,   // capacity = 1
    pub http_transcribe: Arc<Semaphore>, // capacity = 6
}

impl PipelineLanes {
    pub fn new() -> Self {
        Self {
            download: Arc::new(Semaphore::new(1)),
            normalize: Arc::new(Semaphore::new(1)),
            http_transcribe: Arc::new(Semaphore::new(6)),
        }
    }
}
```

Константы (`DOWNLOAD_LANE_CAP = 1`, `NORMALIZE_LANE_CAP = 1`, `GLOBAL_HTTP_CAP = 6`) — `pub const` в этом модуле.

В `lib.rs::run()`:
```rust
let lanes = PipelineLanes::new();
.manage(lanes)
```

### Шаг 2 — Rust: прокинуть семафоры в pipeline и transcribe

`prepare_media_audio` принимает `&PipelineLanes`:
- Перед вызовом `download_audio` / `download_best_video_mp4` — `lanes.download.acquire().await`.
- Перед `ffmpeg_normalize_input` — `lanes.normalize.acquire().await`.
- Permits держатся `let _permit = ...;` до конца соответствующей фазы; падают через RAII при ошибке/отмене.

`transcribe_wav_file_inner` (HTTP API) принимает `&PipelineLanes`:
- Перед каждым POST-запросом — `lanes.http_transcribe.acquire().await`. Permit держится ровно на время одного HTTP-запроса (включая ретраи в одной попытке).

`whisper_local::transcribe_one_wav` — **семафоры не нужны**. Concurrency для `localWhisper` ограничивается только `maxConcurrentJobs` в очереди (количеством параллельных jobs), потому что whisper.cpp сам внутри использует все доступные CPU-ядра.

### Шаг 3 — Rust: `AppSettings.max_concurrent_jobs`

В `src-tauri/src/settings.rs` добавить в `AppSettings`:
```rust
#[serde(default)]
pub max_concurrent_jobs: Option<u32>,
```

`None` означает «использовать default режима». Хелпер:
```rust
pub fn effective_max_concurrent_jobs(settings: &AppSettings) -> u32 {
    settings.max_concurrent_jobs.unwrap_or_else(|| {
        match settings.transcription_mode {
            TranscriptionMode::HttpApi => 2,
            TranscriptionMode::LocalWhisper => 1,
            TranscriptionMode::BrowserWhisper => 1,
        }
    }).clamp(1, 8)
}
```

### Шаг 4 — TypeScript types (`src/types/settings.ts`)

Добавить в `AppSettings`:
```ts
maxConcurrentJobs: number | null;
```
В `defaultAppSettings`:
```ts
maxConcurrentJobs: null,
```
Добавить хелпер:
```ts
export function effectiveMaxConcurrentJobs(settings: AppSettings): number {
  if (settings.maxConcurrentJobs && settings.maxConcurrentJobs >= 1) {
    return Math.min(8, settings.maxConcurrentJobs);
  }
  switch (settings.transcriptionMode) {
    case "httpApi": return 2;
    case "localWhisper": return 1;
    case "browserWhisper": return 1;
    default: return 1;
  }
}

export function defaultMaxConcurrentJobsFor(mode: TranscriptionMode): number {
  return mode === "httpApi" ? 2 : 1;
}
```

### Шаг 5 — UI (`src/components/SettingsPanel.tsx`)

В секцию "Performance" (создать, если нет) добавить:

```tsx
<label className="field">
  <span>Concurrent jobs ({defaultMaxConcurrentJobsFor(settings.transcriptionMode)} default for {settings.transcriptionMode})</span>
  <div className="field-row">
    <input
      type="number"
      min={1}
      max={8}
      value={effectiveMaxConcurrentJobs(settings)}
      aria-label="Maximum concurrent jobs"
      onChange={(e) => onChange({
        ...settings,
        maxConcurrentJobs: clamp(parseInt(e.target.value, 10) || 1, 1, 8),
      })}
    />
    <button
      type="button"
      className="reset-btn"
      onClick={() => onChange({ ...settings, maxConcurrentJobs: null })}
    >
      Reset to default
    </button>
  </div>
  {effectiveMaxConcurrentJobs(settings) > 1 &&
   (settings.transcriptionMode === "localWhisper" ||
    settings.transcriptionMode === "browserWhisper") && (
    <p className="hint hint--warn" role="alert">
      ⚠ Running multiple {settings.transcriptionMode === "localWhisper" ? "Local Whisper" : "In-app WASM"} jobs
      in parallel will load each model into RAM separately. Large models may exceed
      available memory and crash the app. Recommended: keep at 1 unless you know your machine has spare RAM/GPU.
    </p>
  )}
</label>
```

Стили `.field-row` и `.reset-btn` — добавить в `App.css` рядом с `.field` (flex row, gap 0.5rem; reset-btn — secondary outline).

### Шаг 6 — TypeScript: worker pool в `QueuePanel.tsx`

Удалить single-worker инварианты:
- `runningRef`, `currentJobIdRef` → заменить на `runningJobsRef: Set<string>` (id-set активных).
- `stopRequestedRef` остаётся (общий флаг отмены).

Новая `startQueue`:
```ts
const startQueue = useCallback(async () => {
  if (running) return;
  setRunning(true);
  stopRequestedRef.current = false;
  const limit = effectiveMaxConcurrentJobs(settings);
  const queue = jobs.filter(j => j.status === "pending").map(j => j.id);
  const inflight: Map<string, Promise<void>> = new Map();

  while (queue.length > 0 || inflight.size > 0) {
    if (stopRequestedRef.current) break;
    while (inflight.size < limit && queue.length > 0 && !stopRequestedRef.current) {
      const jobId = queue.shift()!;
      runningJobsRef.current.add(jobId);
      const p = processQueueItem(jobId)
        .catch((e) => appendLog(`[error] job ${jobId}: ${stringifyError(e)}`))
        .finally(() => {
          runningJobsRef.current.delete(jobId);
          inflight.delete(jobId);
        });
      inflight.set(jobId, p);
    }
    if (inflight.size > 0) {
      await Promise.race(inflight.values());
    }
  }
  setRunning(false);
}, [jobs, settings, processQueueItem, appendLog]);
```

`stopQueue` — hard stop:
- `stopRequestedRef.current = true`
- Для каждого id в `runningJobsRef.current` → `invoke("cancel_queue_job", { jobId: id })` (используется существующая команда отмены, она вызывает `cancel.cancel()` в Rust → `process_kill::kill_process_tree`).
- `appendLog(\`[stop] hard stop, ${runningJobsRef.current.size} active jobs cancelled\`)`.

### Шаг 7 — Cross-cutting

- В `processQueueItem` снять любые предположения «эта задача единственная активная» (если есть).
- `currentJobIdRef` → удалить или заменить на `runningJobsRef`.
- В `jobs` state: при параллельном обновлении нескольких rows одновременно использовать функциональный setter `setJobs(prev => prev.map(...))`. Любой setter, использующий замыкание над старым `jobs`, — потенциальная гонка → `julik-frontend-races-reviewer` обязательно проверяет.

### Тесты (обязательно)

**Rust:**
- `concurrency.rs`: семафор реально лимитирует параллельные holders.
- `pipeline.rs`: при двух одновременных задачах с URL вызовы `yt-dlp` сериализуются (мок через trait `CommandRunner` или счётчик активных вызовов).

**TypeScript (Vitest):**
- `effectiveMaxConcurrentJobs` для каждой комбинации (mode × user-value).
- `QueuePanel`: при `maxConcurrentJobs=3` и 5 pending jobs `inflight.size` никогда не превышает 3 (мок `processQueueItem`).
- При `stopQueue` все active jobs получают `cancel_queue_job`.

**E2E (ручной, на папке PO):**
- httpApi, jobs=2 — два HTTP-задания идут параллельно, лог это показывает.
- localWhisper, jobs=2 (вручную выставлено) — warning виден, обе задачи выполняются, ничего не падает.
- Stop queue в середине — все активные останавливаются < 2 сек.

### Что **не** делать

- Не добавлять автоматическую регулировку `maxConcurrentJobs` от железа (вне скоупа).
- Не выносить overlap внутри одной задачи (это TASK-11).
- Не реализовывать parallel chunks (это TASK-13).
- Не менять формат session log.

### Критерий готовности

- `cargo build`, `cargo test`, `npm run build`, `npm run test:run` — чисто.
- При `transcriptionMode=httpApi`, `maxConcurrentJobs=null` (=2 default) и трёх задачах в очереди — две идут параллельно, одна ждёт.
- При смене `transcriptionMode` с `httpApi` на `localWhisper` UI показывает «default 1 for localWhisper», warning не появляется (effective = 1).
- При вводе `maxConcurrentJobs=5` для localWhisper — warning виден, очередь работает.
- Stop queue убивает всех активных за < 2 секунд.
- `julik-frontend-races-reviewer` подтверждает отсутствие гонок в `QueuePanel.tsx`.

---

## TASK-13 — Параллельная загрузка чанков HTTP API (Epic 2)

**Приоритет:** низкий
**Агент:** v2t-pipeline-specialist
**Источник:** `.cursor/tasks/BA-20260420-180000-alice.md` (Epic 2)
**Зависимость:** TASK-12 (использует `PipelineLanes::http_transcribe`).
**Файлы:** `src-tauri/src/transcribe.rs`, `src-tauri/src/settings.rs`, `src/types/settings.ts`, `src/components/SettingsPanel.tsx`

### Контекст

Сейчас при работе с большим WAV-файлом через HTTP API чанки отправляются строго последовательно. PO предпочитает `localWhisper` (см. BA OQ-1), поэтому это quality-of-life фича для случая, когда пользователь временно переключается на cloud.

### Шаг 1 — Settings

Rust (`settings.rs`):
```rust
#[serde(default)]
pub parallel_chunks_per_file: Option<u32>,
```

TS (`src/types/settings.ts`):
```ts
parallelChunksPerFile: number | null;
```
Default — `null` (=3). Эффективное значение: `clamp(value || 3, 1, 6)`.

### Шаг 2 — UI (`SettingsPanel.tsx`)

В секции "Performance" (создана в TASK-12) добавить второе поле:
```tsx
<label className="field">
  <span>API: parallel chunks per file (default 3, max 6)</span>
  <input
    type="number"
    min={1}
    max={6}
    aria-label="Parallel chunks per file for HTTP API"
    value={settings.parallelChunksPerFile ?? 3}
    onChange={(e) => onChange({
      ...settings,
      parallelChunksPerFile: clamp(parseInt(e.target.value, 10) || 3, 1, 6),
    })}
  />
  <p className="hint">
    Effective concurrency is capped by the global HTTP limit (6) shared across all jobs.
  </p>
</label>
```

Поле видимо только при `transcriptionMode === "httpApi"` (через условный рендер).

### Шаг 3 — Rust: параллельные чанки

В `transcribe.rs::transcribe_wav_maybe_split` (HTTP API ветка):

После того как через ffmpeg нарезаны все чанки (или с overlap из TASK-11 — производятся постепенно), отправлять их через `futures::stream::iter(chunks).buffered(N)` где `N = effective_parallel_chunks`:

```rust
use futures::StreamExt;

let parallel = settings.parallel_chunks_per_file.unwrap_or(3).clamp(1, 6) as usize;
let results: Vec<Result<(usize, String), String>> = futures::stream::iter(chunk_paths.iter().enumerate())
    .map(|(idx, chunk_path)| {
        let lanes = lanes.clone();
        let cancel = cancel.clone();
        async move {
            // resume check via checkpoint file
            if let Some(text) = read_chunk_checkpoint(fp, idx) {
                return Ok((idx, text));
            }
            let permit = lanes.http_transcribe.acquire().await
                .map_err(|e| format!("semaphore: {e}"))?;
            let text = transcribe_wav_file_inner(chunk_path, ..., &cancel).await?;
            drop(permit);
            write_chunk_checkpoint(fp, idx, &text)?;
            Ok((idx, text))
        }
    })
    .buffered(parallel)
    .collect()
    .await;

// проверить ошибки, отсортировать по idx, склеить
```

Важно:
- Глобальный HTTP-лимит из `PipelineLanes::http_transcribe` (capacity 6) **уже** ограничивает суммарную concurrency между всеми jobs и chunks. `parallel_chunks_per_file` — это per-job upper bound, фактическая concurrency = `min(parallel_chunks_per_file, free permits in global semaphore)`.
- Cancellation: `cancel.cancelled()` в `tokio::select!` обёртке внутри producer-функции; `buffered` корректно отменяет все pending tasks при drop stream.
- Resume: каждый chunk пишет `v2t-api-{fp}-chunk-{i}.txt` сразу после успешной транскрипции (как сейчас); параллельная запись разных файлов — без race.
- Порядок результатов: после `collect()` отсортировать `Vec<(usize, String)>` по `idx`, затем `join` с разделителем (как сейчас).

Для `whisper_local.rs` **не делать** — у локального whisper.cpp нет смысла в параллельных чанках (CPU/GPU bound).

### Тесты (обязательно)

- Unit-тест: мокнутая `transcribe_wav_file_inner` с `sleep(100ms)`; для 6 чанков и `parallel=3` суммарное время ≈ 200ms (3 параллельно × 2 батча), а не 600ms.
- Тест на сохранение порядка: моки возвращают `"text-{idx}"`; результат — `"text-0 text-1 ... text-5"` независимо от порядка завершения.
- Тест на cancellation: после `cancel.cancel()` все inflight chunks завершаются за < 2 секунды.
- Тест на resume: 3 чекпоинта существуют, 3 — нет → `transcribe_wav_file_inner` вызывается ровно 3 раза.

### Критерий готовности

- `cargo build` и `npm run build` без ошибок.
- На реальном файле длительностью 1 час через `httpApi` с `parallelChunksPerFile=3`: время сокращается **не менее чем на 50%** относительно `parallelChunksPerFile=1`.
- При `maxConcurrentJobs=2` + `parallelChunksPerFile=3` — global HTTP semaphore показывает не более 6 одновременных запросов (видно в логе как `[http-permit] acquired (X/6)` если такой лог решено добавить — иначе через test fixture).
- При cancel — очередь pending chunks немедленно дропается.

### Что **не** делать

- Не параллелить чанки для `localWhisper` / `browserWhisper`.
- Не вводить per-host rate limit (одна цель API — только её лимит важен).
- Не менять текущий формат чекпоинтов.

---

## Зависимости между задачами

```
TASK-01..TASK-10 — описаны в исторической части (см. PLAN_NEXT.md о статусе)
TASK-11 — независима. Главный выигрыш для дефолтного сценария PO.
TASK-12 — независима по коду. Зависит от завершения BA (готово). Включает инфраструктуру `PipelineLanes`.
TASK-13 — зависит от TASK-12 (использует `PipelineLanes::http_transcribe`). Если TASK-13 стартует раньше — вынести `concurrency.rs` отдельным шагом и реализовать infra-only часть TASK-12 досрочно.
```

## Порядок выполнения (рекомендуемый)

Текущий фокус (parallelization, BA-артефакт от 2026-04-20):

1. **TASK-11** — pipeline overlap. Высокий приоритет, можно стартовать немедленно. Главный выигрыш для PO.
2. **TASK-12** — параллельная очередь + инфраструктура семафоров. Средний приоритет. Готовит почву для TASK-13.
3. **TASK-13** — параллельные чанки HTTP API. Низкий приоритет (PO на localWhisper).

Исторический хвост (если ещё не закрыт по `PLAN_NEXT.md`):
1. TASK-09 (BrowserWhisper fix) — блокирует пользователей уже сейчас
2. TASK-01 (таб) — визуально улучшит всё остальное
3. TASK-08 (дизайн-фиксы) — параллельно с TASK-01
4. TASK-10 (статистика) — параллельно с TASK-08
5. TASK-02 + TASK-03 параллельно (whisper macOS / Linux)
6. TASK-07 (улучшенный поиск, зависит от TASK-02)
7. TASK-04 (save video)
8. TASK-05 + TASK-06 параллельно (quality of life)
