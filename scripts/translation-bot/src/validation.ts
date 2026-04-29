/**
 * Per-translation validation — surfaces likely-bad outputs as warnings
 * for human review (NOT auto-rejection). All warnings are advisory.
 *
 * Validation contract:
 *   - GLOSSARY_LOST       — protected term in EN missing from translation
 *   - PLACEHOLDER_DRIFT   — {var} count or names changed
 *   - LENGTH_OUT_OF_BAND  — translation < 50% or > 200% of EN length
 *   - MODEL_PREFIX        — model emitted "Here's the translation:" etc.
 *   - EMPTY               — translation is empty or whitespace
 *   - CHECK_REQUESTED     — model self-flagged with "⚠️CHECK"
 */
import { BRAND_GLOSSARY } from './config.js';
import type { WarningCode } from './output.js';

const PLACEHOLDER_RE = /\{[a-zA-Z_][a-zA-Z0-9_]*\}/g;

export interface ValidationResult {
  warnings: WarningCode[];
}

export function validate(opts: {
  enValue: string;
  translation: string;
  modelStripped: boolean;
}): ValidationResult {
  const { enValue, translation, modelStripped } = opts;
  const warnings: WarningCode[] = [];

  if (translation.trim().length === 0) {
    warnings.push('EMPTY');
    return { warnings };
  }

  if (modelStripped) warnings.push('MODEL_PREFIX');

  // Self-flagged ambiguous translation (per prompt rule #6).
  if (/⚠️\s*CHECK/i.test(translation) || /⚠CHECK/.test(translation)) {
    warnings.push('CHECK_REQUESTED');
  }

  // Glossary preservation — case-insensitive presence check.
  const enLower = enValue.toLowerCase();
  const trLower = translation.toLowerCase();
  for (const term of BRAND_GLOSSARY) {
    const t = term.toLowerCase();
    if (enLower.includes(t) && !trLower.includes(t)) {
      warnings.push('GLOSSARY_LOST');
      break;
    }
  }

  // Placeholder drift — count + name set must match.
  const enPlaceholders = (enValue.match(PLACEHOLDER_RE) ?? []).map((p) => p);
  const trPlaceholders = (translation.match(PLACEHOLDER_RE) ?? []).map((p) => p);
  if (enPlaceholders.length !== trPlaceholders.length) {
    warnings.push('PLACEHOLDER_DRIFT');
  } else {
    const enSet = new Set(enPlaceholders);
    const trSet = new Set(trPlaceholders);
    if (enSet.size !== trSet.size || ![...enSet].every((p) => trSet.has(p))) {
      warnings.push('PLACEHOLDER_DRIFT');
    }
  }

  // Length band check — lenient (50%-200%). Tighter would false-positive on
  // German compound words and Russian/Ukrainian Cyrillic expansion.
  const enLen = enValue.length;
  const trLen = translation.length;
  if (enLen >= 8) {
    // Skip very short EN — "Yes" → "Ja" trips ratio but is correct.
    const ratio = trLen / enLen;
    if (ratio < 0.5 || ratio > 2.0) warnings.push('LENGTH_OUT_OF_BAND');
  }

  return { warnings };
}
