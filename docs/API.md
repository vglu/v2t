# v2t REST API

Локальный HTTP-сервер для интеграции с внешними сервисами. Запускается **внутри запущенного приложения v2t**, слушает только на `127.0.0.1`. Когда v2t закрыт — сервер недоступен.

> **Назначение:** один внутренний потребитель (другой ваш сервис) ставит задачи на транскрибацию по URL или локальному файлу, опрашивает статус и/или получает webhook по завершении.

## Безопасность

- **Bind:** строго `127.0.0.1` (loopback). Никакого `0.0.0.0`, никакого LAN-доступа.
- **Auth:** один статический Bearer-токен в заголовке `Authorization: Bearer <token>`.
- **Без TLS:** loopback не нуждается; внешним сервисам ходить через ваш собственный backend, не напрямую.

---

## Включение сервера

1. Откройте `settings.json` (см. путь ниже) или используйте UI.

2. Добавьте/измените секцию:
   ```json
   "apiServer": {
     "enabled": true,
     "port": 8788,
     "bearerToken": ""
   }
   ```
   Оставьте `bearerToken` пустым — он сгенерируется автоматически при первом запуске (32 байта в hex).

3. Перезапустите v2t **или** из фронта вызовите команду `api_server_apply` — она перечитает настройки и поднимет/перезапустит сервер.

4. Узнайте токен:
   - **из UI:** команда `get_api_server_info` (вернёт `{ enabled, running, port, bearerToken, baseUrl }`);
   - **из файла:** в том же `settings.json` появится сгенерированный `bearerToken`;
   - **из логов:** при первом включении в session log пишется `generated bearer token on first enable`.

Путь к `settings.json`:
- **Windows:** `%APPDATA%\com.v2t.app\settings.json` (точное имя bundle id см. в `src-tauri/tauri.conf.json`)
- **macOS:** `~/Library/Application Support/com.v2t.app/settings.json`

---

## Swagger UI

Откройте в браузере: **http://127.0.0.1:8788/v1/docs**

Кликните **Authorize** в правом верхнем углу, вставьте Bearer-токен — после этого "Try it out" будет дёргать реальные эндпоинты. OpenAPI 3.1 JSON отдаётся отдельно по `/v1/openapi.json` (удобно для генерации клиентов через `openapi-generator-cli`, `openapi-typescript`, и т.п.).

И UI, и JSON-спека отдаются **без auth** — это discovery; ничего секретного в схеме нет. Auth требуется для самих вызовов через UI.

## Эндпоинты

Все ответы — JSON, если не указано иное. Кроме `/v1/health`, `/v1/docs` и `/v1/openapi.json`, все требуют `Authorization: Bearer <token>`.

| Метод | Путь | Назначение |
|---|---|---|
| `GET` | `/v1/health` | Liveness check (без auth) |
| `GET` | `/v1/docs` | **Swagger UI** (без auth) |
| `GET` | `/v1/openapi.json` | OpenAPI 3.1 спецификация (без auth) |
| `POST` | `/v1/jobs` | Поставить одиночную задачу |
| `GET` | `/v1/jobs/{id}` | Статус и прогресс задачи |
| `GET` | `/v1/jobs/{id}/transcript` | Текст транскрипта (когда `done`) |
| `GET` | `/v1/jobs/{id}/events` | SSE-стрим прогресса (`text/event-stream`) |
| `POST` | `/v1/jobs/{id}/cancel` | Отменить выполняющуюся задачу |
| `POST` | `/v1/batches` | Поставить пачку задач (до 1000) |
| `GET` | `/v1/batches/{id}` | Агрегированный статус батча |

### `GET /v1/health`

```json
{ "ok": true, "version": "1.7.0" }
```

### `POST /v1/jobs`

**Тело запроса:**

```json
{
  "source": "https://www.youtube.com/watch?v=...",
  "sourceKind": "auto",
  "displayLabel": "My talk",
  "options": {
    "language": "ru",
    "outputDir": "D:/work/transcripts",
    "transcriptionMode": "localWhisper",
    "whisperModel": "small"
  },
  "callback": {
    "url": "https://my-service.example/webhooks/v2t",
    "secret": "shared-secret-for-hmac"
  }
}
```

