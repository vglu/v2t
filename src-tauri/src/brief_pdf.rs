//! Render a BriefDoc to a PDF file using printpdf with embedded NotoSans fonts.
//!
//! Layout: A4 portrait, 20mm left/right margins, 25mm top/bottom margins.
//! Fonts: NotoSans-Regular + NotoSans-Bold (compiled in via include_bytes!).
//! Supports Unicode (Latin, Cyrillic, and other Unicode ranges in NotoSans).

use std::collections::HashSet;
use std::path::Path;

use printpdf::*;

use crate::brief_provider::{BriefDoc, BriefFrame};
use crate::settings::BriefDocMode;

// Embed fonts at compile time — no runtime path lookups needed.
static NOTO_SANS_REGULAR: &[u8] = include_bytes!("../assets/fonts/NotoSans-Regular.ttf");
static NOTO_SANS_BOLD: &[u8] = include_bytes!("../assets/fonts/NotoSans-Bold.ttf");

// A4 dimensions in mm
const PAGE_W: f32 = 210.0;
const PAGE_H: f32 = 297.0;

// Margins in mm
const MARGIN_L: f32 = 20.0;
const MARGIN_R: f32 = 20.0;
const MARGIN_T: f32 = 25.0;
const MARGIN_B: f32 = 20.0;

const BODY_W: f32 = PAGE_W - MARGIN_L - MARGIN_R; // 170 mm

// Font sizes in pt
const SIZE_H1: f32 = 18.0;
const SIZE_H2: f32 = 13.0;
const SIZE_H3: f32 = 11.0;
const SIZE_BODY: f32 = 10.0;
const SIZE_SMALL: f32 = 8.5;
const SIZE_CAPTION: f32 = 9.0;

// Line heights in mm
const LH_H1: f32 = 8.5;
const LH_H2: f32 = 6.5;
const LH_H3: f32 = 5.5;
const LH_BODY: f32 = 5.0;
const LH_SMALL: f32 = 4.5;
const LH_CAPTION: f32 = 4.5;

const PARA_AFTER_H1: f32 = 4.0;
const PARA_AFTER_H2: f32 = 3.0;
const PARA_AFTER_H3: f32 = 2.0;
const PARA_AFTER_ITEM: f32 = 1.5;

const NONE_STATED: &str = "None stated in the meeting.";

/// Render `doc` with optional `frames` (screenshots) to `out_path` as PDF.
pub fn render_brief_pdf(doc: &BriefDoc, frames: &[BriefFrame], out_path: &Path) -> Result<(), String> {
    let default_title = match doc.doc_mode {
        BriefDocMode::Fdd => "Functional Design Document",
        BriefDocMode::Brief => "Design Document",
    };
    let title = if doc.title.is_empty() {
        default_title.to_string()
    } else {
        doc.title.clone()
    };
    let (pdf, page1, layer1) = PdfDocument::new(
        sanitize_title(&title),
        Mm(PAGE_W),
        Mm(PAGE_H),
        "Layer 1",
    );

    let font_regular = pdf
        .add_external_font(std::io::Cursor::new(NOTO_SANS_REGULAR))
        .map_err(|e| format!("NotoSans-Regular load failed: {e}"))?;
    let font_bold = pdf
        .add_external_font(std::io::Cursor::new(NOTO_SANS_BOLD))
        .map_err(|e| format!("NotoSans-Bold load failed: {e}"))?;

    let mut ctx = Ctx {
        pdf: &pdf,
        reg: font_regular,
        bld: font_bold,
        page_idx: page1,
        layer_idx: layer1,
        y: PAGE_H - MARGIN_T,
    };

    write_header(&mut ctx, doc, &title);

    // Build set of frame indices already placed inline under Business Rules (FDD only).
    let inline_indices: HashSet<usize> = if matches!(doc.doc_mode, BriefDocMode::Fdd) {
        doc.business_rules
            .iter()
            .flat_map(|br| br.screenshot_refs.iter().copied())
            .collect()
    } else {
        HashSet::new()
    };

    match doc.doc_mode {
        BriefDocMode::Fdd => write_fdd_body(&mut ctx, doc, frames),
        BriefDocMode::Brief => write_brief_body(&mut ctx, doc),
    }

    // Appendix: only frames not already shown inline under a BR.
    let appendix_frames: Vec<&BriefFrame> =
        frames.iter().filter(|f| !inline_indices.contains(&f.index)).collect();
    write_visual_appendix(&mut ctx, &appendix_frames);

    // Important: ctx must be dropped before pdf.save() so the Rc refcount drops to 1.
    drop(ctx);

    let bytes = pdf.save_to_bytes().map_err(|e| format!("PDF save failed: {e}"))?;
    std::fs::write(out_path, bytes).map_err(|e| format!("Failed to write PDF: {e}"))?;
    Ok(())
}

