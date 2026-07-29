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

/** TikTok photo posts and other non-media links yt-dlp cannot fetch. */
export function isUnsupportedMediaUrl(url: string): boolean {
  const trimmed = url.trim();
  if (!trimmed) return false;

  // Fast path for the most common rule.
  const lower = trimmed.toLowerCase();
  if (lower.includes("tiktok.com") && lower.includes("/photo/")) return true;

  try {
    const u = new URL(trimmed);
    const host = u.hostname.replace(/^www\./i, "").toLowerCase();
    if (!host.endsWith("tiktok.com")) return false;
    const path = u.pathname;

    // Profile root: /@user (with or without trailing slash) -> unsupported.
    // Note: /@user/video/... is supported (not classified as unsupported here).
    if (path === "" || path === "/") return false;
    if (/^\/@[^/]+\/?$/.test(path)) return !path.toLowerCase().includes("/video/");
    return false;
  } catch {
    // Fallback: keep behavior conservative.
    // Examples we treat as unsupported: tiktok.com/@user and no further path.
    const m = lower.match(/tiktok\.com\/@[^/?#]+\/?([?#].*)?$/);
    if (!m) return false;
    if (lower.includes("/video/")) return false;
    return true;
  }
}
