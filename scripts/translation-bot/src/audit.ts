/**
 * In-memory audit synthesizer for v2t.
 *
 * Walks `src/locales/en/*.json`, flattens each catalog to dotted keys
 * prefixed with the namespace (`onboarding.welcome.intro`, etc.) and pairs
 * those against any pre-existing values in `src/locales/{lang}/*.json`.
 * Returns the set of {key, enValue} entries still missing per target
 * locale — same shape `audit.ts` exported in NumbersM, so translator.ts
 * stays untouched.
 *
 * `generatedAt` is the latest mtime across the EN files. State.json
 * compares this on resume; if EN was edited mid-run, drift is logged
 * and resume is best-effort (already-completed keys still skip).
 */
import { readdir, readFile, stat } from 'node:fs/promises';
import path from 'node:path';
import { LOCALE_SOURCE_DIR, LOCALE_TARGET_DIR, TARGET_LOCALES, type TargetLocale } from './config.js';

export interface AuditEntry {
  key: string;
  value: string;
}

interface AuditFileShape {
  generatedAt: string;
  baseLocale: string;
  brandAllowlist?: string[];
  locales: Record<string, { untranslatedKeys: AuditEntry[] }>;
}

let cached: AuditFileShape | null = null;

/** `{ "onboarding.welcome.intro": "<strong>Video to Text</strong>…", … }` */
type FlatCatalog = Record<string, string>;

function flatten(prefix: string, src: unknown, out: FlatCatalog): void {
  if (typeof src === 'string') {
    out[prefix] = src;
    return;
  }
  if (typeof src !== 'object' || src === null) return;
  for (const [k, v] of Object.entries(src as Record<string, unknown>)) {
    const next = prefix ? `${prefix}.${k}` : k;
    flatten(next, v, out);
  }
}

async function readJsonIfExists(file: string): Promise<unknown | null> {
  try {
    const raw = await readFile(file, 'utf8');
    return JSON.parse(raw);
  } catch (e) {
    if ((e as NodeJS.ErrnoException).code === 'ENOENT') return null;
    throw e;
  }
}

/** Read every `*.json` in `dir` and return `{ namespace: parsed-json }`. */
async function readNamespaceCatalogs(dir: string): Promise<Record<string, unknown>> {
  let names: string[];
  try {
    names = await readdir(dir);
  } catch (e) {
    if ((e as NodeJS.ErrnoException).code === 'ENOENT') return {};
    throw e;
  }
  const out: Record<string, unknown> = {};
  for (const name of names) {
    if (!name.endsWith('.json')) continue;
    const ns = name.replace(/\.json$/, '');
    const content = await readJsonIfExists(path.join(dir, name));
    if (content !== null) out[ns] = content;
  }
  return out;
}

/** Latest mtime ISO string across all en/*.json — used as audit generation tag. */
async function latestEnMtime(): Promise<string> {
  const names = await readdir(LOCALE_SOURCE_DIR);
  let latest = 0;
  for (const name of names) {
    if (!name.endsWith('.json')) continue;
    const s = await stat(path.join(LOCALE_SOURCE_DIR, name));
    if (s.mtimeMs > latest) latest = s.mtimeMs;
  }
  return new Date(latest).toISOString();
}

export async function loadAudit(): Promise<AuditFileShape> {
  if (cached) return cached;

  const enCatalogs = await readNamespaceCatalogs(LOCALE_SOURCE_DIR);
  const enFlat: FlatCatalog = {};
  for (const [ns, content] of Object.entries(enCatalogs)) {
    flatten(ns, content, enFlat);
  }

  const generatedAt = await latestEnMtime();
  const locales: Record<string, { untranslatedKeys: AuditEntry[] }> = {};

  for (const lang of TARGET_LOCALES) {
    const langDir = path.join(LOCALE_TARGET_DIR, lang);
    const langCatalogs = await readNamespaceCatalogs(langDir);
    const langFlat: FlatCatalog = {};
    for (const [ns, content] of Object.entries(langCatalogs)) {
      flatten(ns, content, langFlat);
    }

    const untranslated: AuditEntry[] = [];
    for (const [k, v] of Object.entries(enFlat)) {
      const existing = langFlat[k];
      // Treat empty string and identical-to-EN as untranslated. Identical
      // text is sometimes intentional for ISO chips (e.g. "ru" → "ru") —
      // the bot still re-translates and the result is just `ru`, no harm.
      if (existing == null || existing === '' || existing === v) {
        untranslated.push({ key: k, value: v });
      }
    }
    locales[lang] = { untranslatedKeys: untranslated };
  }

  cached = {
    generatedAt,
    baseLocale: 'en',
    locales,
  };
  return cached;
}

export async function getUntranslated(locale: TargetLocale): Promise<AuditEntry[]> {
  const audit = await loadAudit();
  const bucket = audit.locales[locale];
  if (!bucket) {
    throw new Error(
      `Locale "${locale}" not found in audit. Available: ${Object.keys(audit.locales).join(', ')}`,
    );
  }
  return bucket.untranslatedKeys;
}

export function auditGeneratedAt(audit: AuditFileShape): string {
  return audit.generatedAt;
}
