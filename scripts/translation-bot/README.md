# Translation bot — local Ollama, Mercedes-grade UI string translator

W1B — full TypeScript pipeline for translating untranslated keys from
`web/messages/en.json` to `de`/`es`/`fr`/`pt` via local Ollama
(`Qwen2.5-Coder:latest`). Resumes-on-crash, atomic state, draft-only
output (never writes to prod messages directly — human review gates
the merge).

## Why local Ollama, not cloud LLM

Per Mercedes Constitution + CLAUDE.md §12 — ни один user-facing string
не идёт на прод без human-quality review. Local Ollama gives:

- **Cost**: $0 (vs $20-100 for Anthropic at this batch size)
- **Privacy**: nothing leaves the machine
- **Iteration**: tweak prompt → re-run → no API rate limits
- **Drafts only**: output is `output/drafts/{locale}.draft.json` —
  human review pulls approved entries into `web/messages/{locale}.json`

## Triggers

- **«запусти переводы»** → `npx tsx scripts/translation-bot/src/index.ts run --all`
  (resumable; picks up where state.json left off)
- **«останови переводы»** → Ctrl+C the foreground (graceful, saves
  state before exit) or `taskkill /PID <pid> /F` for kill-9 (state
  flushed every 5 strings, so worst-case loss = 5 entries)

## Hardware envelope

Tested on **RTX 3060 Ti (8 GB VRAM)** at ~7.9s/string with partial GPU
offload. Bulk run for 6,799 strings ≈ 14-15h wall-clock — typically
split across 2 nights overnight.

VRAM requirement: **≥ 5 GB free** before starting. Close NVIDIA
Broadcast / Lightroom / heavy browser tabs / MS Teams beforehand. Bot
auto-checks via `/api/ps` and aborts if model can't fit.

## Quickstart

```bash
# 1. Verify Ollama running on bare-metal localhost:11434
curl http://localhost:11434/api/tags

# 2. Verify Qwen2.5-Coder:latest is pulled
ollama list | grep -i qwen

# 3. Run smoke test first (5 strings, ~40s, validates prompt + connection)
npx tsx scripts/translation-bot/src/index.ts smoke

# 4. Run all 4 locales (overnight)
npx tsx scripts/translation-bot/src/index.ts run --all

# 5. Or single locale
npx tsx scripts/translation-bot/src/index.ts run --locale=de

# 6. Check progress at any time
npx tsx scripts/translation-bot/src/index.ts status

# 7. Reset state for a locale (force re-translate)
npx tsx scripts/translation-bot/src/index.ts reset --locale=de
```

## Output structure

```
scripts/translation-bot/
├── output/
│   └── drafts/
│       ├── de.draft.json    # 1806 translations + warnings + timing
│       ├── es.draft.json    # 1626
│       ├── fr.draft.json    # 1765
│       └── pt.draft.json    # 1602
├── state/
│   └── bot-state.json       # resume info per locale (last completed key + counts)
└── logs/
    └── run-<timestamp>.log  # full run transcript (errors, slow strings, warnings)
```

`output/` and `state/` are gitignored — not commit drafts. Human
review approves each translation, then merges into prod
`web/messages/{locale}.json` (manual or via approval-merge tool).

## Brand glossary (never translated)

NumbersM, Numia, Premium, Pro, Free, Family, Practitioner, Telegram,
WhatsApp, Apple, Google, Stripe, JWT, API, OAuth, iOS, Android,
TestFlight. Plus all numerology number-words («7», «22/4», etc.).

Bot post-validates: if EN had glossary term and translation doesn't,
entry marked `warning: "GLOSSARY_LOST"` for human attention.

## Validation per translation

Each translation gets a `warning` flag if any of these fire:

- `GLOSSARY_LOST` — protected term in EN but missing from translation
- `PLACEHOLDER_DRIFT` — `{var}` count mismatch between EN and translation
- `LENGTH_OUT_OF_BAND` — translation length < 50% or > 200% of EN
- `MODEL_PREFIX` — translation starts with «Here's the translation:» or similar artifact
- `EMPTY` — translation is empty string
- `CHECK_REQUESTED` — model self-flagged with «⚠️CHECK» (per prompt rule #6)

`null` warning = clean. Warnings are surfacing-only, NOT auto-rejection
— human reviewer decides.

## Choice of model (decision log)

Smoke harness 2026-04-27 compared:

| Model | Size | Avg latency | Placeholder preservation | Notes |
|-------|------|-------------|-------------------------|-------|
| aya-expanse:8b | 5.0 GB | 9.2s | 60% | Hallucinates decorations, drops bold |
| Qwen2.5-Coder:7b | 4.7 GB | **7.9s** | **94%** | **Selected for W1B** |
| Qwen2.5-Coder:14b | 8.9 GB | 14.3s | 96% | 2% better quality, 80% slower → not worth |

Override via env: `OLLAMA_MODEL=Qwen2.5-Coder:14b npx tsx ... run --all`.

## Mercedes invariants

1. **Atomic writes** — state and drafts use temp+rename; no half-written JSON ever
2. **Graceful shutdown** — SIGINT/SIGTERM → flush state → exit clean
3. **Crash-safe** — state flushed every 5 strings; worst-case loss = 5 entries
4. **Retry with backoff** — Ollama 503 / timeout / network → 3 retries (2s/5s/10s)
5. **Warm-up call** — first request loads model; bot waits and confirms via `/api/ps` before bulk start
6. **Throttled** — 200ms between calls keeps GPU steady; if avg latency >15s/string, auto-pause for 30s
7. **Drafts only** — never touches `web/messages/*.json` directly
