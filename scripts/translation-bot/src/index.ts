#!/usr/bin/env node
/**
 * CLI entry — parses argv, dispatches to subcommands.
 *
 *   smoke                       — 5 strings × 4 locales (~3-5 min, validates
 *                                  prompt + Ollama warm-up + glossary check)
 *   run --all                   — full bulk run, all configured locales
 *   run --locale=de             — single locale
 *   status                      — print current state + ETA, no work
 *   reset --locale=de           — discard state + draft for one locale
 *   reset --all                 — nuclear: discard all state + drafts
 *
 * No external CLI library — `process.argv` parsing is trivial here and
 * keeps the bot a zero-dep TypeScript script that runs via `tsx`.
 */
import { rm } from 'node:fs/promises';
import path from 'node:path';
import {
  DRAFTS_DIR,
  OLLAMA_MODEL,
  STATE_FILE,
  TARGET_LOCALES,
  type TargetLocale,
} from './config.js';
import { loadAudit } from './audit.js';
import { listAvailableModels, listLoadedModels, preflight } from './ollama.js';
import { loadState } from './state.js';
import { runTranslation } from './translator.js';
import { reportRunEnd } from './progress.js';
import { generateReview } from './review.js';

interface ParsedArgs {
  command: 'smoke' | 'run' | 'status' | 'reset' | 'review' | 'help';
  all: boolean;
  locale?: TargetLocale;
  limit?: number;
}

function parseArgs(argv: string[]): ParsedArgs {
  const [, , cmd = 'help', ...rest] = argv;
  const all = rest.includes('--all');
  let locale: TargetLocale | undefined;
  let limit: number | undefined;
  for (const arg of rest) {
    if (arg.startsWith('--locale=')) {
      const v = arg.slice('--locale='.length);
      if ((TARGET_LOCALES as readonly string[]).includes(v)) {
        locale = v as TargetLocale;
      } else {
        throw new Error(`Invalid --locale=${v}. Allowed: ${TARGET_LOCALES.join(', ')}`);
      }
    } else if (arg.startsWith('--limit=')) {
      const n = Number(arg.slice('--limit='.length));
      if (!Number.isFinite(n) || n <= 0) throw new Error(`Invalid --limit=${arg}`);
      limit = n;
    }
  }
  return {
    command:
      cmd === 'smoke' ||
      cmd === 'run' ||
      cmd === 'status' ||
      cmd === 'reset' ||
      cmd === 'review'
        ? cmd
        : 'help',
    all,
    locale,
    limit,
  };
}

async function main(): Promise<number> {
  const args = parseArgs(process.argv);

  if (args.command === 'help') {
    printHelp();
    return 0;
  }

  if (args.command === 'status') {
    return runStatus();
  }

  if (args.command === 'reset') {
    return runReset(args);
  }

  if (args.command === 'review') {
    return runReview(args);
  }

  // smoke + run both go through preflight → translator
  // eslint-disable-next-line no-console
  console.log(`[bot] preflight: model=${OLLAMA_MODEL}, validating Ollama…`);
  const pf = await preflight();
  // eslint-disable-next-line no-console
  console.log(
    `[bot] preflight OK — warm-up ${(pf.warmupMs / 1000).toFixed(1)}s, loaded models: ${pf.loadedModels.join(', ') || '(none reported)'}`,
  );

  const locales =
    args.locale != null
      ? [args.locale]
      : args.all
        ? [...TARGET_LOCALES]
        : args.command === 'smoke'
          ? [...TARGET_LOCALES]
          : (() => {
              throw new Error(
                'run requires --all or --locale=<uk|ru|de|es|fr|pl|pt>. Try: run --all',
              );
            })();

  const limit =
    args.command === 'smoke' ? Math.min(args.limit ?? 5, 5) : args.limit;

  const finalState = await runTranslation({
    locales,
    limit,
  });

  reportRunEnd(finalState);
  return 0;
}