fn write_header(ctx: &mut Ctx<'_>, doc: &BriefDoc, title: &str) {
    ctx.h1(title);
    ctx.gap(PARA_AFTER_H1);

    let mut meta: Vec<String> = Vec::new();
    meta.push(format!("Document type: {}", doc.doc_mode.label()));
    if !doc.date.is_empty() {
        meta.push(format!("Date: {}", doc.date));
    }
    if doc.duration_secs > 0.0 {
        meta.push(format!("Duration: {}", fmt_duration(doc.duration_secs)));
    }
    if !doc.source.is_empty() {
        meta.push(format!("Source: {}", doc.source));
    }
    for m in &meta {
        ctx.body(m);
    }
    if !meta.is_empty() {
        ctx.gap(PARA_AFTER_H2);
    }

    if let Some(w) = &doc.no_video_warning {
        ctx.small(&format!("Note: {w}"));
        ctx.gap(PARA_AFTER_ITEM);
    }
    if doc.has_transcript_gaps {
        ctx.small("Note: Transcript may contain ASR gaps. Verify decisions against recording.");
        ctx.gap(PARA_AFTER_ITEM);
    }
}

fn write_brief_body(ctx: &mut Ctx<'_>, doc: &BriefDoc) {
    ctx.h2("Context / Attendees");
    ctx.gap(PARA_AFTER_H2);
    if doc.attendees.is_empty() {
        ctx.body(NONE_STATED);
    } else {
        for a in &doc.attendees {
            ctx.bullet(a);
        }
    }
    ctx.gap(PARA_AFTER_H2);

    ctx.h2("Summary");
    ctx.gap(PARA_AFTER_H2);
    if doc.summary.is_empty() {
        ctx.body(NONE_STATED);
    } else {
        ctx.paragraph(&doc.summary);
    }
    ctx.gap(PARA_AFTER_H2);

    ctx.h2("Decisions");
    ctx.gap(PARA_AFTER_H2);
    if doc.decisions.is_empty() {
        ctx.body(NONE_STATED);
    } else {
        for item in &doc.decisions {
            let ts = ts_suffix(&item.timestamp);
            ctx.paragraph(&format!("- {}{ts}", item.text));
            ctx.gap(PARA_AFTER_ITEM);
        }
    }
    ctx.gap(PARA_AFTER_H2);

    write_action_items(ctx, doc);
    write_open_questions(ctx, doc);

    ctx.h2("Engineering Design");
    ctx.gap(PARA_AFTER_H2);

    ctx.h3("Scope - In");
    ctx.gap(PARA_AFTER_H3);
    if doc.scope_in.is_empty() {
        ctx.body(NONE_STATED);
    } else {
        for s in &doc.scope_in {
            ctx.bullet(s);
        }
    }
    ctx.gap(PARA_AFTER_H3);

    ctx.h3("Scope - Out");
    ctx.gap(PARA_AFTER_H3);
    if doc.scope_out.is_empty() {
        ctx.body(NONE_STATED);
    } else {
        for s in &doc.scope_out {
            ctx.bullet(s);
        }
    }
    ctx.gap(PARA_AFTER_H3);

    ctx.h3("Acceptance Criteria");
    ctx.gap(PARA_AFTER_H3);
    if doc.acceptance_criteria.is_empty() {
        ctx.body(NONE_STATED);
    } else {
        for s in &doc.acceptance_criteria {
            ctx.bullet(s);
        }
    }
    ctx.gap(PARA_AFTER_H3);

    write_risks(ctx, doc);
}

