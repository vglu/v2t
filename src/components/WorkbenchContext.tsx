import { useTranslation } from "react-i18next";
import type { AppSettings } from "../types/settings";

type Props = {
  settings: Pick<AppSettings, "transcriptionMode" | "outputDir">;
  readinessComplete: boolean;
  onOpenPreferences: () => void;
  onChangeOutput?: () => void;
};

function truncatePath(path: string, max = 42): string {
  const p = path.trim();
  if (p.length <= max) return p;
  return `…${p.slice(-(max - 1))}`;
}

export function WorkbenchContext({
  settings,
  readinessComplete,
  onOpenPreferences,
  onChangeOutput,
}: Props) {
  const { t } = useTranslation("common");
  const out = settings.outputDir?.trim() ?? "";
  const modeText =
    settings.transcriptionMode === "localWhisper"
      ? t("context.mode_local")
      : settings.transcriptionMode === "browserWhisper"
        ? t("context.mode_browser")
        : t("context.mode_cloud");

  return (
    <div className="workbench-context" data-testid="workbench-context">
      <div className="workbench-context-main">
        <button
          type="button"
          className="workbench-engine-chip"
          data-testid="workbench-engine-chip"
          onClick={onOpenPreferences}
          title={t("context.engine_title")}
        >
          {modeText}
        </button>
        <button
          type="button"
          className="workbench-output-chip"
          data-testid="workbench-output-chip"
          onClick={onChangeOutput ?? onOpenPreferences}
          title={out || t("context.output_unset")}
        >
          {out ? truncatePath(out) : t("context.output_unset")}
        </button>
        <span
          className={
            readinessComplete
              ? "workbench-ready workbench-ready--ok"
              : "workbench-ready workbench-ready--amber"
          }
          data-testid="workbench-ready"
        >
          {readinessComplete ? t("context.ready") : t("context.not_ready")}
        </span>
      </div>
      <button
        type="button"
        className="ghost"
        data-testid="open-preferences"
        onClick={onOpenPreferences}
      >
        {t("preferences.open")}
      </button>
    </div>
  );
}
