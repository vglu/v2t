/** tinydiarize (`small.en-tdrz`) is English-only; never enable it for other languages. */
export function speakersAllowedForLanguage(language: string | null | undefined): boolean {
  return (language ?? "").trim().toLowerCase() === "en";
}

/** Mirrors Rust `label_speakers_effective`. */
export function labelSpeakersEffective(
  exportWebVtt: boolean,
  labelSpeakers: boolean,
  language: string | null | undefined,
): boolean {
  return Boolean(exportWebVtt && labelSpeakers && speakersAllowedForLanguage(language));
}
