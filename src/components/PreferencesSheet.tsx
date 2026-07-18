import { useEffect } from "react";
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
  settings: AppSettings;
  onChange: (s: AppSettings) => void;
  onSave: () => void;
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
  settings,
  onChange,
  onSave,
  onPersistSettings,
  onRefreshReadiness,
  onLanguageChange,
  saving,
}: Props) {
  const { t } = useTranslation("common");

  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        onClose();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, onClose]);

  if (!open) return null;

  return (
    <div
      className="preferences-backdrop"
      data-testid="preferences-sheet"
      role="presentation"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div
        className="preferences-modal"
        role="dialog"
        aria-modal="true"
        aria-labelledby="preferences-title"
      >
        <div className="preferences-modal-head">
          <h2 id="preferences-title">{t("preferences.title")}</h2>
          <button
            type="button"
            className="ghost preferences-close"
            data-testid="preferences-close"
            aria-label={t("preferences.close_aria")}
            onClick={onClose}
          >
            {t("preferences.close")}
          </button>
        </div>

        <div
          className="preferences-depth-nav"
          role="tablist"
          aria-label={t("preferences.depth_aria")}
        >
          {(
            [
              ["essentials", t("preferences.depth.essentials")],
              ["engine", t("preferences.depth.engine")],
              ["advanced", t("preferences.depth.advanced")],
            ] as const
          ).map(([id, label]) => (
            <button
              key={id}
              type="button"
              role="tab"
              className={
                depth === id
                  ? "preferences-depth-tab preferences-depth-tab--active"
                  : "preferences-depth-tab"
              }
              aria-selected={depth === id}
              data-testid={`prefs-depth-${id}`}
              onClick={() => onDepthChange(id)}
            >
              {label}
            </button>
          ))}
        </div>

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
    </div>
  );
}
