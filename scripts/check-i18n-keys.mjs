#!/usr/bin/env node
/**
 * CI gate for i18n key parity.
 *
 * Walks `src/locales/en/<namespace>.json` (the source of truth) and
 * verifies every flat key exists in each `src/locales/<lang>/<namespace>.json`.
 * Empty-string values count as missing — translation drafts that the
 * bot left blank shouldn't pretend to be done.
 *
 * Exit codes:
 *   0 — all target locales fully translated.
 *   1 — at least one missing key (printed to stderr). Build should fail.
 *
 * Per-locale partial coverage (M3d in-progress) is reported as a warning
 * but only blocks if `CHECK_I18N_STRICT=1` (set in production builds).
 *
 * Usage:
 *   node scripts/check-i18n-keys.mjs            # report-only
 *   CHECK_I18N_STRICT=1 node scripts/check-i18n-keys.mjs   # fail on miss
 */
import { readdirSync, readFileSync, existsSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(__dirname, "..");
const LOCALES_DIR = path.join(REPO_ROOT, "src", "locales");
const SOURCE_LANG = "en";
const TARGET_LANGS = ["uk", "ru", "de", "es", "fr", "pl", "pt"];

const STRICT = process.env.CHECK_I18N_STRICT === "1";

/** Recursively flatten a JSON tree into `{ "a.b.c": "value", ... }`. */
function flatten(obj, prefix = "", out = {}) {
  if (typeof obj === "string") {
    out[prefix] = obj;
    return out;
  }
  if (typeof obj !== "object" || obj === null) return out;
  for (const [k, v] of Object.entries(obj)) {
    flatten(v, prefix ? `${prefix}.${k}` : k, out);
  }
  return out;
}

function readNamespaceFiles(lang) {
  const dir = path.join(LOCALES_DIR, lang);
  if (!existsSync(dir)) return {};
  const files = readdirSync(dir).filter((n) => n.endsWith(".json"));
  const out = {};
  for (const file of files) {
    const ns = file.replace(/\.json$/, "");
    try {
      const raw = readFileSync(path.join(dir, file), "utf8");
      out[ns] = flatten(JSON.parse(raw), ns);
    } catch (e) {
      console.error(`[check-i18n] ${lang}/${file}: parse error — ${e.message}`);
      process.exitCode = 1;
      out[ns] = {};
    }
  }
  return out;
}

function mergeNs(byNs) {
  const out = {};
  for (const ns of Object.keys(byNs)) Object.assign(out, byNs[ns]);
  return out;
}

const enByNs = readNamespaceFiles(SOURCE_LANG);
const enFlat = mergeNs(enByNs);
const totalKeys = Object.keys(enFlat).length;
console.log(`[check-i18n] source ${SOURCE_LANG}: ${totalKeys} keys across ${Object.keys(enByNs).length} namespaces`);

let totalMissing = 0;
for (const lang of TARGET_LANGS) {
  const langFlat = mergeNs(readNamespaceFiles(lang));
  const missing = [];
  const empty = [];
  for (const [k, v] of Object.entries(enFlat)) {
    const tr = langFlat[k];
    if (tr == null) missing.push(k);
    else if (tr === "") empty.push(k);
  }
  const total = missing.length + empty.length;
  totalMissing += total;
  if (total === 0) {
    console.log(`[check-i18n] ${lang}: ✓ ${totalKeys}/${totalKeys}`);
  } else {
    const pct = (((totalKeys - total) / totalKeys) * 100).toFixed(1);
    console.log(
      `[check-i18n] ${lang}: ${totalKeys - total}/${totalKeys} (${pct}%) — ${missing.length} missing, ${empty.length} empty`,
    );
    // Print first few keys for visibility — full list would flood the terminal.
    const sample = [...missing, ...empty.map((k) => `${k} (empty)`)].slice(0, 5);
    for (const k of sample) console.log(`  · ${k}`);
    if (total > 5) console.log(`  · … and ${total - 5} more`);
  }
}

if (totalMissing === 0) {
  console.log(`[check-i18n] ✓ all ${TARGET_LANGS.length} target locales fully translated`);
  process.exit(0);
}

if (STRICT) {
  console.error(`[check-i18n] ✗ ${totalMissing} keys missing across target locales (STRICT mode)`);
  process.exit(1);
}

console.log(
  `[check-i18n] ⚠ ${totalMissing} keys missing across target locales (advisory; set CHECK_I18N_STRICT=1 to fail builds)`,
);
process.exit(0);
