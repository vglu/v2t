/**
 * Review-file generator — produces human-review markdown per locale.
 *
 * Output: `output/review/{locale}-review.md` with entries that have at
 * least one warning. Each entry shows EN source + RU anchor (from prod
 * messages or RU draft) + target translation + warning codes + approve/
 * edit/reject affordance.
 *
 * Format optimized for Mercedes-grade manual review by a Russian-
 * speaking founder. RU column lets the reviewer see "what we meant"
 * before approving the new locale rendering.
 */
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import path from 'node:path';
import {
  BOT_ROOT,
  DRAFTS_DIR,
  REPO_ROOT,
  type TargetLocale,
} from './config.js';
import type { DraftEntry, DraftFile } from './output.js';

interface ReviewOptions {
  /** Target locale to review (de or uk currently). */
  locale: TargetLocale;
  /** Optional cap on entries (for sample preview). */
  limit?: number;
}

const REVIEW_DIR = path.join(BOT_ROOT, 'output', 'review');

const WARNING_GLYPH: Record<string, string> = {
  PLACEHOLDER_DRIFT: '⚠️ placeholder',
  LENGTH_OUT_OF_BAND: '📏 length',
  GLOSSARY_LOST: '🏷 glossary',
  MODEL_PREFIX: '🤖 prefix',
  EMPTY: '🚫 empty',
  CHECK_REQUESTED: '❓ check',
};

const WARNING_ORDER = [
  'PLACEHOLDER_DRIFT',
  'GLOSSARY_LOST',
  'MODEL_PREFIX',
  'EMPTY',
  'LENGTH_OUT_OF_BAND',
  'CHECK_REQUESTED',
];

interface FlattenedKv {
  [key: string]: string;
}

function flatten(obj: unknown, prefix = '', acc: FlattenedKv = {}): FlattenedKv {
  if (obj == null) return acc;
  if (typeof obj === 'string') {
    if (prefix) acc[prefix] = obj;
    return acc;
  }
  if (typeof obj === 'object') {
    for (const [k, v] of Object.entries(obj as Record<string, unknown>)) {
      const next = prefix ? `${prefix}.${k}` : k;
      flatten(v, next, acc);
    }
  }
  return acc;
}

async function loadProdMessages(locale: string): Promise<FlattenedKv> {
  try {
    const raw = await readFile(
      path.join(REPO_ROOT, 'web', 'messages', `${locale}.json`),
      'utf8',
    );
    return flatten(JSON.parse(raw));
  } catch {
    return {};
  }
}

async function loadDraftKv(locale: TargetLocale): Promise<DraftFile | null> {
  try {
    const raw = await readFile(path.join(DRAFTS_DIR, `${locale}.draft.json`), 'utf8');
    return JSON.parse(raw) as DraftFile;
  } catch {
    return null;
  }
}

interface RuAnchor {
  value: string;
  /** Source: prod messages.json or draft (✱ marker). */
  source: 'prod' | 'draft' | 'missing';
}

function getRuAnchor(
  key: string,
  prodRu: FlattenedKv,
  draftRu: DraftFile | null,
): RuAnchor {
  const fromProd = prodRu[key];
  if (typeof fromProd === 'string' && fromProd.length > 0) {
    return { value: fromProd, source: 'prod' };
  }
  const fromDraft = draftRu?.translations[key];
  if (fromDraft) {
    return { value: fromDraft.translation, source: 'draft' };
  }
  return { value: '(no RU anchor)', source: 'missing' };
}

function escapeMd(text: string): string {
  // Backticks become guarded — we wrap in backticks in the table.
  // Pipes need escaping inside markdown tables.
  return text.replace(/\|/g, '\\|').replace(/\n/g, ' ⏎ ');
}

function localeLabel(locale: string): string {
  return (
    {
      en: 'EN',
      ru: 'RU',
      uk: 'UK',
      de: 'DE',
      es: 'ES',
      fr: 'FR',
      pt: 'PT',
    }[locale] ?? locale.toUpperCase()
  );
}