| Поле | Тип | Обязат. | Описание |
|---|---|---|---|
| `source` | string | да | URL **или** абсолютный путь к локальному файлу |
| `sourceKind` | `"url"\|"file"\|"auto"` | нет | По умолчанию `auto` — определяется по `http(s)://` |
| `displayLabel` | string | нет | Метка для шаблона имени файла (`{title}`). По умолчанию = `source` |
| `options.language` | string | нет | ISO-код языка для Whisper (`ru`, `en`, …) |
| `options.outputDir` | string | нет | Куда писать транскрипт. По умолчанию — `outputDir` из настроек |
| `options.transcriptionMode` | `"httpApi"\|"localWhisper"` | нет | `browserWhisper` отклоняется (нужен webview) |
| `options.whisperModel` | string | нет | id из каталога (`tiny`, `base`, `small`, …) |
| `callback` | object | нет | См. раздел Webhook ниже |

**Ответ 202 Accepted:**

```json
{
  "jobId": "api-9c1a...",
  "status": "queued",
  "location": "/v1/jobs/api-9c1a..."
}
```

**Ошибки:**
- `400` — пустой `source`, неверный `sourceKind`, `browserWhisper`, отсутствует `outputDir`
- `401` — нет/неверный Bearer-токен
- `503` — токен не сконфигурирован

### `GET /v1/jobs/{id}`

```json
{
  "id": "api-9c1a...",
  "status": "running",
  "createdAt": "2026-05-19T12:34:56Z",
  "updatedAt": "2026-05-19T12:35:02Z",
  "source": "https://...",
  "sourceKind": "url",
  "progress": {
    "phase": "transcribe",
    "message": "Transcribing track 1/1 (splitting if file is large)…",
    "subtaskIndex": 1,
    "subtaskTotal": 1,
    "subtaskPercent": 45
  },
  "transcriptPath": null,
  "error": null
}
```

`status` ∈ `queued | running | done | failed | cancelled | interrupted`.
`interrupted` — задача была в работе, когда v2t остановился; после перезапуска восстановлена из БД, но уже не выполняется (нужно поставить заново).

### `GET /v1/jobs/{id}/transcript`

Возвращает `text/plain; charset=utf-8` — содержимое транскрипта.
- `409 Conflict`, если `status != done`
- `404`, если задача неизвестна

### `POST /v1/jobs/{id}/cancel`

```json
{ "cancelled": true }
```
- `404`, если активной задачи с таким id нет.

### `GET /v1/jobs/{id}/events` — SSE

Возвращает `text/event-stream` с live-обновлениями состояния задачи. Первым отдаёт текущий snapshot, затем — все последующие изменения до терминального статуса.

Формат события:
```
event: snapshot
data: {"kind":"snapshot","id":"api-...","status":"running","progress":{...},...}

event: terminal
data: {"kind":"terminal"}
```

`event: snapshot` — полный JSON ApiJob (как от `GET /v1/jobs/{id}`).
`event: terminal` — закрывающее событие; сервер закроет поток сразу после.

Keep-alive: каждые 15 с шлётся `:` (ping-комментарий по SSE-конвенции).

### `POST /v1/batches`

Постановка пачки задач за один запрос. Лимит — **1000 items**. Каждый item — такой же объект, как тело `POST /v1/jobs`. Опционально — `defaults` (опции/callback), применяются ко всем items; item-level поля имеют приоритет.

```json
{
  "items": [
    { "source": "https://example.com/a.mp3" },
    { "source": "https://example.com/b.mp3", "displayLabel": "Item B",
      "options": { "language": "en" } }
  ],
  "defaults": {
    "options": {
      "outputDir": "D:/work/batch-2026-05-19",
      "language": "ru",
      "transcriptionMode": "localWhisper",
      "whisperModel": "small"
    },
    "callback": {
      "url": "https://my-service.example/webhooks/v2t",
      "secret": "shared-secret"
    }
  }
}
```