fn write_fdd_body(ctx: &mut Ctx<'_>, doc: &BriefDoc, frames: &[BriefFrame]) {
    ctx.h2("1. Purpose & Scope");
    ctx.gap(PARA_AFTER_H2);
    if doc.purpose.is_empty() && doc.summary.is_empty() {
        ctx.body(NONE_STATED);
    } else if !doc.purpose.is_empty() {
        ctx.paragraph(&doc.purpose);
    } else {
        ctx.paragraph(&doc.summary);
    }
    ctx.gap(PARA_AFTER_H3);

    ctx.h3("In scope");
    ctx.gap(PARA_AFTER_H3);
    if doc.scope_in.is_empty() {
        ctx.body(NONE_STATED);
    } else {
        for s in &doc.scope_in {
            ctx.bullet(s);
        }
    }
    ctx.gap(PARA_AFTER_H3);

    ctx.h3("Out of scope");
    ctx.gap(PARA_AFTER_H3);
    if doc.scope_out.is_empty() {
        ctx.body(NONE_STATED);
    } else {
        for s in &doc.scope_out {
            ctx.bullet(s);
        }
    }
    ctx.gap(PARA_AFTER_H2);

    ctx.h2("2. Glossary & Data Dictionary");
    ctx.gap(PARA_AFTER_H2);
    if doc.glossary.is_empty() {
        ctx.body(NONE_STATED);
    } else {
        for g in &doc.glossary {
            ctx.paragraph(&format!("- {}: {}", g.term, g.meaning));
            let mut meta = String::new();
            if !g.source.is_empty() {
                meta.push_str(&format!("Source: {}", g.source));
            }
            if !g.type_format.is_empty() {
                if !meta.is_empty() {
                    meta.push_str("  |  ");
                }
                meta.push_str(&format!("Type: {}", g.type_format));
            }
            if !meta.is_empty() {
                ctx.small(&format!("  {meta}"));
            }
            ctx.gap(PARA_AFTER_ITEM);
        }
    }
    ctx.gap(PARA_AFTER_H2);

    if !doc.work_items.is_empty() {
        ctx.h2("Work Items (ADO)");
        ctx.gap(PARA_AFTER_H2);
        for wi in &doc.work_items {
            let title = if wi.title.is_empty() {
                String::new()
            } else {
                format!(" — {}", wi.title)
            };
            let src = if wi.source.is_empty() {
                String::new()
            } else {
                format!("  ({})", wi.source)
            };
            ctx.bullet(&format!("#{}{title}{src}", wi.id));
        }
        ctx.gap(PARA_AFTER_H2);
    }

    ctx.h2("3. Business Rules");
    ctx.gap(PARA_AFTER_H2);
    if doc.business_rules.is_empty() {
        ctx.body(NONE_STATED);
    } else {
        for br in &doc.business_rules {
            let ts = ts_suffix(&br.timestamp);
            ctx.paragraph(&format!("{} — {}{ts}", br.id, br.text));
            // Inline screenshots evidencing this rule
            for &ref_idx in &br.screenshot_refs {
                if let Some(frame) = frames.iter().find(|f| f.index == ref_idx) {
                    ctx.gap(PARA_AFTER_ITEM);
                    if let Err(e) = ctx.insert_image(&frame.path) {
                        ctx.small(&format!("(Screenshot not available: {e})"));
                    }
                    ctx.gap(PARA_AFTER_H3);
                    let caption_text = if frame.caption.is_empty() {
                        format!("[{}]  {}", frame.timestamp_str(), frame.label.replace('_', " "))
                    } else {
                        format!(
                            "[{}]  {}  — {}",
                            frame.timestamp_str(),
                            frame.label.replace('_', " "),
                            frame.caption
                        )
                    };
                    ctx.paragraph_size(&caption_text, SIZE_CAPTION, LH_CAPTION);
                }
            }
            ctx.gap(PARA_AFTER_ITEM);
        }
    }
    ctx.gap(PARA_AFTER_H2);

    ctx.h2("4. Calculations & Formulas");
    ctx.gap(PARA_AFTER_H2);
    if doc.formulas.is_empty() {
        ctx.body(NONE_STATED);
    } else {
        for f in &doc.formulas {
            let name = if f.name.is_empty() { "Formula" } else { f.name.as_str() };
            ctx.h3(name);
            ctx.gap(PARA_AFTER_H3);
            if !f.formula.is_empty() {
                ctx.paragraph(&f.formula);
            }
            if !f.inputs.is_empty() {
                ctx.small(&format!("Inputs: {}", f.inputs));
            }
            if !f.example.is_empty() {
                ctx.small(&format!("Example: {}", f.example));
            }
            ctx.gap(PARA_AFTER_ITEM);
        }
    }
    ctx.gap(PARA_AFTER_H2);

    ctx.h2("5. Worked Examples");
    ctx.gap(PARA_AFTER_H2);
    if doc.worked_examples.is_empty() {
        ctx.body(NONE_STATED);
    } else {
        for ex in &doc.worked_examples {
            let ts = ts_suffix(&ex.timestamp);
            let heading = if ex.title.is_empty() {
                format!("Example{ts}")
            } else {
                format!("{}{ts}", ex.title)
            };
            ctx.h3(&heading);
            ctx.gap(PARA_AFTER_H3);
            for (i, step) in ex.steps.iter().enumerate() {
                ctx.bullet(&format!("{}. {step}", i + 1));
            }
            ctx.gap(PARA_AFTER_ITEM);
        }
    }
    ctx.gap(PARA_AFTER_H2);

    ctx.h2("6. Edge Cases & Validation");
    ctx.gap(PARA_AFTER_H2);
    if doc.edge_cases.is_empty() {
        ctx.body(NONE_STATED);
    } else {
        for ec in &doc.edge_cases {
            ctx.paragraph(&format!("- {}: {}", ec.case, ec.expected));
            ctx.gap(PARA_AFTER_ITEM);
        }
    }
    ctx.gap(PARA_AFTER_H2);

    write_open_questions(ctx, doc);
    write_action_items(ctx, doc);

    if !doc.release_notes.is_empty() {
        ctx.h2("Release & Process Notes");
        ctx.gap(PARA_AFTER_H2);
        for n in &doc.release_notes {
            ctx.bullet(n);
        }
        ctx.gap(PARA_AFTER_H2);
    }

    write_risks(ctx, doc);
}

