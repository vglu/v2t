import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
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

        <div className="preferences-layout">
          <nav
            className="preferences-depth-nav"
            aria-label={t("preferences.depth_aria")}
          >
            {(
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
                  t("preferences.depth.advanced"),
                  t("preferences.depth_hint.advanced"),
                ],
              ] as const
            ).map(([id, label, hint]) => (
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
              onChange={onChange}
              onSave={onSave}
              onPersistSettings={onPersistSettings}
              onRefreshReadiness={onRefreshReadiness}
              onLanguageChange={onLanguageChange}
              saving={saving}
              depth={depth}
              focus={focus}
              embeddedInSheet
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
