/**
 * Draft writer — atomic per-locale output of `output/drafts/{locale}.draft.json`.
 *
 * Each entry: { en, translation, elapsedMs, warning, attempts, model }.
 * Format is intentionally NOT a drop-in replacement for `web/messages/*.json`
 * — human review extracts approved entries and merges them into the
 * production messages tree. This separation is the Mercedes guard
 * against accidental machine-translate-and-ship (CLAUDE.md §12).
 */
import { mkdir, readFile, rename, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { DRAFTS_DIR, OLLAMA_MODEL, type TargetLocale } from './config.js';

export type WarningCode =
  | 'GLOSSARY_LOST'
  | 'PLACEHOLDER_DRIFT'
  | 'LENGTH_OUT_OF_BAND'
  | 'MODEL_PREFIX'
  | 'EMPTY'
  | 'CHECK_REQUESTED';

export interface DraftEntry {
  en: string;
  translation: string;
  elapsedMs: number;
  attempts: number;
  warnings: WarningCode[];
  model: string;
  generatedAt: string;
}

export interface DraftFile {
  version: 1;
  locale: TargetLocale;
  model: string;
  generatedAt: string;
  totalStrings: number;
  /** Map of full dotted key → entry. */
  translations: Record<string, DraftEntry>;
}

function draftPath(locale: TargetLocale): string {
  return path.join(DRAFTS_DIR, `${locale}.draft.json`);
}

export async function loadDraft(locale: TargetLocale): Promise<DraftFile | null> {
  try {
    const raw = await readFile(draftPath(locale), 'utf8');
    return JSON.parse(raw) as DraftFile;
  } catch (err) {
    if ((err as NodeJS.ErrnoException).code === 'ENOENT') return null;
    throw err;
  }
}

export async function saveDraft(file: DraftFile): Promise<void> {
  await mkdir(DRAFTS_DIR, { recursive: true });
  const tmp = path.join(DRAFTS_DIR, `.${file.locale}.draft.tmp.json`);
  await writeFile(tmp, JSON.stringify(file, null, 2), 'utf8');
  await rename(tmp, draftPath(file.locale));
}

export function emptyDraft(locale: TargetLocale, totalStrings: number): DraftFile {
  return {
    version: 1,
    locale,
    model: OLLAMA_MODEL,
    generatedAt: new Date().toISOString(),
    totalStrings,
    translations: {},
  };
}

/** Add or replace a translation entry. */
export function addEntry(file: DraftFile, key: string, entry: DraftEntry): void {
  file.translations[key] = entry;
  file.generatedAt = new Date().toISOString();
}
