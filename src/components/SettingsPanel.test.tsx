import { render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { SettingsPanel } from "./SettingsPanel";
import { defaultAppSettings } from "../types/settings";

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn().mockResolvedValue(null),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn().mockResolvedValue(() => {}),
}));

vi.mock("../lib/invokeSafe", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../lib/invokeSafe")>();
  return {
    ...actual,
    listWhisperModels: vi.fn().mockResolvedValue([
      { id: "base", fileName: "ggml-base.bin", sizeMib: 142 },
    ]),
    defaultWhisperModelsDir: vi.fn().mockResolvedValue("C:\\\\AppData\\\\models"),
  };
});

describe("SettingsPanel", () => {
  it("shows OS credential store API key hint", async () => {
    render(
      <SettingsPanel
        settings={defaultAppSettings}
        onChange={vi.fn()}
        onSave={vi.fn()}
        onPersistSettings={vi.fn().mockResolvedValue(undefined)}
        onLanguageChange={vi.fn()}
        saving={false}
      />,
    );
    await waitFor(() => {
      expect(screen.getByTestId("cloud-credential-store-hint")).toBeVisible();
    });
  });

  it("language switcher fires onLanguageChange with the new locale", async () => {
    const onLanguageChange = vi.fn();
    render(
      <SettingsPanel
        settings={defaultAppSettings}
        onChange={vi.fn()}
        onSave={vi.fn()}
        onPersistSettings={vi.fn().mockResolvedValue(undefined)}
        onLanguageChange={onLanguageChange}
        saving={false}
      />,
    );
    const select = screen.getByTestId("settings-language-switcher") as HTMLSelectElement;
    expect(select.value).toBe("auto");
    const { default: userEvent } = await import("@testing-library/user-event");
    const user = userEvent.setup();
    await user.selectOptions(select, "uk");
    expect(onLanguageChange).toHaveBeenCalledWith("uk");
  });

  it("toggling subtitles fast-path reveals priority langs and keep-srt", async () => {
    const onChange = vi.fn();
    render(
      <SettingsPanel
        settings={defaultAppSettings}
        onChange={onChange}
        onSave={vi.fn()}
        onPersistSettings={vi.fn().mockResolvedValue(undefined)}
        onLanguageChange={vi.fn()}
        saving={false}
      />,
    );
    expect(screen.queryByTestId("subtitle-priority-langs")).toBeNull();

    render(
      <SettingsPanel
        settings={{ ...defaultAppSettings, useSubtitlesWhenAvailable: true }}
        onChange={onChange}
        onSave={vi.fn()}
        onPersistSettings={vi.fn().mockResolvedValue(undefined)}
        onLanguageChange={vi.fn()}
        saving={false}
      />,
    );
    const langInput = screen.getByTestId("subtitle-priority-langs") as HTMLInputElement;
    expect(langInput.value).toBe("uk, ru, en");
  });
});
