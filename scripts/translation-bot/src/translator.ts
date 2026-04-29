/**
 * Main per-locale translation loop.
 *
 * Orchestrates audit → state → ollama → validate → output → state-flush.
 * Resumable: if state.completedKeys already contains a key, skip it.
 *
 * Auto-pause: if recent rolling avg > SLOW_AVG_THRESHOLD_MS, pause for
 * AUTO_PAUSE_MS to let GPU thermals / OS scheduler recover. Common on
 * RTX 3060 Ti when other apps reclaim VRAM mid-run.
 */
import {
  AUTO_PAUSE_MS,
  OLLAMA_MODEL,
  SLOW_AVG_THRESHOLD_MS,
  STATE_FLUSH_EVERY,
  TARGET_LOCALES,
  THROTTLE_MS,
  type TargetLocale,
} from './config.js';
import { getUntranslated, loadAudit, type AuditEntry } from './audit.js';
import { generate, sleep } from './ollama.js';
import { buildPrompt, cleanResponse } from './prompt.js';
import {
  reportLocaleEnd,
  reportLocaleStart,
  reportProgress,
} from './progress.js';
import {
  addEntry,
  emptyDraft,
  loadDraft,
  saveDraft,
  type DraftEntry,
  type DraftFile,
} from './output.js';
import {
  emptyState,
  isKeyCompleted,
  loadState,
  markCompleted,
  saveState,
  type BotState,
} from './state.js';
import { validate } from './validation.js';

interface RunOptions {
  /** Limit number of strings translated per locale (smoke / dry-run). */
  limit?: number;
  /** Locales to run; defaults to all configured TARGET_LOCALES. */
  locales?: TargetLocale[];
  /** Hook fired when SIGINT requested — allows main() to print summary. */
  onShutdown?: (state: BotState) => void;
}

export async function runTranslation(options: RunOptions = {}): Promise<BotState> {
  const audit = await loadAudit();
  const targetLocales =
    (options.locales ?? []).length > 0
      ? options.locales!
      : ([...TARGET_LOCALES] as TargetLocale[]);

  let state = await loadState();
  if (!state) {
    state = emptyState(OLLAMA_MODEL, audit.generatedAt);
  } else if (state.auditGeneratedAt !== audit.generatedAt) {
    // eslint-disable-next-line no-console
    console.warn(
      `[bot] state was generated against audit ${state.auditGeneratedAt}, ` +
        `but current audit is ${audit.generatedAt}. Resuming anyway — completed keys ` +
        `will be skipped if they still exist in the new audit.`,
    );
  }

  // Wire SIGINT/SIGTERM for graceful shutdown — the loop checks `interrupted`
  // between strings; the latest state is saved before exit.
  let interrupted = false;
  const onSig = () => {
    interrupted = true;
    // eslint-disable-next-line no-console
    console.log('\n[bot] interrupt received — finishing current string then exiting…');
  };
  process.once('SIGINT', onSig);
  process.once('SIGTERM', onSig);

  try {
    for (const locale of targetLocales) {
      if (interrupted) break;

      const entries = await getUntranslated(locale);
      const ls = state.locales[locale];
      ls.total = entries.length;

      // Build skip-set from prior progress (faster than scanning the array per key).
      const completedSet = new Set(ls.completedKeys);
      const remainingEntries = entries.filter((e) => !completedSet.has(e.key));

      // Apply per-locale limit (smoke mode).
      const queue =
        options.limit && options.limit > 0
          ? remainingEntries.slice(0, options.limit)
          : remainingEntries;

      reportLocaleStart(locale, ls.total, ls.completed);

      let draft = (await loadDraft(locale)) ?? emptyDraft(locale, ls.total);
      // If the audit grew, update totalStrings on the draft.
      draft.totalStrings = ls.total;

      const recentLatencies: number[] = [];
      for (let i = 0; i < queue.length; i++) {
        if (interrupted) break;
        const entry = queue[i]!;
        const t = await translateOne(locale, entry, draft, state);
        recentLatencies.push(t.elapsedMs);
        if (recentLatencies.length > 10) recentLatencies.shift();

        // State + draft flush every N strings (atomic; crash-loss bound).
        if ((i + 1) % STATE_FLUSH_EVERY === 0) {
          await saveDraft(draft);
          await saveState(state);
          reportProgress(state, locale, t.elapsedMs);
        }

        // Auto-pause if rolling avg over last 10 calls is bad.
        const rollingAvg =
          recentLatencies.reduce((s, n) => s + n, 0) / recentLatencies.length;
        if (
          recentLatencies.length >= 10 &&
          rollingAvg > SLOW_AVG_THRESHOLD_MS
        ) {
          // eslint-disable-next-line no-console
          console.warn(
            `[bot] rolling avg ${(rollingAvg / 1000).toFixed(1)}s > ${(SLOW_AVG_THRESHOLD_MS / 1000).toFixed(0)}s threshold — auto-pausing ${(AUTO_PAUSE_MS / 1000).toFixed(0)}s…`,
          );
          await sleep(AUTO_PAUSE_MS);
          recentLatencies.length = 0; // reset window after pause
        } else if (THROTTLE_MS > 0) {
          await sleep(THROTTLE_MS);
        }
      }

      // Final flush per locale.
      await saveDraft(draft);
      await saveState(state);
      if (!interrupted) reportLocaleEnd(state, locale);
    }
  } finally {
    process.off('SIGINT', onSig);
    process.off('SIGTERM', onSig);
    // Final state save in case the catch path is taken.
    await saveState(state);
    options.onShutdown?.(state);
  }

  return state;
}

async function translateOne(
  locale: TargetLocale,
  entry: AuditEntry,
  draft: DraftFile,
  state: BotState,
): Promise<{ elapsedMs: number; warnings: number }> {
  const prompt = buildPrompt({ locale, key: entry.key, enValue: entry.value });
  const result = await generate(prompt);
  const { clean, stripped } = cleanResponse(result.response);
  const validation = validate({
    enValue: entry.value,
    translation: clean,
    modelStripped: stripped,
  });

  const draftEntry: DraftEntry = {
    en: entry.value,
    translation: clean,
    elapsedMs: result.elapsedMs,
    attempts: result.attempts,
    warnings: validation.warnings,
    model: OLLAMA_MODEL,
    generatedAt: new Date().toISOString(),
  };

  addEntry(draft, entry.key, draftEntry);
  markCompleted(state, locale, entry.key, result.elapsedMs, validation.warnings.length > 0);

  if (isKeyCompleted(state, locale, entry.key)) {
    // sanity tick to keep TS strict happy about unused import
  }

  return { elapsedMs: result.elapsedMs, warnings: validation.warnings.length };
}
