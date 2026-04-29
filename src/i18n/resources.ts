/**
 * i18n resource bundler.
 *
 * Vite glob-imports all `src/locales/<lang>/<namespace>.json` at build time
 * and shapes them into the `{ lang: { namespace: keys… } }` structure that
 * `i18next.init({ resources })` expects.
 *
 * **Why glob, not explicit imports?** When M3 lands the bot's drafts, new
 * `src/locales/{ru,de,…}/*.json` files appear without code changes; glob
 * picks them up automatically. Explicit imports would bottleneck every new
 * locale on a hand-edit of this file.
 *
 * **Empty placeholder JSONs are fine.** During M1 only `en/*.json` exist
 * (and they're `{}` until M3a extracts strings). i18next falls back to the
 * key itself when the value is missing, so the UI continues to render its
 * current hard-coded English while the catalogs are being filled — no
 * regression.
 */

type Catalog = Record<string, unknown>;
type Resources = Record<string, Record<string, Catalog>>;

const modules = import.meta.glob<Catalog>("../locales/*/*.json", {
  eager: true,
  import: "default",
});

function buildResources(): Resources {
  const out: Resources = {};
  for (const [path, content] of Object.entries(modules)) {
    const m = path.match(/locales\/([a-z]+)\/([a-z]+)\.json$/);
    if (!m) continue;
    const lang = m[1]!;
    const ns = m[2]!;
    if (!out[lang]) out[lang] = {};
    out[lang][ns] = content;
  }
  return out;
}

export const resources: Resources = buildResources();

/** All language codes with at least one namespace file present. */
export const detectedLanguages: string[] = Object.keys(resources);
