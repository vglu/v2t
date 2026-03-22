# AGENTS — v2t

Инструкции для AI-агентов в Cursor по этому репозиторию.

## Язык

Ответы пользователю — **русский**, комментарии в коде и UI-строки по умолчанию **английский** (если не оговорено иначе).

## Назначение проекта

**v2t** — портативное десктоп-приложение: получить текст из **видео/аудио** (файл, папка, URL YouTube/плейлист). Внешние бинарники: **ffmpeg**, **yt-dlp**; транскрипция — **HTTP API** (OpenAI-совместимый) **или** локально **whisper.cpp** (`whisper-cli`).

Пошаговый план работ: **`docs/PLAN.md`** (Фазы 1–7).  
BA-разбор требований для оркестратора: **`.cursor/tasks/BA-20250322-120000-alice.md`**.

## Стек

- Tauri 2, React, TypeScript, Vite
- Rust в `src-tauri` для команд и процессов (`process_queue_item`, `cancel_queue_job`, kill дерева процесса при отмене на Windows; HTTP-транскрипция и локальный **whisper-cli** с отменой)

## Тесты

- **Юнит / компоненты (JS):** `npm run test` (watch) или `npm run test:run` (один прогон; Vitest + Testing Library).
- **Юнит (Rust):** `cd src-tauri && cargo test`.
- **E2E (UI в браузере):** `npm run e2e` (поднимает `npm run dev`, таймаут webServer до 180s, `workers: 1`); при занятом порте 1420 освободи порт или запусти `npm run dev` и `e2e` с `reuseExistingServer`.

## Обязательное поведение

1. Выполнять команды в терминале самостоятельно, не ограничиваться советами «запусти сам».
2. Минимальный объём изменений; не трогать несвязанные файлы.
3. Не коммитить секреты; API keys только через настройки приложения / keychain.
4. Ссылаться на существующий код в формате Cursor: блоки с номерами строк и путём файла.

## Субагенты проекта

Файлы в **`.cursor/agents/`** (приоритет над глобальными одноимёнными, если Cursor разрешает override):

| Имя | Назначение |
|-----|------------|
| `kieran-typescript-reviewer` | Строгий обзор TypeScript/React |
| `security-sentinel` | Безопасность, секреты, инъекции, Tauri capabilities |
| `code-simplicity-reviewer` | YAGNI, упрощение после фичи |
| `julik-frontend-races-reviewer` | Гонки в async UI, очередь, отмена |
| `rust-tauri-reviewer` | Rust/Tauri команды и sidecars |
| `v2t-pipeline-specialist` | Цепочка yt-dlp → ffmpeg → API |

Вызывай их по смыслу задачи (ревью, безопасность, пайплайн).

## Правила Cursor

- **`.cursor/rules/v2t-project.mdc`** — всегда включённые правила репозитория.
- Глобально у пользователя: **`C:\Users\vetal\.cursor\rules\ports-global.mdc`** — учитывать при появлении Docker/сервисов.

## Скиллы на машине (не дублировать в git)

При необходимости читай:

- `C:\Users\vetal\.cursor\skills\bootstrap-new-project\SKILL.md`
- `C:\Users\vetal\.cursor\skills-cursor\create-rule\SKILL.md`
- `C:\Users\vetal\.cursor\skills-cursor\create-skill\SKILL.md`
- `C:\Users\vetal\.agents\skills\` — доменные (Azure, и т.д.)

## MCP

Конфигурация MCP остаётся в **пользовательских** настройках Cursor. **Не** добавляй токены и ключи в файлы проекта.