async function runStatus(): Promise<number> {
  const state = await loadState();
  if (!state) {
    // eslint-disable-next-line no-console
    console.log('[bot] no state file — bot has never run. Use `run --all` to start.');
    return 0;
  }
  const audit = await loadAudit().catch(() => null);
  const loaded = await listLoadedModels().catch(() => [] as string[]);
  const available = await listAvailableModels().catch(() => [] as string[]);

  // eslint-disable-next-line no-console
  console.log(`Model:           ${state.model}`);
  // eslint-disable-next-line no-console
  console.log(`Started:         ${state.startedAt}`);
  // eslint-disable-next-line no-console
  console.log(`Last update:     ${state.lastUpdatedAt}`);
  if (audit) {
    // eslint-disable-next-line no-console
    console.log(
      `Audit gen:       ${audit.generatedAt} ${audit.generatedAt === state.auditGeneratedAt ? '✓' : '⚠ drift'}`,
    );
  }
  // eslint-disable-next-line no-console
  console.log(
    `Ollama loaded:   ${loaded.join(', ') || '(none)'} | available: ${available.length}`,
  );
  // eslint-disable-next-line no-console
  console.log('');
  // eslint-disable-next-line no-console
  console.log('Per-locale progress:');
  for (const locale of TARGET_LOCALES) {
    const ls = state.locales[locale];
    const pct = ls.total > 0 ? ((ls.completed / ls.total) * 100).toFixed(1) : '0.0';
    const avg = ls.completed > 0 ? (ls.elapsedMs / ls.completed / 1000).toFixed(1) : '—';
    const remaining = Math.max(0, ls.total - ls.completed);
    const eta =
      ls.completed > 0 ? `${((remaining * (ls.elapsedMs / ls.completed)) / 1000 / 60).toFixed(1)}m` : '—';
    // eslint-disable-next-line no-console
    console.log(
      `  ${locale}  ${ls.completed.toString().padStart(4)}/${ls.total.toString().padStart(4)} (${pct.padStart(5)}%) · avg ${avg}s · warnings ${ls.warnings} · remaining ETA ~${eta}`,
    );
  }
  return 0;
}

async function runReview(args: ParsedArgs): Promise<number> {
  if (!args.locale && !args.all) {
    // eslint-disable-next-line no-console
    console.error('review requires --locale=<uk|ru|de|es|fr|pl|pt> or --all.');
    return 1;
  }
  const locales: TargetLocale[] = args.all ? [...TARGET_LOCALES] : [args.locale!];
  for (const l of locales) {
    const out = await generateReview({ locale: l, limit: args.limit });
    // eslint-disable-next-line no-console
    console.log(`[review] ${l} → ${out}`);
  }
  return 0;
}

async function runReset(args: ParsedArgs): Promise<number> {
  if (!args.all && !args.locale) {
    // eslint-disable-next-line no-console
    console.error('reset requires --all or --locale=<uk|ru|de|es|fr|pl|pt>.');
    return 1;
  }
  if (args.all) {
    await rm(STATE_FILE, { force: true });
    for (const l of TARGET_LOCALES) {
      await rm(path.join(DRAFTS_DIR, `${l}.draft.json`), { force: true });
    }
    // eslint-disable-next-line no-console
    console.log('[bot] reset complete — state.json + all drafts removed.');
  } else if (args.locale) {
    // For per-locale reset we leave state.json but zero out the locale entry.
    const state = await loadState();
    if (state) {
      state.locales[args.locale] = {
        total: 0,
        completed: 0,
        completedKeys: [],
        lastKey: null,
        elapsedMs: 0,
        warnings: 0,
      };
      const { saveState } = await import('./state.js');
      await saveState(state);
    }
    await rm(path.join(DRAFTS_DIR, `${args.locale}.draft.json`), { force: true });
    // eslint-disable-next-line no-console
    console.log(`[bot] reset ${args.locale} — locale state cleared, draft removed.`);
  }
  return 0;
}

function printHelp(): void {
  // eslint-disable-next-line no-console
  console.log(`
v2t Translation Bot — local Ollama UI string translator

USAGE
  npx tsx scripts/translation-bot/src/index.ts <command> [options]

COMMANDS
  smoke                 5 strings × 7 locales — validates prompt + warm-up
  run --all             Full run, all 7 locales (uk/ru/de/es/fr/pl/pt) — overnight job
  run --locale=uk       Single locale
  run --limit=50        Cap strings per locale (smoke / partial)
  status                Print current progress, ETA, and Ollama state
  reset --all           Discard all state + drafts (nuclear)
  reset --locale=uk     Reset one locale only

ENVIRONMENT
  OLLAMA_URL            Default: http://localhost:11434
  OLLAMA_MODEL          Default: Qwen2.5-Coder:latest
  THROTTLE_MS           Sleep between calls (default 200)

EXAMPLES
  # Validate prompt quality before bulk
  npm --prefix scripts/translation-bot run smoke

  # Full overnight run
  npm --prefix scripts/translation-bot run run:all

  # Resume single locale after crash
  npx tsx scripts/translation-bot/src/index.ts run --locale=uk

  # Check progress mid-run from another shell
  npm --prefix scripts/translation-bot run status

  # Re-translate Ukrainian from scratch
  npx tsx scripts/translation-bot/src/index.ts reset --locale=uk
  npx tsx scripts/translation-bot/src/index.ts run --locale=uk
`);
}

main()
  .then((code) => process.exit(code))
  .catch((err) => {
    // eslint-disable-next-line no-console
    console.error(`\n[bot] fatal: ${err instanceof Error ? err.message : String(err)}`);
    if (err instanceof Error && err.stack) {
      // eslint-disable-next-line no-console
      console.error(err.stack);
    }
    process.exit(1);
  });
