import "@testing-library/jest-dom/vitest";
// Initialize i18next with the en catalogs before any component renders.
// Without this `useTranslation()` returns the raw keys ("panel_aria") and
// text-content matchers break.
import "../i18n";

