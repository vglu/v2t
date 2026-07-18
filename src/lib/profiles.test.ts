import { describe, expect, it } from "vitest";
import {
  applyPreset,
  matchesPreset,
  reconcileProfileId,
} from "./profiles";
import { defaultAppSettings } from "../types/settings";

describe("profiles", () => {
  it("applyPreset sets profileId and quality defaults", () => {
    const next = applyPreset(defaultAppSettings, "quality");
    expect(next.profileId).toBe("quality");
    expect(next.exportWebVtt).toBe(true);
    expect(next.whisperModel).toBe("large-v3");
    expect(next.apiModel).toBe("whisper-1");
    expect(next.keepDownloadedAudio).toBe(true);
    expect(next.apiKey).toBe(defaultAppSettings.apiKey);
    expect(next.outputDir).toBe(defaultAppSettings.outputDir);
  });

  it("simple disables REST and VTT", () => {
    const withApi = {
      ...defaultAppSettings,
      apiServer: { enabled: true, port: 8788, bearerToken: "tok" },
      exportWebVtt: true,
    };
    const next = applyPreset(withApi, "simple");
    expect(next.exportWebVtt).toBe(false);
    expect(next.useSubtitlesWhenAvailable).toBe(true);
    expect(next.apiServer.enabled).toBe(false);
    expect(next.apiServer.bearerToken).toBe("tok");
  });

  it("matchesPreset / reconcileProfileId detect drift", () => {
    const quality = applyPreset(defaultAppSettings, "quality");
    expect(matchesPreset(quality, "quality")).toBe(true);
    const drifted = { ...quality, exportWebVtt: false };
    expect(matchesPreset(drifted, "quality")).toBe(false);
    expect(reconcileProfileId(drifted).profileId).toBe("custom");
  });

  it("power keeps REST disabled until user enables it", () => {
    const next = applyPreset(defaultAppSettings, "power");
    expect(next.profileId).toBe("power");
    expect(next.keepDownloadedVideo).toBe(true);
    expect(next.recursiveFolderScan).toBe(true);
    expect(next.apiServer.enabled).toBe(false);
  });
});
