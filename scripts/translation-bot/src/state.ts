/**
 * Atomic state file persistence.
 *
 * State tracks per-locale progress: how many strings completed, last
 * key processed, cumulative time. Resume reads this and skips already-
 * translated keys.
 *
 * Atomicity: write to temp file, then rename. POSIX rename is atomic;
 * Node's `fs.rename` on Windows is also atomic on the same volume.
 * No half-written JSON ever lands on disk.
 */
import { mkdir, readFile, rename, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { STATE_DIR, STATE_FILE, TARGET_LOCALES, type TargetLocale } from './config.js';

export interface LocaleState {
  total: number;
  completed: number;
  /** Set of completed keys — fast skip on resume. */
  completedKeys: string[];
  lastKey: string | null;
  elapsedMs: number;
  /** Sum of warnings emitted across this run. */
  warnings: number;
}

export interface BotState {
  version: 1;
  model: string;
  startedAt: string;
  lastUpdatedAt: string;
  auditGeneratedAt: string;
  locales: Record<TargetLocale, LocaleState>;
}

function emptyLocaleState(): LocaleState {
  return {
    total: 0,
    completed: 0,
    completedKeys: [],
    lastKey: null,
    elapsedMs: 0,
    warnings: 0,
  };
}

export function emptyState(model: string, auditGeneratedAt: string): BotState {
  const now = new Date().toISOString();
  const locales = Object.fromEntries(
    TARGET_LOCALES.map((l) => [l, emptyLocaleState()]),
  ) as Record<TargetLocale, LocaleState>;
  return {
    version: 1,
    model,
    startedAt: now,
    lastUpdatedAt: now,
    auditGeneratedAt,
    locales,
  };
}

export async function loadState(): Promise<BotState | null> {
  try {
    const raw = await readFile(STATE_FILE, 'utf8');
    const parsed = JSON.parse(raw) as BotState;
    // Defensive: if any locale entry is missing (e.g. user added a locale),
    // backfill empty state for it.
    for (const l of TARGET_LOCALES) {
      if (!parsed.locales[l]) parsed.locales[l] = emptyLocaleState();
    }
    return parsed;
  } catch (err) {
    if ((err as NodeJS.ErrnoException).code === 'ENOENT') return null;
    throw err;
  }
}

export async function saveState(state: BotState): Promise<void> {
  state.lastUpdatedAt = new Date().toISOString();
  await mkdir(STATE_DIR, { recursive: true });
  const tmp = path.join(STATE_DIR, '.bot-state.tmp.json');
  await writeFile(tmp, JSON.stringify(state, null, 2), 'utf8');
  await rename(tmp, STATE_FILE);
}

export function markCompleted(
  state: BotState,
  locale: TargetLocale,
  key: string,
  elapsedMs: number,
  hadWarning: boolean,
): void {
  const ls = state.locales[locale];
  ls.completed++;
  ls.completedKeys.push(key);
  ls.lastKey = key;
  ls.elapsedMs += elapsedMs;
  if (hadWarning) ls.warnings++;
}

export function isKeyCompleted(state: BotState, locale: TargetLocale, key: string): boolean {
  // O(N) scan; with ~1800 keys per locale this is ~7MB of comparisons
  // per skip-check, fine for our throughput. If we ever need <1ms,
  // swap to Set built once at run start.
  return state.locales[locale].completedKeys.includes(key);
}