**Ответ 202:**

```json
{
  "batchId": "batch-2f8a...",
  "jobIds": ["api-...", "api-..."],
  "location": "/v1/batches/batch-2f8a..."
}
```

**Атомарность:** если *любой* item не валиден (нет `source`, выключенный `outputDir`, `browserWhisper`, и т.п.), весь батч отклоняется с `4xx`. Уже зарегистрированные на этот батч джобы помечаются `cancelled`.

**Конкурентность:** глобальный лимит на сервере — **2 параллельных запуска** (M3 хардкод). Остальные ждут своей очереди в семафоре. Cancel на ожидающую задачу работает мгновенно — она не запустится.

### `GET /v1/batches/{id}`

```json
{
  "id": "batch-2f8a...",
  "createdAt": "2026-05-19T12:00:00Z",
  "total": 2,
  "queued": 0,
  "running": 1,
  "done": 0,
  "failed": 0,
  "cancelled": 0,
  "jobIds": ["api-...", "api-..."]
}
```

Опросом этого эндпоинта можно следить за прогрессом батча целиком. Для пер-job детализации — `GET /v1/jobs/{jobId}` или SSE на каждую отдельно.

---

## Webhook

Если в `POST /v1/jobs` передан `callback.url`, v2t отправит POST на этот URL при достижении терминального статуса.

**Заголовки:**
- `Content-Type: application/json`
- `X-V2T-Event: job.completed | job.failed | job.cancelled`
- `X-V2T-Delivery-Id: <uuid>` — идемпотентный токен доставки
- `X-V2T-Signature: <hex>` — HMAC-SHA256 от тела по `callback.secret` (если секрет задан)

**Тело:**

```json
{
  "event": "job.completed",
  "jobId": "api-9c1a...",
  "data": {
    "status": "done",
    "transcriptPath": "D:/work/transcripts/My talk_2026-05-19.txt",
    "summary": "Saved: D:/work/transcripts/..."
  }
}
```

Для `job.failed` / `job.cancelled` в `data` будут `status` и `error`.

**Retry:** до 3 попыток с экспоненциальной задержкой (≈0.5 с, 1 с). 4xx не ретраится. Окончательная неудача только логируется в session log.

**Проверка подписи (Python):**

```python
import hmac, hashlib
sig = request.headers["X-V2T-Signature"]
body = request.get_data()
expected = hmac.new(secret.encode(), body, hashlib.sha256).hexdigest()
if not hmac.compare_digest(sig, expected):
    abort(401)
```

---

## Примеры

### curl

```bash
TOKEN=...   # из get_api_server_info / settings.json
BASE=http://127.0.0.1:8788

# Health
curl $BASE/v1/health

# Submit (URL)
curl -X POST $BASE/v1/jobs \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "source": "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
    "options": { "language": "en", "outputDir": "/tmp/v2t-out" }
  }'

# Submit (local file)
curl -X POST $BASE/v1/jobs \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "source": "/Users/me/talk.mp4",
    "options": { "outputDir": "/Users/me/transcripts" }
  }'

# Status
curl -H "Authorization: Bearer $TOKEN" $BASE/v1/jobs/api-9c1a...

# Transcript
curl -H "Authorization: Bearer $TOKEN" $BASE/v1/jobs/api-9c1a.../transcript

# Cancel
curl -X POST -H "Authorization: Bearer $TOKEN" $BASE/v1/jobs/api-9c1a.../cancel

# SSE — live progress (curl -N = --no-buffer)
curl -N -H "Authorization: Bearer $TOKEN" $BASE/v1/jobs/api-9c1a.../events

# Batch
curl -X POST $BASE/v1/batches \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "items": [
      { "source": "https://example.com/a.mp3" },
      { "source": "https://example.com/b.mp3" }
    ],
    "defaults": {
      "options": { "outputDir": "/tmp/v2t-batch", "language": "ru" }
    }
  }'

# Batch status
curl -H "Authorization: Bearer $TOKEN" $BASE/v1/batches/batch-2f8a...
```

