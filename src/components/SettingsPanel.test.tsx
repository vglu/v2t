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
        saving={false}
      />,
    );
    await waitFor(() => {
      expect(
        screen.getByText(/API key is saved in the OS credential store/i),
      ).toBeVisible();
    });
  });
});