fn write_action_items(ctx: &mut Ctx<'_>, doc: &BriefDoc) {
    ctx.h2("Action Items / Tasks");
    ctx.gap(PARA_AFTER_H2);
    if doc.action_items.is_empty() {
        ctx.body(NONE_STATED);
    } else {
        for item in &doc.action_items {
            let ts = ts_suffix(&item.timestamp);
            ctx.paragraph(&format!("- {}{ts}", item.what));
            ctx.small(&format!("  Who: {}  |  Deadline: {}", item.who, item.deadline));
            ctx.gap(PARA_AFTER_ITEM);
        }
    }
    ctx.gap(PARA_AFTER_H2);
}

fn write_open_questions(ctx: &mut Ctx<'_>, doc: &BriefDoc) {
    let heading = match doc.doc_mode {
        BriefDocMode::Fdd => "7. Open Questions",
        BriefDocMode::Brief => "Open Questions / Clarifications",
    };
    ctx.h2(heading);
    ctx.gap(PARA_AFTER_H2);
    if doc.open_questions.is_empty() {
        ctx.body(NONE_STATED);
    } else {
        for item in &doc.open_questions {
            let ts = ts_suffix(&item.timestamp);
            ctx.paragraph(&format!("- {}{ts}", item.text));
            ctx.gap(PARA_AFTER_ITEM);
        }
    }
    ctx.gap(PARA_AFTER_H2);
}

fn write_risks(ctx: &mut Ctx<'_>, doc: &BriefDoc) {
    if doc.risks.is_empty() && matches!(doc.doc_mode, BriefDocMode::Fdd) {
        return;
    }
    ctx.h3("Risks");
    ctx.gap(PARA_AFTER_H3);
    if doc.risks.is_empty() {
        ctx.body(NONE_STATED);
    } else {
        for r in &doc.risks {
            ctx.bullet(&format!("[{}] {}", r.severity.to_uppercase(), r.text));
        }
    }
    ctx.gap(PARA_AFTER_H2);
}

fn write_visual_appendix(ctx: &mut Ctx<'_>, frames: &[&BriefFrame]) {
    if frames.is_empty() {
        return;
    }
    // New page only when the current one is already well used — avoids a blank
    // trailing page when appendix would fit / when prior page was barely started.
    if ctx.y < PAGE_H - MARGIN_T - 40.0 {
        ctx.new_page();
    }
    ctx.h2("Visual Appendix");
    ctx.gap(PARA_AFTER_H2);

    for frame in frames {
        ctx.h3(&format!(
            "[{}]  {}",
            frame.timestamp_str(),
            frame.label.replace('_', " ")
        ));
        ctx.gap(PARA_AFTER_H3);

        if let Err(e) = ctx.insert_image(&frame.path) {
            ctx.small(&format!("(Screenshot not available: {e})"));
        }
        ctx.gap(PARA_AFTER_H3);

        if !frame.caption.is_empty() {
            ctx.paragraph_size(&frame.caption, SIZE_CAPTION, LH_CAPTION);
        }
        ctx.gap(PARA_AFTER_H2);
    }
}

