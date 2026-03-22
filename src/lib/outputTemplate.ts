export type FilenameContext = {
  title: string;
  date: string;
  index: number;
  /** Track number within one queue job (1-based; playlists). */
  track: number;
  source: string;
};

/** Remove characters unsafe for Windows / macOS file names. */
export function sanitizeFilenameSegment(raw: string): string {
  const cleaned = raw
    .replace(/[<>:"/\\|?*\u0000-\u001f]/g, "_")
    .trim()
    .slice(0, 120);
  return cleaned.length > 0 ? cleaned : "untitled";
}

export function formatOutputFilename(
  template: string,
  ctx: FilenameContext,
): string {
  return template
    .replace(/\{title\}/g, sanitizeFilenameSegment(ctx.title))
    .replace(/\{date\}/g, sanitizeFilenameSegment(ctx.date))
    .replace(/\{index\}/g, String(ctx.index))
    .replace(/\{track\}/g, String(ctx.track))
    .replace(/\{source\}/g, sanitizeFilenameSegment(ctx.source));
}
