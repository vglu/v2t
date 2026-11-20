//! Write FDD / Brief as Markdown (video-to-spec style) next to the PDF.

use std::path::{Path, PathBuf};

use crate::brief_pdf::normalize_timestamp_display;
use crate::brief_provider::{BriefDoc, BriefFrame};
use crate::settings::BriefDocMode;

/// Write `{stem}.fdd.md` / `{stem}.brief.md` and copy frames into `screenshots/`.
///
/// Returns the markdown path. Screenshot paths inside the MD are relative
/// (`screenshots/01_label.png`) so the file opens cleanly next to that folder.
pub fn write_design_markdown(
    doc: &BriefDoc,
    frames: &[BriefFrame],
    out_dir: &Path,
    stem: &str,
) -> Result<PathBuf, String> {
    let suffix = doc.doc_mode.pdf_suffix();
    let md_path = out_dir.join(format!("{stem}.{suffix}.md"));
    let shots_dir = out_dir.join("screenshots");

    if !frames.is_empty() {
        std::fs::create_dir_all(&shots_dir)
            .map_err(|e| format!("create screenshots dir: {e}"))?;
    }

    let mut copied: Vec<(BriefFrame, String)> = Vec::new();
    for frame in frames {
        let safe = sanitize_file_stem(&frame.label);
        let name = format!("{:02}_{safe}.png", frame.index + 1);
        let dest = shots_dir.join(&name);
        if frame.path.is_file() {
            std::fs::copy(&frame.path, &dest)
                .map_err(|e| format!("copy screenshot {}: {e}", frame.path.display()))?;
            copied.push((frame.clone(), format!("screenshots/{name}")));
        }
    }

    let body = match doc.doc_mode {
        BriefDocMode::Fdd => render_fdd_md(doc, &copied),
        BriefDocMode::Brief => render_brief_md(doc, &copied),
    };

    std::fs::write(&md_path, body).map_err(|e| format!("write markdown: {e}"))?;
    Ok(md_path)
}

fn render_fdd_md(doc: &BriefDoc, frames: &[(BriefFrame, String)]) -> String {
    let mut o = String::new();
    let title = if doc.title.is_empty() {
        "Functional Specification"
    } else {
        doc.title.as_str()
    };
    o.push_str(&format!("# {title}\n\n"));
    if !doc.date.is_empty() {
        o.push_str(&format!("**Date:** {}\n\n", doc.date));
    }
    if !doc.source.is_empty() {
        o.push_str(&format!("**Source:** `{}`\n\n", doc.source));
    }
    if doc.has_transcript_gaps {
        o.push_str(
            "> Note: Transcript may contain ASR gaps. Verify decisions against the recording.\n\n",
        );
    }

    o.push_str("## 1. Purpose & Scope\n\n");
    let purpose = if !doc.purpose.is_empty() {
        doc.purpose.as_str()
    } else {
        doc.summary.as_str()
    };
    if purpose.is_empty() {
        o.push_str("_None stated._\n\n");
    } else {
        o.push_str(purpose);
        o.push_str("\n\n");
    }
    o.push_str("**In scope**\n\n");
    push_bullets(&mut o, &doc.scope_in);
    o.push_str("\n**Out of scope**\n\n");
    push_bullets(&mut o, &doc.scope_out);

    maybe_embed_near(&mut o, frames, 0);

    o.push_str("\n## 2. Glossary & Data Dictionary\n\n");
    if doc.glossary.is_empty() {
        o.push_str("_None stated._\n\n");
    } else {
        o.push_str("| Term | Meaning | Source | Type/Format |\n| --- | --- | --- | --- |\n");
        for g in &doc.glossary {
            o.push_str(&format!(
                "| {} | {} | {} | {} |\n",
                esc_cell(&g.term),
                esc_cell(&g.meaning),
                esc_cell(&g.source),
                esc_cell(&g.type_format)
            ));
        }
        o.push('\n');
    }

    o.push_str("## 3. Business Rules\n\n");
    if doc.business_rules.is_empty() {
        o.push_str("_None stated._\n\n");
    } else {
        for br in &doc.business_rules {
            let ts = ts_line(&br.timestamp);
            o.push_str(&format!("**{}**  \n{}{ts}\n\n", br.id, br.text));
            // Inline screenshots evidencing this rule
            for &ref_idx in &br.screenshot_refs {
                if let Some((frame, rel)) = frames.iter().find(|(f, _)| f.index == ref_idx) {
                    let alt = if frame.caption.is_empty() {
                        frame.label.replace('_', " ")
                    } else {
                        frame.caption.clone()
                    };
                    o.push_str(&format!("![{alt}]({rel})\n\n"));
                }
            }
        }
    }

    o.push_str("## 4. Calculations & Formulas\n\n");
    if doc.formulas.is_empty() {
        o.push_str("_None stated._\n\n");
    } else {
        for f in &doc.formulas {
            let name = if f.name.is_empty() { "Formula" } else { f.name.as_str() };
            o.push_str(&format!("### {name}\n\n"));
            if !f.formula.is_empty() {
                o.push_str("```\n");
                o.push_str(&f.formula);
                o.push_str("\n```\n\n");
            }
            if !f.inputs.is_empty() {
                o.push_str(&format!("**Inputs:** {}\n\n", f.inputs));
            }
            if !f.example.is_empty() {
                o.push_str(&format!("**Example:** {}\n\n", f.example));
            }
        }
    }

    o.push_str("## 5. Worked Examples\n\n");
    if doc.worked_examples.is_empty() {
        o.push_str("_None stated._\n\n");
    } else {
        for ex in &doc.worked_examples {
            let ts = ts_line(&ex.timestamp);
            o.push_str(&format!("### {}{ts}\n\n", if ex.title.is_empty() { "Example" } else { &ex.title }));
            for (i, step) in ex.steps.iter().enumerate() {
                o.push_str(&format!("{}. {step}\n", i + 1));
            }
            o.push('\n');
        }
    }

    o.push_str("## 6. Edge Cases & Validation\n\n");
    if doc.edge_cases.is_empty() {
        o.push_str("_None stated._\n\n");
    } else {
        o.push_str("| Case | Expected behavior |\n| --- | --- |\n");
        for ec in &doc.edge_cases {
            o.push_str(&format!(
                "| {} | {} |\n",
                esc_cell(&ec.case),
                esc_cell(&ec.expected)
            ));
        }
        o.push('\n');
    }

    o.push_str("## 7. Open Questions\n\n");
    if doc.open_questions.is_empty() {
        o.push_str("_None stated._\n\n");
    } else {
        for (i, q) in doc.open_questions.iter().enumerate() {
            let ts = ts_line(&q.timestamp);
            o.push_str(&format!("{}. {}{ts}\n", i + 1, q.text));
        }
        o.push('\n');
    }

    if !doc.action_items.is_empty() {
        o.push_str("## Action Items\n\n");
        for a in &doc.action_items {
            let ts = ts_line(&a.timestamp);
            o.push_str(&format!(
                "- {}{ts}  \n  Who: {} | Deadline: {}\n",
                a.what, a.who, a.deadline
            ));
        }
        o.push('\n');
    }

    if !doc.release_notes.is_empty() {
        o.push_str("## Release & Process Notes\n\n");
        push_bullets(&mut o, &doc.release_notes);
    }

    if !doc.risks.is_empty() {
        o.push_str("## Risks\n\n");
        for r in &doc.risks {
            o.push_str(&format!("- **[{}]** {}\n", r.severity.to_uppercase(), r.text));
        }
        o.push('\n');
    }

    if !frames.is_empty() {
        o.push_str("## Visual Appendix\n\n");
        for (frame, rel) in frames {
            o.push_str(&format!(
                "### [{}] {}\n\n",
                frame.timestamp_str(),
                frame.label.replace('_', " ")
            ));
            o.push_str(&format!(
                "![{}]({})\n\n",
                esc_cell(&frame.caption),
                rel
            ));
            if !frame.caption.is_empty() {
                o.push_str(&frame.caption);
                o.push_str("\n\n");
            }
        }
    }

    o
}

