/** Non-empty trimmed lines from a textarea (URLs or paths). */
export function parseInputLines(text: string): string[] {
  return text
    .split(/\r?\n/)
    .map((l) => l.trim())
    .filter((l) => l.length > 0);
}

export function shortLabel(source: string, max = 56): string {
  const t = source.trim();
  if (t.length <= max) return t;
  return `${t.slice(0, max - 1)}…`;
}

export function newJobId(): string {
  if (typeof crypto !== "undefined" && "randomUUID" in crypto) {
    return crypto.randomUUID();
  }
  return `job-${Date.now()}-${Math.random().toString(16).slice(2)}`;
}
