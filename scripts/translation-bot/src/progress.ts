/**
 * Console progress reporter.
 *
 * Prints a status line every N completed strings and on locale
 * boundaries, so an overnight run is monitorable via `tail -f` of
 * stdout-redirect or just glancing at the terminal.
 */
import type { BotState } from './state.js';
import type { TargetLocale } from './config.js';

function formatDuration(ms: number): string {
  const sec = Math.round(ms / 1000);
  if (sec < 60) return `${sec}s`;
  const min = Math.floor(sec / 60);
  const remSec = sec % 60;
  if (min < 60) return `${min}m ${remSec}s`;
  const hr = Math.floor(min / 60);
  const remMin = min % 60;
  return `${hr}h ${remMin}m`;
}

export function reportProgress(
  state: BotState,
  locale: TargetLocale,
  recentLatencyMs: number,
): void {
  const ls = state.locales[locale];
  const pct = ls.total > 0 ? ((ls.completed / ls.total) * 100).toFixed(1) : '0.0';
  const avgMs = ls.completed > 0 ? ls.elapsedMs / ls.completed : 0;
  const remaining = Math.max(0, ls.total - ls.completed);
  const etaMs = remaining * avgMs;

  // eslint-disable-next-line no-console
  console.log(
    `[${locale}] ${ls.completed}/${ls.total} (${pct}%) · last ${(recentLatencyMs / 1000).toFixed(1)}s · avg ${(avgMs / 1000).toFixed(1)}s · ETA ${formatDuration(etaMs)} · warnings: ${ls.warnings}`,
  );
}

export function reportLocaleStart(locale: TargetLocale, total: number, alreadyDone: number): void {
  // eslint-disable-next-line no-console
  console.log(
    `\n━━━ ${locale.toUpperCase()} — ${alreadyDone}/${total} done, ${total - alreadyDone} to go ━━━\n`,
  );
}

export function reportLocaleEnd(state: BotState, locale: TargetLocale): void {
  const ls = state.locales[locale];
  // eslint-disable-next-line no-console
  console.log(
    `\n[${locale}] ✅ DONE — ${ls.completed}/${ls.total} translated, ${ls.warnings} warnings, ${formatDuration(ls.elapsedMs)} wall-clock\n`,
  );
}

export function reportRunEnd(state: BotState): void {
  const totalCompleted = Object.values(state.locales).reduce(
    (s, ls) => s + ls.completed,
    0,
  );
  const totalElapsed = Object.values(state.locales).reduce(
    (s, ls) => s + ls.elapsedMs,
    0,
  );
  const totalWarnings = Object.values(state.locales).reduce(
    (s, ls) => s + ls.warnings,
    0,
  );

  // eslint-disable-next-line no-console
  console.log(
    `\n═══ RUN COMPLETE ═══\n` +
      `Translated: ${totalCompleted} strings across ${Object.keys(state.locales).length} locales\n` +
      `Warnings:   ${totalWarnings} (review in output/drafts/)\n` +
      `Elapsed:    ${formatDuration(totalElapsed)}\n` +
      `Output:     scripts/translation-bot/output/drafts/{locale}.draft.json\n` +
      `Next step:  human review → merge approved entries into web/messages/{locale}.json\n`,
  );
}