fn ts_suffix(timestamp: &str) -> String {
    let normalized = normalize_timestamp_display(timestamp);
    if normalized.is_empty() {
        String::new()
    } else {
        format!("  [{normalized}]")
    }
}

/// Fix common LLM mistakes like `08:55:00` meaning `00:08:55` (MM:SS padded as HH:MM:SS).
pub fn normalize_timestamp_display(timestamp: &str) -> String {
    let t = timestamp.trim();
    if t.is_empty() {
        return String::new();
    }
    let parts: Vec<&str> = t.split(':').collect();
    match parts.as_slice() {
        [h, m, s] => {
            let Ok(hh) = h.parse::<u32>() else {
                return t.to_string();
            };
            let Ok(mm) = m.parse::<u32>() else {
                return t.to_string();
            };
            let Ok(ss) = s.parse::<u32>() else {
                return t.to_string();
            };
            // Model wrote MM:SS:00 as HH:MM:SS (e.g. 08:55:00 → 00:08:55).
            // Keep genuine hour marks (01:30:00 = 1h30m) when hour part is small.
            if ss == 0 && (3..=59).contains(&hh) && mm <= 59 {
                return format!("00:{hh:02}:{mm:02}");
            }
            format!("{hh:02}:{mm:02}:{ss:02}")
        }
        [m, s] => {
            let Ok(mm) = m.parse::<u32>() else {
                return t.to_string();
            };
            let Ok(ss) = s.parse::<u32>() else {
                return t.to_string();
            };
            format!("00:{mm:02}:{ss:02}")
        }
        _ => t.to_string(),
    }
}

// ── Render context ─────────────────────────────────────────────────────────────

struct Ctx<'a> {
    pdf: &'a PdfDocumentReference,
    reg: IndirectFontRef,   // regular font
    bld: IndirectFontRef,   // bold font
    page_idx: PdfPageIndex,
    layer_idx: PdfLayerIndex,
    /// Current Y in mm from bottom (we write top-down).
    y: f32,
}

impl<'a> Ctx<'a> {
    fn layer(&self) -> PdfLayerReference {
        self.pdf.get_page(self.page_idx).get_layer(self.layer_idx)
    }

    fn new_page(&mut self) {
        let (pi, li) = self.pdf.add_page(Mm(PAGE_W), Mm(PAGE_H), "Layer 1");
        self.page_idx = pi;
        self.layer_idx = li;
        self.y = PAGE_H - MARGIN_T;
    }

    fn advance(&mut self, lh: f32) {
        self.y -= lh;
        if self.y < MARGIN_B {
            self.new_page();
        }
    }

    fn gap(&mut self, mm: f32) {
        self.y -= mm;
        if self.y < MARGIN_B {
            self.new_page();
        }
    }

    fn draw_line_at(&mut self, y: f32) {
        let layer = self.layer();
        let line = Line {
            points: vec![
                (Point::new(Mm(MARGIN_L), Mm(y)), false),
                (Point::new(Mm(PAGE_W - MARGIN_R), Mm(y)), false),
            ],
            is_closed: false,
        };
        layer.add_line(line);
    }

    fn emit_text(&self, text: &str, size: f32, bold: bool) {
        let font = if bold { &self.bld } else { &self.reg };
        let layer = self.layer();
        layer.use_text(text, size, Mm(MARGIN_L), Mm(self.y), font);
    }

    // ── Public draw methods ─────────────────────────────────────────────────

    fn h1(&mut self, text: &str) {
        self.wrapped(text, SIZE_H1, LH_H1, true);
    }

    fn h2(&mut self, text: &str) {
        // Underline *below* the heading — never draw a rule through glyph bodies.
        self.gap(4.0);
        if self.y < MARGIN_B + LH_H2 + 8.0 {
            self.new_page();
        }
        self.wrapped(text, SIZE_H2, LH_H2, true);
        self.gap(1.5);
        self.draw_line_at(self.y);
        self.gap(3.5);
    }

    fn h3(&mut self, text: &str) {
        self.wrapped(text, SIZE_H3, LH_H3, true);
    }

    fn body(&mut self, text: &str) {
        self.wrapped(text, SIZE_BODY, LH_BODY, false);
    }

