import { describe, expect, it } from "vitest";
import {
  defaultModeForProfile,
  stepsForProfile,
  titleKeyForStep,
} from "./onboardingSteps";

describe("stepsForProfile", () => {
  it("keeps Simple short without mode/engine", () => {
    expect(stepsForProfile("simple")).toEqual([
      "welcome",
      "profile",
      "output",
      "tools",
      "done",
    ]);
  });

  it("gives Quality and Power the full engine path", () => {
    const full = [
      "welcome",
      "profile",
      "output",
      "tools",
      "mode",
      "engine",
      "done",
    ];
    expect(stepsForProfile("quality")).toEqual(full);
    expect(stepsForProfile("power")).toEqual(full);
  });
});

describe("defaultModeForProfile", () => {
  it("seeds browser for Simple and local otherwise", () => {
    expect(defaultModeForProfile("simple")).toBe("browser");
    expect(defaultModeForProfile("quality")).toBe("local");
    expect(defaultModeForProfile("power")).toBe("local");
  });
});

describe("titleKeyForStep", () => {
  it("maps engine titles from mode", () => {
    expect(titleKeyForStep("engine", "cloud")).toBe("step_title.cloud");
    expect(titleKeyForStep("engine", "later")).toBe("step_title.later");
    expect(titleKeyForStep("done", "browser")).toBe("step_title.run");
  });
});
