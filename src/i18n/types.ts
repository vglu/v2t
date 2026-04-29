/**
 * Strict TypeScript types for i18next keys.
 *
 * Augments the `i18next` module so `useTranslation("settings").t("typo")`
 * fails at compile time once M3a populates `src/locales/en/*.json` with
 * real keys. While the JSON files are still empty (M1), the inferred shape
 * is `{}` and `t(...)` accepts any string — that's intentional: M4-M6
 * progressively narrow the type as keys are added.
 *
 * See: https://www.i18next.com/overview/typescript
 */
import "i18next";

import type commonEn from "../locales/en/common.json";
import type onboardingEn from "../locales/en/onboarding.json";
import type settingsEn from "../locales/en/settings.json";
import type queueEn from "../locales/en/queue.json";
import type readinessEn from "../locales/en/readiness.json";

declare module "i18next" {
  interface CustomTypeOptions {
    defaultNS: "common";
    resources: {
      common: typeof commonEn;
      onboarding: typeof onboardingEn;
      settings: typeof settingsEn;
      queue: typeof queueEn;
      readiness: typeof readinessEn;
    };
  }
}
