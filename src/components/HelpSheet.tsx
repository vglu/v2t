import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import type { PrefsTarget } from "../types/preferences";

export type HelpSectionId =
  | "overview"
  | "cases"
  | "speakers"
  | "modes"
  | "outputs"
  | "cookies"
  | "map"
  | "limits";

const HELP_SECTIONS: readonly HelpSectionId[] = [
  "overview",
  "cases",
  "speakers",
  "modes",
  "outputs",
  "cookies",
  "map",
  "limits",
] as const;

const CASE_IDS = ["c1", "c2", "c3", "c4", "c5", "c6", "c7", "c8"] as const;
const MAP_IDS = [
  "m1",
  "m2",
  "m3",
  "m4",
  "m5",
  "m6",
  "m7",
  "m8",
  "m9",
  "m10",
  "m11",
  "m12",
] as const;

type Props = {
  open: boolean;
  onClose: () => void;
  onOpenPreferences?: (target?: PrefsTarget) => void;
  onOpenSetup?: () => void;
  initialSection?: HelpSectionId;
};

export function HelpSheet({
  open,
  onClose,
  onOpenPreferences,
  onOpenSetup,
  initialSection = "overview",
}: Props) {
  const { t } = useTranslation("help");
  const [section, setSection] = useState<HelpSectionId>(initialSection);

  useEffect(() => {
    if (!open) return;
    setSection(initialSection);
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        onClose();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, initialSection, onClose]);

  if (!open) return null;

  return (
    <div
      className="preferences-backdrop help-backdrop"
      data-testid="help-sheet"
      role="presentation"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div
        className="preferences-modal help-modal"
        role="dialog"
        aria-modal="true"
        aria-labelledby="help-title"
      >
        <div className="preferences-modal-top">
          <div className="preferences-modal-head">
            <div>
              <span className="preferences-eyebrow">{t("eyebrow")}</span>
              <h2 id="help-title">{t("title")}</h2>
              <p>{t("subtitle")}</p>
            </div>
            <button
              type="button"
              className="ghost preferences-close"
              data-testid="help-close"
              aria-label={t("close_aria")}
              onClick={onClose}
            >
              {t("close")}
            </button>
          </div>
          <div className="help-actions" role="group">
            {onOpenPreferences ? (
              <button
                type="button"
                className="ghost"
                data-testid="help-open-preferences"
                onClick={() => {
                  onClose();
                  onOpenPreferences();
                }}
              >
                {t("open_preferences")}
              </button>
            ) : null}
            {onOpenSetup ? (
              <button
                type="button"
                className="ghost"
                data-testid="help-open-setup"
                onClick={() => {
                  onClose();
                  onOpenSetup();
                }}
              >
                {t("open_setup")}
              </button>
            ) : null}
          </div>
        </div>

        <div className="preferences-layout">
          <nav
            className="preferences-depth-nav"
            aria-label={t("nav_aria")}
          >
            {HELP_SECTIONS.map((id) => (
              <button
                key={id}
                type="button"
                className={
                  section === id
                    ? "preferences-depth-tab preferences-depth-tab--active"
                    : "preferences-depth-tab"
                }
                aria-current={section === id ? "page" : undefined}
                data-testid={`help-nav-${id}`}
                onClick={() => setSection(id)}
              >
                <strong>{t(`nav.${id}`)}</strong>
                <span>{t(`nav_hint.${id}`)}</span>
              </button>
            ))}
          </nav>

          <div className="preferences-modal-body help-body">
            {section === "overview" ? <OverviewSection /> : null}
            {section === "cases" ? <CasesSection /> : null}
            {section === "speakers" ? <SpeakersSection /> : null}
            {section === "modes" ? <ModesSection /> : null}
            {section === "outputs" ? <OutputsSection /> : null}
            {section === "cookies" ? <CookiesSection /> : null}
            {section === "map" ? <MapSection /> : null}
            {section === "limits" ? <LimitsSection /> : null}
          </div>
        </div>
      </div>
    </div>
  );
}

function OverviewSection() {
  const { t } = useTranslation("help");
  return (
    <article className="help-article">
      <h3>{t("overview.title")}</h3>
      <p>{t("overview.intro")}</p>
      <h4>{t("overview.flow_title")}</h4>
      <ol className="help-steps">
        <li>{t("overview.flow_1")}</li>
        <li>{t("overview.flow_2")}</li>
        <li>{t("overview.flow_3")}</li>
      </ol>
      <h4>{t("overview.profiles_title")}</h4>
      <ul className="help-list">
        <li>{t("overview.profile_simple")}</li>
        <li>{t("overview.profile_quality")}</li>
        <li>{t("overview.profile_power")}</li>
      </ul>
      <p className="help-callout">{t("overview.note")}</p>
    </article>
  );
}

