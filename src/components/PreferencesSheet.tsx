import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  applyPreset,
  isSimpleSurface,
  NAMED_PROFILES,
  reconcileProfileId,
  type ProfileId,
} from "../lib/profiles";
import type { PrefsDepth, PrefsFocus } from "../types/preferences";
import type { AppSettings, UiLanguage } from "../types/settings";
import { SettingsPanel } from "./SettingsPanel";

type Props = {
  open: boolean;
  depth: PrefsDepth;
  focus: PrefsFocus;
  onDepthChange: (d: PrefsDepth) => void;
  onClose: () => void;
  onDiscard: () => void;
  dirty: boolean;
  settings: AppSettings;
  onChange: (s: AppSettings) => void;
  onSave: () => Promise<boolean>;
  onPersistSettings: (s: AppSettings) => Promise<void>;
  onRefreshReadiness?: () => void;
  onLanguageChange: (lang: UiLanguage) => void;
  saving: boolean;
};

export function PreferencesSheet({
  open,
  depth,
  focus,
  onDepthChange,
  onClose,
  onDiscard,
  dirty,
  settings,
  onChange,
  onSave,
  onPersistSettings,
  onRefreshReadiness,
  onLanguageChange,
  saving,
}: Props) {
  const { t } = useTranslation("common");
  const [confirmClose, setConfirmClose] = useState(false);
  const [pendingProfile, setPendingProfile] = useState<
    Exclude<ProfileId, "custom"> | null
  >(null);

  const simple = isSimpleSurface(settings.profileId);

  const requestClose = () => {
    if (dirty) {
      setConfirmClose(true);
      return;
    }
    onClose();
  };

  useEffect(() => {
    if (!open) return;
    setConfirmClose(false);
    setPendingProfile(null);
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        requestClose();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [dirty, open, onClose]);

  if (!open) return null;

  const depthTabs = (
    [
      [
        "essentials",
        t("preferences.depth.essentials"),
        t("preferences.depth_hint.essentials"),
      ],
      [
        "engine",
        t("preferences.depth.engine"),
        t("preferences.depth_hint.engine"),
      ],
      [
        "advanced",
        simple
          ? t("preferences.depth.tools")
          : t("preferences.depth.advanced"),
        simple
          ? t("preferences.depth_hint.tools")
          : t("preferences.depth_hint.advanced"),
      ],
    ] as const
  );

  const activeProfile: ProfileId =
    settings.profileId === "simple" ||
    settings.profileId === "quality" ||
    settings.profileId === "power" ||
    settings.profileId === "custom"
      ? settings.profileId
      : "custom";

  return (
    <div
      className="preferences-backdrop"
      data-testid="preferences-sheet"
      role="presentation"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) requestClose();
      }}
    >
      <div
        className="preferences-modal"
        role="dialog"
        aria-modal="true"
        aria-labelledby="preferences-title"
      >
        <div className="preferences-modal-top">
          <div className="preferences-modal-head">
            <div>
              <span className="preferences-eyebrow">
                {t("preferences.eyebrow")}
              </span>
              <h2 id="preferences-title">{t("preferences.title")}</h2>
              <p>{t("preferences.subtitle")}</p>
            </div>
            <button
              type="button"
              className="ghost preferences-close"
              data-testid="preferences-close"
              aria-label={t("preferences.close_aria")}
              onClick={requestClose}
            >
              {t("preferences.close")}
            </button>
          </div>

          <div
            className="preferences-profile-bar"
            role="group"
            aria-label={t("preferences.profile_aria")}
            data-testid="preferences-profile-bar"
          >
            <div className="preferences-profile-bar-copy">
              <strong>{t("preferences.profile_label")}</strong>
              <span>{t("preferences.profile_hint")}</span>
            </div>
            <div className="preferences-profile-seg">
              {NAMED_PROFILES.map((id) => (
                <button
                  key={id}
                  type="button"
                  className={
                    activeProfile === id
                      ? "preferences-profile-chip preferences-profile-chip--active"
                      : "preferences-profile-chip"
                  }
                  data-testid={`prefs-profile-${id}`}
                  aria-pressed={activeProfile === id}
                  onClick={() => {
                    if (activeProfile === id) return;
                    setPendingProfile(id);
                  }}
                >
                  {t(`preferences.profile.${id}`)}
                </button>
              ))}
              {activeProfile === "custom" ? (
                <span
                  className="preferences-profile-chip preferences-profile-chip--custom"
                  data-testid="prefs-profile-custom"
                >
                  {t("preferences.profile.custom")}
                </span>
              ) : null}
            </div>
          </div>
        </div>

        <div className="preferences-layout">
          <nav
            className="preferences-depth-nav"
            aria-label={t("preferences.depth_aria")}
          >
            {depthTabs.map(([id, label, hint]) => (
              <button
                key={id}
                type="button"
                className={
                  depth === id
                    ? "preferences-depth-tab preferences-depth-tab--active"
                    : "preferences-depth-tab"
                }
                aria-current={depth === id ? "page" : undefined}
                data-testid={`prefs-depth-${id}`}
                onClick={() => onDepthChange(id)}
              >
                <strong>{label}</strong>
                <span>{hint}</span>
              </button>
            ))}
          </nav>

          <div className="preferences-modal-body">
            <SettingsPanel
              settings={settings}
              onChange={(next) => onChange(reconcileProfileId(next))}
              onSave={onSave}
              onPersistSettings={onPersistSettings}
              onRefreshReadiness={onRefreshReadiness}
              onLanguageChange={onLanguageChange}
              saving={saving}
              depth={depth}
              focus={focus}
              embeddedInSheet
              simpleSurface={simple}
            />
          </div>
        </div>

        <footer className="preferences-savebar">
          <div
            className={
              dirty
                ? "preferences-save-state preferences-save-state--dirty"
                : "preferences-save-state"
            }
            role="status"
          >
            <span aria-hidden />
            {dirty
              ? t("preferences.unsaved")
              : t("preferences.saved")}
          </div>
          <div className="preferences-save-actions">
            <button type="button" disabled={!dirty || saving} onClick={onDiscard}>
              {t("preferences.discard")}
            </button>
            <button
              type="button"
              className="primary"
              disabled={!dirty || saving}
              onClick={() => void onSave()}
            >
              {saving
                ? t("preferences.saving")
                : t("preferences.save")}
            </button>
          </div>
        </footer>

        {pendingProfile ? (
          <div className="preferences-close-confirm" role="alertdialog">
            <div>
              <strong>
                {t("preferences.profile_apply_title", {
                  profile: t(`preferences.profile.${pendingProfile}`),
                })}
              </strong>
              <p>{t("preferences.profile_apply_body")}</p>
            </div>
            <div className="preferences-save-actions">
              <button type="button" onClick={() => setPendingProfile(null)}>
                {t("preferences.profile_apply_cancel")}
              </button>
              <button
                type="button"
                className="primary"
                data-testid="prefs-profile-apply-confirm"
                onClick={() => {
                  onChange(applyPreset(settings, pendingProfile));
                  setPendingProfile(null);
                }}
              >
                {t("preferences.profile_apply_confirm")}
              </button>
            </div>
          </div>
        ) : null}

        {confirmClose ? (
          <div className="preferences-close-confirm" role="alertdialog">
            <div>
              <strong>{t("preferences.close_confirm_title")}</strong>
              <p>{t("preferences.close_confirm_body")}</p>
            </div>
            <div className="preferences-save-actions">
              <button type="button" onClick={() => setConfirmClose(false)}>
                {t("preferences.keep_editing")}
              </button>
              <button
                type="button"
                onClick={() => {
                  onDiscard();
                  onClose();
                }}
              >
                {t("preferences.discard_close")}
              </button>
              <button
                type="button"
                className="primary"
                disabled={saving}
                onClick={() => {
                  void onSave().then((ok) => {
                    if (ok) onClose();
                  });
                }}
              >
                {t("preferences.save")}
              </button>
            </div>
          </div>
        ) : null}
      </div>
    </div>
  );
}