    fn small(&mut self, text: &str) {
        self.wrapped(text, SIZE_SMALL, LH_SMALL, false);
    }

    fn bullet(&mut self, text: &str) {
        self.wrapped(&format!("- {text}"), SIZE_BODY, LH_BODY, false);
    }

    fn paragraph(&mut self, text: &str) {
        self.wrapped(text, SIZE_BODY, LH_BODY, false);
    }

    fn paragraph_size(&mut self, text: &str, size: f32, lh: f32) {
        self.wrapped(text, size, lh, false);
    }

    fn wrapped(&mut self, text: &str, size: f32, lh: f32, bold: bool) {
        let cpl = chars_per_line(size, BODY_W);
        for line in wrap_text(text, cpl) {
            if self.y < MARGIN_B + lh {
                self.new_page();
            }
            self.emit_text(&line, size, bold);
            self.advance(lh);
        }
    }

    fn insert_image(&mut self, path: &Path) -> Result<(), String> {
        const IMG_MAX_H: f32 = 100.0; // mm
        const IMG_MAX_W: f32 = BODY_W;

        let dyn_img = printpdf::image_crate::open(path)
            .map_err(|e| format!("open image: {e}"))?;

        let orig_w = dyn_img.width() as f32;
        let orig_h = dyn_img.height() as f32;
        // Assume 96 DPI for screen captures
        let mm_per_px = 25.4_f32 / 96.0;
        let w_mm = (orig_w * mm_per_px).min(IMG_MAX_W);
        let aspect = orig_h / orig_w;
        let h_mm = (w_mm * aspect).min(IMG_MAX_H);
        // If height was capped, recalculate width
        let (final_w, final_h) = if w_mm * aspect > IMG_MAX_H {
            (IMG_MAX_H / aspect, IMG_MAX_H)
        } else {
            (w_mm, h_mm)
        };

        if self.y - final_h < MARGIN_B {
            self.new_page();
        }

        // scale factors: image is placed 1px = 1pt by default in printpdf,
        // so we need scale_x = final_w_pt / native_w_pt
        // native width in pt at 300 dpi = orig_w * (72/300) pt
        // We want final_w mm = final_w * 2.835 pt
        let pt_per_mm = 2.835_f32;
        let native_w_pt = orig_w * (72.0 / 300.0); // printpdf default: 300 dpi
        let native_h_pt = orig_h * (72.0 / 300.0);
        let target_w_pt = final_w * pt_per_mm;
        let target_h_pt = final_h * pt_per_mm;

        let image = printpdf::Image::from_dynamic_image(&dyn_img);
        let transform = ImageTransform {
            translate_x: Some(Mm(MARGIN_L)),
            translate_y: Some(Mm(self.y - final_h)),
            scale_x: Some(target_w_pt / native_w_pt),
            scale_y: Some(target_h_pt / native_h_pt),
            ..Default::default()
        };
        image.add_to_layer(self.layer(), transform);
        self.advance(final_h + 2.0);
        Ok(())
    }
}

// ── Utilities ──────────────────────────────────────────────────────────────────

fn sanitize_title(title: &str) -> String {
    if title.is_empty() {
        "Design Document".to_string()
    } else {
        title.chars().take(80).collect()
    }
}

fn fmt_duration(secs: f64) -> String {
    let s = secs as u64;
    if s < 60 {
        format!("{s}s")
    } else if s < 3600 {
        format!("{}m {:02}s", s / 60, s % 60)
    } else {
        format!("{}h {:02}m", s / 3600, (s % 3600) / 60)
    }
}

/// Estimate characters per line based on font size (pt) and line width (mm).
/// Average glyph width ≈ size_pt * 0.55, 1pt ≈ 0.3528 mm.
fn chars_per_line(size_pt: f32, width_mm: f32) -> usize {
    let avg_glyph_mm = size_pt * 0.55 * 0.3528;
    if avg_glyph_mm <= 0.0 { return 80; }
    (width_mm / avg_glyph_mm).floor() as usize
}

