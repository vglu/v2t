/**
 * Prompt builder for v2t — adapted from NumbersM.
 *
 * Differences from NumbersM:
 * - App context: v2t (Tauri desktop transcriber), not NumbersM (wellness mobile).
 * - Audience: technically literate users — content creators, researchers,
 *   students. Register: neutral-friendly, technically precise.
 * - Glossary: read from BRAND_GLOSSARY (v2t list — yt-dlp, ffmpeg, Whisper,
 *   CUDA, file extensions, …) and inlined into hard rule #2.
 * - Hard rule #4 expanded to preserve inline HTML tags (`<strong>`, `<code>`,
 *   `<em>`, `<a>`) which v2t catalogs use so React renders them via
 *   <Trans components={...}>.
 * - i18next double-curly placeholders: `{{count}}`, `{{path}}`, etc. (vs.
 *   single-curly in NumbersM).
 */
import { BRAND_GLOSSARY, LOCALE_LABELS, NAMESPACE_HINTS, type TargetLocale } from './config.js';

export interface PromptParams {
  locale: TargetLocale;
  /** Full dotted i18n key — `onboarding.welcome.intro`, etc. */
  key: string;
  /** EN source value. */
  enValue: string;
}

const GLOSSARY_LIST = BRAND_GLOSSARY.map((t) => `"${t}"`).join(', ');

const PROMPT_TEMPLATE = `You are a professional UI/UX translator for v2t — a free open-source desktop app (Tauri + React) that converts video and audio files / URLs into text using ffmpeg, yt-dlp, and Whisper. Your job: translate one short UI string from English to {target_label}.

Context:
- i18n namespace: "{namespace}" (this string lives on the {namespace_hint} surface of the app)
- Full key: {full_key}
- Inferred string kind: {inferred_kind}
- Target audience: technically literate users — content creators, journalists, researchers, students — who already know what an "API key", "ffmpeg", or "subtitles" is. Native {target_short} speakers expecting natural, clear desktop-app copy.

Hard rules:
1. Use a neutral-friendly, technically precise register in {target_label}. Clear, concise, formal-you ("Sie" / "vous" / "Ви" / "Вы" / "Pan/Pani"). Not legal-stiff, not chatty.
2. NEVER translate these terms — keep them verbatim, exact case: {glossary}.
3. NEVER translate technical identifiers: file paths, URLs, code blocks inside <code>...</code>, ISO 639-1 codes (en/uk/ru/de/es/fr/pl/pt), command-line arguments (--cookies-from-browser, etc.), filenames.
4. PRESERVE EXACTLY:
   - i18next placeholders like {{count}}, {{path}}, {{label}}, {{n}}, {{title}} — keep both braces and the name.
   - Inline HTML tags: <strong>...</strong>, <code>...</code>, <em>...</em>, <a>...</a>, <li>...</li>, <ul>...</ul>. Translate the text inside but keep the tag wrappers exact.
   - Line breaks and emoji.
   - Ellipsis dots, em-dashes, arrows (→, ▾, ▴, ↻, ✓, ✗, ⏸, ▶, ⏭, 📝, 🌐, …).
5. Length within ±50% of the original (UI elements have layout constraints).
6. If the English string is ambiguous WITHOUT context, output the most likely translation followed by " ⚠️CHECK".
7. Output ONLY the translation. No quotes, no preamble, no explanation, no "Here's the translation:". Just the translated string.

English source: {en_value}

{target_label} translation:`;

export function inferKind(key: string, value: string): string {
  const last = (key.split('.').pop() ?? '').toLowerCase();
  const len = value.length;

  if (/title|heading|header/.test(last)) return 'panel or section title';
  if (/btn|button|cta|action/.test(last)) return 'button label';
  if (/hint|help|tooltip|description|note|subtitle|tip|intro|body/.test(last))
    return 'helper / description text';
  if (/error|err|warning/.test(last)) return 'error or warning message';
  if (/placeholder|ph_/.test(last)) return 'input placeholder';
  if (/label/.test(last)) return 'form field label';
  if (/aria/.test(last)) return 'screen-reader aria-label (no markup)';
  if (/option/.test(last)) return 'select dropdown option label';
  if (/success/.test(last)) return 'success banner / confirmation';
  if (/msg|message/.test(last)) return 'log line or status message';

  if (len < 16) return 'short label or button';
  if (len < 60) return 'medium-length label or heading';
  return 'longer descriptive paragraph';
}

export function namespaceHint(namespace: string): string {
  return NAMESPACE_HINTS[namespace] ?? namespace;
}

export function buildPrompt(params: PromptParams): string {
  const { locale, key, enValue } = params;
  const namespace = key.split('.', 1)[0] ?? key;

  return PROMPT_TEMPLATE.replace(/\{target_label\}/g, LOCALE_LABELS[locale])
    .replace(/\{target_short\}/g, locale)
    .replace('{namespace}', namespace)
    .replace('{namespace_hint}', namespaceHint(namespace))
    .replace('{full_key}', key)
    .replace('{inferred_kind}', inferKind(key, enValue))
    .replace('{glossary}', GLOSSARY_LIST)
    .replace('{en_value}', enValue);
}

/**
 * Strip common model artifacts that sneak past hard-rule #7.
 * Returns the cleaned string + whether anything was stripped (signal
 * for `MODEL_PREFIX` warning).
 */
export function cleanResponse(raw: string): { clean: string; stripped: boolean } {
  let clean = raw.trim();
  let stripped = false;

  // Surrounding quotes (single, double, smart).
  const quotePairs: Array<[string, string]> = [
    ['"', '"'],
    ["'", "'"],
    ['«', '»'],
    ['“', '”'],
    ['‘', '’'],
    ['„', '“'],
  ];
  for (const [open, close] of quotePairs) {
    if (clean.startsWith(open) && clean.endsWith(close) && clean.length >= 2) {
      clean = clean.slice(open.length, clean.length - close.length).trim();
      stripped = true;
    }
  }

  // Common preambles model may emit despite rule #7.
  const preambles = [
    /^here'?s? the translation[:.]?\s*/i,
    /^translation[:.]?\s*/i,
    /^die übersetzung[:.]?\s*/i,
    /^la traducción[:.]?\s*/i,
    /^la traduction[:.]?\s*/i,
    /^a tradução[:.]?\s*/i,
    /^переклад[:.]?\s*/i,
    /^перевод[:.]?\s*/i,
  ];
  for (const re of preambles) {
    if (re.test(clean)) {
      clean = clean.replace(re, '').trim();
      stripped = true;
    }
  }

  return { clean, stripped };
}
