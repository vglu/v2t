export type FilenameContext = {
  title: string;
  date: string;
  index: number;
  /** Track number within one queue job (1-based; playlists). */
  track: number;
  source: string;
  /** File extension without dot, e.g. `txt` or `mp4`. When omitted, legacy `.txt` templates still work. */
  ext?: string;
};

/** Remove characters unsafe for Windows / macOS file names. */
export function sanitizeFilenameSegment(raw: string): string {
  const cleaned = raw
    .replace(/[<>:"/\\|?*\u0000-\u001f]/g, "_")
    .trim()
    .slice(0, 120);
  return cleaned.length > 0 ? cleaned : "untitled";
}

function rewriteTxtSuffixToMp4(name: string): string {
  if (name.endsWith(".txt")) {
    return `${name.slice(0, -4)}.mp4`;
  }
  const dot = name.lastIndexOf(".");
  if (dot >= 0) {
    return `${name.slice(0, dot)}.mp4`;
  }
  return `${name}.mp4`;
}

/** Replace `{title}`, `{date}`, `{index}`, `{track}`, `{source}`, `{ext}`. */
export function formatOutputFilename(
  template: string,
  ctx: FilenameContext,
): string {
  const ext = ctx.ext;
  let t = template
    .replace(/\{title\}/g, sanitizeFilenameSegment(ctx.title))
    .replace(/\{date\}/g, sanitizeFilenameSegment(ctx.date))
    .replace(/\{index\}/g, String(ctx.index))
    .replace(/\{track\}/g, String(ctx.track))
    .replace(/\{source\}/g, sanitizeFilenameSegment(ctx.source));
  if (template.includes("{ext}") && ext) {
    t = t.replace(/\{ext\}/g, ext);
  } else if (ext === "mp4") {
    t = rewriteTxtSuffixToMp4(t);
  }
  return t;
}

/** Video filename next to transcript: same rules with `ext: "mp4"`. */
export function videoFilenameFromTranscriptTemplate(
  template: string,
  ctx: Omit<FilenameContext, "ext">,
): string {
  return formatOutputFilename(template, { ...ctx, ext: "mp4" });
}