/// Simple greedy word-wrap.
fn wrap_text(text: &str, max_chars: usize) -> Vec<String> {
    if max_chars == 0 { return vec![text.to_string()]; }
    let mut lines = Vec::new();
    for para in text.split('\n') {
        if para.trim().is_empty() { continue; }
        let mut cur = String::new();
        for word in para.split_whitespace() {
            if cur.is_empty() {
                cur.push_str(word);
            } else if cur.len() + 1 + word.len() <= max_chars {
                cur.push(' ');
                cur.push_str(word);
            } else {
                lines.push(cur.clone());
                cur = word.to_string();
            }
        }
        if !cur.is_empty() { lines.push(cur); }
    }
    if lines.is_empty() { lines.push(String::new()); }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brief_provider::BriefDoc;

    #[test]
    fn wrap_basic() {
        let lines = wrap_text("hello world foo bar baz qux long line here yes", 15);
        for l in &lines { assert!(l.len() <= 20, "Line too long: '{l}'"); }
        assert!(!lines.is_empty());
    }

    #[test]
    fn wrap_single_long_word() {
        let lines = wrap_text("superlongword", 5);
        assert!(!lines.is_empty());
        assert!(lines[0].contains("superlongword"));
    }

    #[test]
    fn fmt_duration_seconds() {
        assert_eq!(fmt_duration(45.0), "45s");
        assert_eq!(fmt_duration(90.0), "1m 30s");
        assert_eq!(fmt_duration(3661.0), "1h 01m");
    }

    #[test]
    fn normalize_timestamp_fixes_mm_ss_bug() {
        assert_eq!(normalize_timestamp_display("08:55:00"), "00:08:55");
        assert_eq!(normalize_timestamp_display("01:30:00"), "01:30:00");
    }

    #[test]
    fn render_pdf_creates_file() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("test.brief.pdf");
        let doc = BriefDoc {
            title: "Test Meeting".to_string(),
            summary: "This is a test Кириллица summary with Cyrillic text.".to_string(),
            decisions: vec![crate::brief_provider::BriefItem {
                text: "Use Rust for the backend".to_string(),
                timestamp: "00:05:00".to_string(),
            }],
            ..Default::default()
        };
        let result = render_brief_pdf(&doc, &[], &out);
        assert!(result.is_ok(), "PDF render failed: {:?}", result);
        assert!(out.exists(), "PDF not created");
        assert!(out.metadata().unwrap().len() > 1024, "PDF too small");
    }

    #[test]
    fn render_fdd_pdf_with_br_screenshot_refs_missing_frame() {
        // BR references frame index 0, but frames slice is empty.
        // PDF must render without error (graceful missing image handling).
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("test.fdd.pdf");
        let doc = BriefDoc {
            doc_mode: BriefDocMode::Fdd,
            title: "FDD with inline screenshot refs".to_string(),
            business_rules: vec![crate::brief_provider::BriefBusinessRule {
                id: "BR-01".to_string(),
                text: "Invoice total must equal sum of lines".to_string(),
                timestamp: "00:05:00".to_string(),
                screenshot_refs: vec![0],
            }],
            ..Default::default()
        };
        let result = render_brief_pdf(&doc, &[], &out);
        assert!(result.is_ok(), "PDF render failed: {:?}", result);
        assert!(out.exists(), "PDF not created");
    }

    #[test]
    fn appendix_excludes_inline_frames() {
        // Frame index 0 is referenced by BR-01 → should not appear in appendix.
        // Frame index 1 is unbound → should appear in appendix.
        // We render with two frames but frame 0 path is missing (graceful skip inline),
        // frame 1 path is also missing but we just check the PDF renders without panic.
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("test.fdd2.pdf");
        let frames = vec![
            crate::brief_provider::BriefFrame {
                index: 0,
                seconds: 10.0,
                label: "bound_frame".to_string(),
                path: std::path::PathBuf::from("/nonexistent/frame0.png"),
                caption: "".to_string(),
            },
            crate::brief_provider::BriefFrame {
                index: 1,
                seconds: 60.0,
                label: "unbound_frame".to_string(),
                path: std::path::PathBuf::from("/nonexistent/frame1.png"),
                caption: "Free screenshot".to_string(),
            },
        ];
        let doc = BriefDoc {
            doc_mode: BriefDocMode::Fdd,
            title: "Appendix filter test".to_string(),
            business_rules: vec![crate::brief_provider::BriefBusinessRule {
                id: "BR-01".to_string(),
                text: "Test rule".to_string(),
                timestamp: "00:00:10".to_string(),
                screenshot_refs: vec![0],
            }],
            ..Default::default()
        };
        let result = render_brief_pdf(&doc, &frames, &out);
        assert!(result.is_ok(), "PDF render failed: {:?}", result);
    }
}
