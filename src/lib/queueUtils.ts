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

/** Filename without parent dirs and without extension. Handles both Windows
 * (`\\`) and Unix (`/`) separators since drops on Windows can mix both. */
export function fileBasenameNoExt(source: string): string {
  const t = source.trim();
  if (!t) return t;
  const lastSep = Math.max(t.lastIndexOf("\\"), t.lastIndexOf("/"));
  const base = lastSep >= 0 ? t.slice(lastSep + 1) : t;
  const dot = base.lastIndexOf(".");
  if (dot <= 0) return base; // no extension or hidden file like ".env"
  return base.slice(0, dot);
}

export function newJobId(): string {
  if (typeof crypto !== "undefined" && "randomUUID" in crypto) {
    return crypto.randomUUID();
  }
  return `job-${Date.now()}-${Math.random().toString(16).slice(2)}`;
}