export async function generateReview(options: ReviewOptions): Promise<string> {
  const { locale, limit } = options;

  const draft = await loadDraftKv(locale);
  if (!draft) {
    throw new Error(`No draft for ${locale}. Run translator first.`);
  }

  const prodRu = await loadProdMessages('ru');
  const draftRu = locale === 'ru' ? null : await loadDraftKv('ru');

  const flagged: Array<{ key: string; entry: DraftEntry; firstWarning: string }> = [];
  for (const [key, entry] of Object.entries(draft.translations)) {
    if (entry.warnings.length === 0) continue;
    const firstWarning =
      WARNING_ORDER.find((w) => entry.warnings.includes(w as never)) ??
      entry.warnings[0]!;
    flagged.push({ key, entry, firstWarning });
  }

  flagged.sort((a, b) => {
    const ai = WARNING_ORDER.indexOf(a.firstWarning);
    const bi = WARNING_ORDER.indexOf(b.firstWarning);
    if (ai !== bi) return ai - bi;
    return a.key.localeCompare(b.key);
  });

  const cappedFlagged = limit && limit > 0 ? flagged.slice(0, limit) : flagged;

  const totalEntries = Object.keys(draft.translations).length;
  const totalFlagged = flagged.length;
  const byCode = new Map<string, number>();
  for (const f of flagged) {
    for (const w of f.entry.warnings) byCode.set(w, (byCode.get(w) ?? 0) + 1);
  }

  const lines: string[] = [];
  lines.push(`# ${localeLabel(locale)} Translation Review — ${draft.generatedAt.slice(0, 10)}`);
  lines.push('');
  lines.push(
    `**${totalFlagged} entries flagged** (out of ${totalEntries} translated). Source: \`output/drafts/${locale}.draft.json\` · model \`${draft.model}\`.`,
  );
  if (limit) {
    lines.push(`**SAMPLE — showing first ${cappedFlagged.length}** (full file would have ${totalFlagged}).`);
  }
  lines.push('');
  lines.push(`**RU anchor source:** prod \`web/messages/ru.json\` if available; else RU draft (marked ✱). Lets you read the meaning the team already shipped before evaluating ${localeLabel(locale)}.`);
  lines.push('');
  lines.push('## Warning codes');
  lines.push('');
  for (const code of WARNING_ORDER) {
    const count = byCode.get(code) ?? 0;
    if (count === 0) continue;
    const glyph = WARNING_GLYPH[code] ?? code;
    lines.push(`- ${glyph} **${code}** — ${count} entries`);
  }
  lines.push('');
  lines.push(
    '## How to mark each entry',
    '',
    'Edit the `[ ]` checkbox to `[x]` next to your decision:',
    '- `[x] keep` — translation is fine as-is',
    '- `[x] edit` — replace the translation; write your fix on the `Fix:` line',
    '- `[x] reject` — drop this entry; we leave the EN fallback or re-translate later',
    '',
    'After review the merge script will read the `[x]` marks + `Fix:` lines and apply them to `web/messages/' + locale + '.json`.',
    '',
    '---',
    '',
  );

  // Render per warning group with H2 headers.
  let lastGroup = '';
  for (const { key, entry, firstWarning } of cappedFlagged) {
    if (firstWarning !== lastGroup) {
      const glyph = WARNING_GLYPH[firstWarning] ?? firstWarning;
      const groupCount = flagged.filter((f) => f.firstWarning === firstWarning).length;
      lines.push(`## ${glyph} — ${firstWarning} (${groupCount})`);
      lines.push('');
      lastGroup = firstWarning;
    }

    const ru = getRuAnchor(key, prodRu, draftRu);
    const ruMarker = ru.source === 'draft' ? ' ✱' : ru.source === 'missing' ? ' ⛔' : '';
    const warningGlyphs = entry.warnings
      .map((w) => WARNING_GLYPH[w] ?? w)
      .join(' · ');

    lines.push(`### \`${key}\``);
    lines.push(`*Warnings: ${warningGlyphs}*`);
    lines.push('');
    lines.push('| | |');
    lines.push('|---|---|');
    lines.push(`| **EN** | \`${escapeMd(entry.en)}\` |`);
    lines.push(`| **RU**${ruMarker} | \`${escapeMd(ru.value)}\` |`);
    lines.push(`| **${localeLabel(locale)}** | \`${escapeMd(entry.translation)}\` |`);
    lines.push('');
    lines.push('`[ ] keep   [ ] edit   [ ] reject`');
    lines.push('');
    lines.push('Fix: ');
    lines.push('');
    lines.push('---');
    lines.push('');
  }

  await mkdir(REVIEW_DIR, { recursive: true });
  const outPath = path.join(
    REVIEW_DIR,
    limit ? `${locale}-review-sample.md` : `${locale}-review.md`,
  );
  await writeFile(outPath, lines.join('\n'), 'utf8');
  return outPath;
}