function CasesSection() {
  const { t } = useTranslation("help");
  return (
    <article className="help-article">
      <h3>{t("cases.title")}</h3>
      <p>{t("cases.intro")}</p>
      <div className="help-table-wrap">
        <table className="help-table">
          <thead>
            <tr>
              <th scope="col">{t("cases.want_label")}</th>
              <th scope="col">{t("cases.do_label")}</th>
            </tr>
          </thead>
          <tbody>
            {CASE_IDS.map((id) => (
              <tr key={id}>
                <td>{t(`cases.${id}_want`)}</td>
                <td>{t(`cases.${id}_do`)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </article>
  );
}

function SpeakersSection() {
  const { t } = useTranslation("help");
  return (
    <article className="help-article">
      <h3>{t("speakers.title")}</h3>
      <p>{t("speakers.intro")}</p>
      <h4>{t("speakers.path_a_title")}</h4>
      <ol className="help-steps">
        <li>{t("speakers.path_a_1")}</li>
        <li>{t("speakers.path_a_2")}</li>
        <li>{t("speakers.path_a_3")}</li>
        <li>{t("speakers.path_a_4")}</li>
        <li>{t("speakers.path_a_5")}</li>
      </ol>
      <h4>{t("speakers.path_b_title")}</h4>
      <ol className="help-steps">
        <li>{t("speakers.path_b_1")}</li>
        <li>{t("speakers.path_b_2")}</li>
        <li>{t("speakers.path_b_3")}</li>
      </ol>
      <h4>{t("speakers.why_missing_title")}</h4>
      <ul className="help-list">
        <li>{t("speakers.why_1")}</li>
        <li>{t("speakers.why_2")}</li>
        <li>{t("speakers.why_3")}</li>
        <li>{t("speakers.why_4")}</li>
      </ul>
    </article>
  );
}

function ModesSection() {
  const { t } = useTranslation("help");
  return (
    <article className="help-article">
      <h3>{t("modes.title")}</h3>
      <p>{t("modes.intro")}</p>
      <h4>{t("modes.cloud_title")}</h4>
      <p>{t("modes.cloud_body")}</p>
      <h4>{t("modes.local_title")}</h4>
      <p>{t("modes.local_body")}</p>
      <h4>{t("modes.browser_title")}</h4>
      <p>{t("modes.browser_body")}</p>
      <h4>{t("modes.subs_title")}</h4>
      <p>{t("modes.subs_body")}</p>
    </article>
  );
}

function OutputsSection() {
  const { t } = useTranslation("help");
  return (
    <article className="help-article">
      <h3>{t("outputs.title")}</h3>
      <p>{t("outputs.intro")}</p>
      <ul className="help-list">
        <li>{t("outputs.txt")}</li>
        <li>{t("outputs.vtt")}</li>
        <li>{t("outputs.srt")}</li>
        <li>{t("outputs.video")}</li>
        <li>{t("outputs.audio")}</li>
        <li>{t("outputs.info")}</li>
        <li>{t("outputs.template")}</li>
      </ul>
    </article>
  );
}

function CookiesSection() {
  const { t } = useTranslation("help");
  return (
    <article className="help-article">
      <h3>{t("cookies.title")}</h3>
      <p>{t("cookies.intro")}</p>
      <h4>{t("cookies.win_title")}</h4>
      <p>{t("cookies.win_body")}</p>
      <h4>{t("cookies.auto_title")}</h4>
      <p>{t("cookies.auto_body")}</p>
      <h4>{t("cookies.deno_title")}</h4>
      <p>{t("cookies.deno_body")}</p>
      <h4>{t("cookies.tiktok_title")}</h4>
      <p>{t("cookies.tiktok_body")}</p>
    </article>
  );
}

function MapSection() {
  const { t } = useTranslation("help");
  return (
    <article className="help-article">
      <h3>{t("map.title")}</h3>
      <p>{t("map.intro")}</p>
      <div className="help-table-wrap">
        <table className="help-table">
          <thead>
            <tr>
              <th scope="col">{t("map.where_label")}</th>
              <th scope="col">{t("map.section_label")}</th>
            </tr>
          </thead>
          <tbody>
            {MAP_IDS.map((id) => (
              <tr key={id}>
                <td>{t(`map.${id}`)}</td>
                <td>{t(`map.${id}w`)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </article>
  );
}

function LimitsSection() {
  const { t } = useTranslation("help");
  return (
    <article className="help-article">
      <h3>{t("limits.title")}</h3>
      <ul className="help-list">
        <li>{t("limits.l1")}</li>
        <li>{t("limits.l2")}</li>
        <li>{t("limits.l3")}</li>
        <li>{t("limits.l4")}</li>
        <li>{t("limits.l5")}</li>
        <li>{t("limits.l6")}</li>
        <li>{t("limits.l7")}</li>
        <li>{t("limits.l8")}</li>
      </ul>
    </article>
  );
}
