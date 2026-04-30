#!/usr/bin/env node
/**
 * Merge translation-bot drafts into the per-namespace locale catalogs.
 *
 * Reads `scripts/translation-bot/output/drafts/<lang>.draft.json` (flat
 * keys: `namespace.full.path`), splits the namespace prefix off, and
 * writes nested JSON into `src/locales/<lang>/<namespace>.json`.
 *
 * Skip rules:
 *   --skip-warnings=<csv>    skip entries with any of these warning codes
 *                            (e.g. `--skip-warnings=GLOSSARY_LOST,EMPTY`).
 *   --only=<lang,lang>       merge only these target locales (default: all).
 *   --dry                    print summary, don't write files.
 *
 * Reports per-locale entry counts + warning histogram. Existing keys in
 * the target catalog are overwritten (re-runs are idempotent under the
 * same draft).
 */
import { readdirSync, readFileSync, writeFileSync, mkdirSync, existsSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(__dirname, "..");
const DRAFTS_DIR = path.join(REPO_ROOT, "scripts", "translation-bot", "output", "drafts");
const LOCALES_DIR = path.join(REPO_ROOT, "src", "locales");
const TARGET_LANGS = ["uk", "ru", "de", "es", "fr", "pl", "pt"];

const args = process.argv.slice(2);
const dry = args.includes("--dry");
const skipWarnings = new Set(
  args
    .find((a) => a.startsWith("--skip-warnings="))
    ?.slice("--skip-warnings=".length)
    .split(",")
    .map((s) => s.trim())
    .filter(Boolean) ?? [],
);
const onlyArg = args.find((a) => a.startsWith("--only="));
const onlyLangs = onlyArg
  ? onlyArg.slice("--only=".length).split(",").map((s) => s.trim()).filter(Boolean)
  : TARGET_LANGS;

function setNested(tree, dottedKey, value) {
  const parts = dottedKey.split(".");
  let cur = tree;
  for (let i = 0; i < parts.length - 1; i++) {
    const k = parts[i];
    if (typeof cur[k] !== "object" || cur[k] === null || Array.isArray(cur[k])) {
      cur[k] = {};
    }
    cur = cur[k];
  }
  cur[parts[parts.length - 1]] = value;
}

let totalProcessed = 0;
let totalSkipped = 0;

for (const lang of onlyLangs) {
  const draftFile = path.join(DRAFTS_DIR, `${lang}.draft.json`);
  if (!existsSync(draftFile)) {
    console.log(`[merge] ${lang}: skip — no draft at ${draftFile}`);
    continue;
  }
  const draft = JSON.parse(readFileSync(draftFile, "utf8"));
  const translations = draft.translations ?? {};

  // Group by namespace (first dotted segment) and accumulate nested trees.
  const byNs = {};
  const warningHist = {};
  let processed = 0;
  let skipped = 0;
  for (const [fullKey, entry] of Object.entries(translations)) {
    const text = entry.translation ?? "";
    const warnings = entry.warnings ?? [];
    if (warnings.some((w) => skipWarnings.has(w))) {
      skipped++;
      for (const w of warnings) warningHist[w] = (warningHist[w] ?? 0) + 1;
      continue;
    }
    if (!text || !text.trim()) {
      skipped++;
      warningHist["EMPTY_AT_MERGE"] = (warningHist["EMPTY_AT_MERGE"] ?? 0) + 1;
      continue;
    }
    const dotIdx = fullKey.indexOf(".");
    if (dotIdx <= 0) {
      console.warn(`[merge] ${lang}: skip key without namespace: ${fullKey}`);
      skipped++;
      continue;
    }
    const ns = fullKey.slice(0, dotIdx);
    const subKey = fullKey.slice(dotIdx + 1);
    if (!byNs[ns]) byNs[ns] = {};
    setNested(byNs[ns], subKey, text);
    processed++;
    for (const w of warnings) warningHist[w] = (warningHist[w] ?? 0) + 1;
  }

  totalProcessed += processed;
  totalSkipped += skipped;

  const histStr = Object.entries(warningHist)
    .sort((a, b) => b[1] - a[1])
    .map(([k, v]) => `${k}=${v}`)
    .join(", ") || "(none)";
  console.log(`[merge] ${lang}: ${processed} merged, ${skipped} skipped — warnings: ${histStr}`);

  if (dry) continue;

  const langDir = path.join(LOCALES_DIR, lang);
  if (!existsSync(langDir)) mkdirSync(langDir, { recursive: true });
  for (const [ns, tree] of Object.entries(byNs)) {
    const outFile = path.join(langDir, `${ns}.json`);
    // Pretty print — same shape as src/locales/en/*.json so diffs review well.
    writeFileSync(outFile, JSON.stringify(tree, null, 2) + "\n", "utf8");
  }
}

console.log(`\n[merge] DONE — ${totalProcessed} entries written, ${totalSkipped} skipped${dry ? " (dry-run)" : ""}`);
