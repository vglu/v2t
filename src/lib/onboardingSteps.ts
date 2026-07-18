import type { ProfileId } from "../types/settings";

export type IntentChoice = Exclude<ProfileId, "custom">;

export type WizardStepId =
  | "welcome"
  | "profile"
  | "output"
  | "tools"
  | "mode"
  | "engine"
  | "done";

export type ModeChoice = "cloud" | "local" | "browser" | "later";

/** Setup path length and content depend on the chosen usage profile. */
export function stepsForProfile(profile: IntentChoice): WizardStepId[] {
  switch (profile) {
    case "simple":
      return ["welcome", "profile", "output", "tools", "done"];
    case "quality":
    case "power":
      return ["welcome", "profile", "output", "tools", "mode", "engine", "done"];
  }
}

export function defaultModeForProfile(profile: IntentChoice): ModeChoice {
  return profile === "simple" ? "browser" : "local";
}

export function titleKeyForStep(
  stepId: WizardStepId,
  modeChoice: ModeChoice,
): string {
  switch (stepId) {
    case "welcome":
      return "step_title.welcome";
    case "profile":
      return "step_title.intent";
    case "output":
      return "step_title.output";
    case "tools":
      return "step_title.tools";
    case "mode":
      return "step_title.transcription";
    case "engine":
      if (modeChoice === "cloud") return "step_title.cloud";
      if (modeChoice === "local") return "step_title.local";
      if (modeChoice === "browser") return "step_title.browser";
      return "step_title.later";
    case "done":
      return "step_title.run";
  }
}
