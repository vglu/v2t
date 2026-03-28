# Выпуск версии (maintainers)

## 1. Синхронизировать версию

Одно и то же значение (например `1.0.1`) во всех файлах:

| Файл | Поле |
|------|------|
| `package.json` | `version` |
| `src-tauri/Cargo.toml` | `[package] version` |
| `src-tauri/tauri.conf.json` | `version` |

При необходимости обновите примеры имён установщиков в `README.md`.

Если в GitHub Actions падает шаг **`npm ci`** на Linux/macOS с сообщением вроде *Missing: @tauri-apps/cli-linux-x64-gnu from lock file*, значит в `package-lock.json` не хватает записей для optional-пакетов других ОС (часто после сборки lock-файла только на Windows). Пересоберите lock-файл: удалите `node_modules` и `package-lock.json`, выполните **`npm install`**, закоммитьте новый `package-lock.json` (в нём должны появиться секции `node_modules/@tauri-apps/cli-linux-*`, `node_modules/@esbuild/*` и т.д.).

## 2. Обновить `CHANGELOG.md`

Добавьте секцию с датой и списком изменений для пользователей.

## 3. Закоммитить и запушить

```bash
git add -A
git commit -m "chore: release v1.0.1"
git push origin main
```

(ветка может называться `master` — подставьте свою.)

## 4. Тег и GitHub Release

```bash
git tag v1.0.1
git push origin v1.0.1
```

Workflow **Release** (`.github/workflows/release.yml`) соберёт артефакты и прикрепит их к [GitHub Release](https://docs.github.com/repositories/releasing-projects-on-github/about-releases).

### Права `GITHUB_TOKEN`

Если релиз падает с **Resource not accessible by integration**: **Settings → Actions → General → Workflow permissions** → **Read and write permissions**.

## 5. Локальная сборка (без CI)

```bash
npm ci
npm run tauri build
```

**Windows:** перед сборкой закройте запущенное приложение **Video to Text**, иначе линкер не сможет перезаписать `v2t.exe` (ошибка *Access is denied*).

**Linux:** зависимости как в CI — см. шаг `Install Linux dependencies` в `.github/workflows/ci.yml`.

## 6. Опционально: подпись и нотаризация

В `release.yml` закомментированы переменные для **Apple** (сертификат, notarization) и при необходимости можно добавить шаги подписи **Windows**. После настройки секретов в репозитории раскомментируйте и дополните workflow по [документации Tauri](https://v2.tauri.app/distribute/).

## 7. Ссылка в CHANGELOG

Внизу `CHANGELOG.md` замените `OWNER/REPO` на реальный путь репозитория на GitHub.
