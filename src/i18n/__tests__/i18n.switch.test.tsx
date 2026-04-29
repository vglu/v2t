import { beforeAll, describe, expect, it } from "vitest";
import i18next from "i18next";
import "../index";

describe("i18next runtime", () => {
  beforeAll(async () => {
    if (!i18next.isInitialized) {
      await new Promise<void>((resolve) => {
        i18next.on("initialized", () => resolve());
      });
    }
  });

  it("registers the supported locales as resource buckets", () => {
    // Empty placeholders are still bound — i18next won't error when callers
    // ask for a `t("key")` that doesn't exist; it falls back to the key.
    const resources = i18next.options.resources ?? {};
    expect(Object.keys(resources)).toEqual(expect.arrayContaining(["en"]));
    expect(i18next.options.fallbackLng).toEqual(expect.arrayContaining(["en"]));
  });

  it("changeLanguage swaps the active locale", async () => {
    await i18next.changeLanguage("uk");
    expect(i18next.language).toBe("uk");
    await i18next.changeLanguage("en");
    expect(i18next.language).toBe("en");
  });

  it("exists() returns false for absent keys (placeholder catalogs in M1)", () => {
    // M3a will populate real keys; until then any key is missing and the
    // app keeps rendering its hard-coded English strings unchanged.
    expect(i18next.exists("nonexistent.key")).toBe(false);
  });
});
