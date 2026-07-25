import { describe, expect, it } from "vitest";
import {
  labelSpeakersEffective,
  speakersAllowedForLanguage,
} from "./speakersPolicy";

describe("speakersPolicy", () => {
  it("allows speakers only for explicit English", () => {
    expect(speakersAllowedForLanguage(null)).toBe(false);
    expect(speakersAllowedForLanguage("")).toBe(false);
    expect(speakersAllowedForLanguage("ru")).toBe(false);
    expect(speakersAllowedForLanguage("uk")).toBe(false);
    expect(speakersAllowedForLanguage("en")).toBe(true);
    expect(speakersAllowedForLanguage("EN")).toBe(true);
    expect(speakersAllowedForLanguage(" en ")).toBe(true);
  });

  it("requires WebVTT + toggle + English", () => {
    expect(labelSpeakersEffective(true, true, "ru")).toBe(false);
    expect(labelSpeakersEffective(true, true, null)).toBe(false);
    expect(labelSpeakersEffective(false, true, "en")).toBe(false);
    expect(labelSpeakersEffective(true, false, "en")).toBe(false);
    expect(labelSpeakersEffective(true, true, "en")).toBe(true);
  });
});