### Python (минимальный клиент)

```python
import requests, time

BASE = "http://127.0.0.1:8788"
TOKEN = "..."
H = {"Authorization": f"Bearer {TOKEN}"}

# Submit
r = requests.post(f"{BASE}/v1/jobs", json={
    "source": "https://example.com/talk.mp3",
    "options": {"language": "ru", "outputDir": "/tmp/out"}
}, headers=H)
r.raise_for_status()
job_id = r.json()["jobId"]

# Poll
while True:
    job = requests.get(f"{BASE}/v1/jobs/{job_id}", headers=H).json()
    if job["status"] in ("done", "failed", "cancelled"):
        break
    print(job.get("progress", {}).get("message", job["status"]))
    time.sleep(2)

# Result
if job["status"] == "done":
    txt = requests.get(f"{BASE}/v1/jobs/{job_id}/transcript", headers=H).text
    print(txt[:200])
else:
    print("Failed:", job.get("error"))
```

### Python: SSE-клиент (sseclient-py)

```python
import requests, json
from sseclient import SSEClient

TOKEN = "..."
job_id = "api-..."
url = f"http://127.0.0.1:8788/v1/jobs/{job_id}/events"
resp = requests.get(url, headers={"Authorization": f"Bearer {TOKEN}"}, stream=True)
for ev in SSEClient(resp).events():
    if ev.event == "terminal":
        print("done streaming")
        break
    snapshot = json.loads(ev.data)
    print(snapshot["status"], snapshot.get("progress", {}).get("message", ""))
```

### Python: батч с ожиданием

```python
import requests, time

BASE = "http://127.0.0.1:8788"
H = {"Authorization": "Bearer ..."}

r = requests.post(f"{BASE}/v1/batches", json={
    "items": [{"source": u} for u in ["https://a", "https://b", "https://c"]],
    "defaults": {"options": {"outputDir": "/tmp/v2t-batch", "language": "ru"}},
}, headers=H).json()

batch_id = r["batchId"]
while True:
    s = requests.get(f"{BASE}/v1/batches/{batch_id}", headers=H).json()
    print(f"done={s['done']} failed={s['failed']} running={s['running']} queued={s['queued']}")
    if s["done"] + s["failed"] + s["cancelled"] == s["total"]:
        break
    time.sleep(3)
```

### Webhook-приёмник (Flask)

```python
from flask import Flask, request, abort
import hmac, hashlib

SECRET = b"shared-secret-for-hmac"
app = Flask(__name__)

@app.post("/webhooks/v2t")
def hook():
    sig = request.headers.get("X-V2T-Signature", "")
    expected = hmac.new(SECRET, request.get_data(), hashlib.sha256).hexdigest()
    if not hmac.compare_digest(sig, expected):
        abort(401)
    payload = request.get_json()
    print(payload["event"], payload["jobId"], payload["data"])
    return ""
```

---

## Что работает / что в плане

| Волна | Состояние | Что внутри |
|---|---|---|
| M1 | ✅ done | Рефакторинг прогресса на `ProgressSink` |
| M2 | ✅ done | REST + bearer + webhook (HMAC) |
| M3 | ✅ done | Батчи до 1000 + SSE-стрим прогресса |
| M3.5 | ✅ done | OpenAPI 3.1 + Swagger UI (`/v1/docs`, `/v1/openapi.json`) |
| M4 | ✅ done | SQLite-персистентность + UI-панель управления (Settings → REST API) |
| M5 | план | Метрики, S3/MinIO выгрузка, авторестарт interrupted-задач |

> **Персистентность (M4):** джобы и батчи пишутся в `app_data/v2t-api.db` (SQLite, WAL). История переживает рестарт v2t. Задачи, бывшие в работе на момент остановки, при следующем старте помечаются `interrupted`. Сам REST-сервер живёт только пока запущено приложение.
>
> **Управление из UI:** Settings → секция **REST API** — тумблер вкл/выкл, порт, показ/копирование токена, регенерация, ссылка на Swagger. Можно не править `settings.json` руками.