fn render_brief_md(doc: &BriefDoc, frames: &[(BriefFrame, String)]) -> String {
    let mut o = String::new();
    let title = if doc.title.is_empty() {
        "Meeting Brief"
    } else {
        doc.title.as_str()
    };
    o.push_str(&format!("# {title}\n\n"));
    if !doc.summary.is_empty() {
        o.push_str("## Summary\n\n");
        o.push_str(&doc.summary);
        o.push_str("\n\n");
    }
    if !doc.decisions.is_empty() {
        o.push_str("## Decisions\n\n");
        for d in &doc.decisions {
            o.push_str(&format!("- {}{}\n", d.text, ts_line(&d.timestamp)));
        }
        o.push('\n');
    }
    if !doc.action_items.is_empty() {
        o.push_str("## Action Items\n\n");
        for a in &doc.action_items {
            o.push_str(&format!(
                "- {}{} — Who: {} | Deadline: {}\n",
                a.what,
                ts_line(&a.timestamp),
                a.who,
                a.deadline
            ));
        }
        o.push('\n');
    }
    if !frames.is_empty() {
        o.push_str("## Screenshots\n\n");
        for (frame, rel) in frames {
            o.push_str(&format!("![{}]({})\n\n", frame.label, rel));
        }
    }
    o
}

fn maybe_embed_near(o: &mut String, frames: &[(BriefFrame, String)], index: usize) {
    if let Some((frame, rel)) = frames.get(index) {
        o.push_str(&format!(
            "\n![{}]({})\n\n[Video @ {}]\n\n",
            if frame.caption.is_empty() {
                frame.label.replace('_', " ")
            } else {
                frame.caption.clone()
            },
            rel,
            frame.timestamp_str()
        ));
    }
}

fn push_bullets(o: &mut String, items: &[String]) {
    if items.is_empty() {
        o.push_str("_None stated._\n");
        return;
    }
    for s in items {
        o.push_str(&format!("- {s}\n"));
    }
}

fn ts_line(timestamp: &str) -> String {
    let n = normalize_timestamp_display(timestamp);
    if n.is_empty() {
        String::new()
    } else {
        format!("  \n[Video @ {n}]")
    }
}

fn esc_cell(s: &str) -> String {
    s.replace('|', "\\|").replace('\n', " ")
}

fn sanitize_file_stem(label: &str) -> String {
    let clean: String = label
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect();
    let trimmed = clean.trim_matches('_');
    if trimmed.is_empty() {
        "frame".into()
    } else {
        trimmed.chars().take(40).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brief_pdf::normalize_timestamp_display;

    #[test]
    fn normalizes_mm_ss_as_hh_mm_ss_bug() {
        assert_eq!(normalize_timestamp_display("08:55:00"), "00:08:55");
        assert_eq!(normalize_timestamp_display("00:08:55"), "00:08:55");
        assert_eq!(normalize_timestamp_display("01:30:00"), "01:30:00");
        assert_eq!(normalize_timestamp_display("9:36"), "00:09:36");
    }
}
