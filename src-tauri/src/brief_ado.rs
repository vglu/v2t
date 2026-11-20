//! Harvest Azure DevOps / work-item ids from transcript text and captions.
//! Only explicit numeric ids — never invent. No extra crate deps.

use crate::brief_provider::{BriefDoc, BriefGlossaryEntry, BriefWorkItem};

/// Extract ADO-style work item ids (6–8 digits), optionally prefixed with `#`.
pub fn harvest_ado_ids(text: &str) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    let lower = text.to_ascii_lowercase();
    let bytes = text.as_bytes();

    // 1) All `#NNNNNN` forms
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'#' && i + 1 < bytes.len() && bytes[i + 1].is_ascii_digit() {
            let start = i + 1;
            let mut j = start;
            while j < bytes.len() && bytes[j].is_ascii_digit() {
                j += 1;
            }
            let len = j - start;
            if (6..=8).contains(&len) && !is_alnum_byte(bytes, j) {
                push_unique(&mut found, &text[start..j]);
            }
            i = j;
        } else {
            i += 1;
        }
    }

    // 2) Contextual keywords → scan ahead ~48 chars for a bare 6–8 digit id
    const KEYWORDS: &[&str] = &[
        "work item",
        "workitem",
        "user story",
        "requirement",
        "ado ",
        "ado\t",
        "ado:",
        "ado#",
        " pbi ",
        " bug ",
        " wi ",
    ];
    for kw in KEYWORDS {
        let mut search_from = 0usize;
        while let Some(rel) = lower[search_from..].find(kw) {
            let at = search_from + rel + kw.len();
            if let Some(id) = first_id_in_window(text, at, 48) {
                push_unique(&mut found, &id);
            }
            search_from = at;
        }
    }

    found
}

fn first_id_in_window(text: &str, from: usize, window: usize) -> Option<String> {
    let bytes = text.as_bytes();
    let end = (from + window).min(bytes.len());
    let mut i = from;
    while i < end {
        if bytes[i] == b'#' {
            i += 1;
            continue;
        }
        if bytes[i].is_ascii_digit() {
            let start = i;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            let len = i - start;
            let prev_ok = start == 0 || !bytes[start - 1].is_ascii_alphanumeric();
            if (6..=8).contains(&len) && !is_alnum_byte(bytes, i) && prev_ok {
                return Some(text[start..i].to_string());
            }
            continue;
        }
        i += 1;
    }
    None
}

fn is_alnum_byte(bytes: &[u8], idx: usize) -> bool {
    bytes
        .get(idx)
        .map(|b| b.is_ascii_alphanumeric())
        .unwrap_or(false)
}

fn push_unique(out: &mut Vec<String>, id: &str) {
    if !out.iter().any(|x| x == id) {
        out.push(id.to_string());
    }
}

/// Merge harvested ids into `doc.work_items` and glossary (no duplicates, no invention).
pub fn merge_work_items_into_doc(doc: &mut BriefDoc, ids: &[String], source: &str) {
    for id in ids {
        if doc.work_items.iter().any(|w| w.id == *id) {
            continue;
        }
        doc.work_items.push(BriefWorkItem {
            id: id.clone(),
            title: String::new(),
            source: source.to_string(),
        });
        if !doc.glossary.iter().any(|g| g.term.contains(id)) {
            doc.glossary.push(BriefGlossaryEntry {
                term: format!("ADO #{id}"),
                meaning: "Azure DevOps work item referenced in the meeting".to_string(),
                source: source.to_string(),
                type_format: "Work item id".to_string(),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn harvests_hash_and_contextual_ids_from_arb_style_text() {
        let text = r#"
            Looking at ADO work item 1129017 for the decoupling hotfix.
            Also #1125575 false-synced pay statements.
            Requirement 1129517 Impact analysis.
            Ignore short 12345 and long 12345678901.
        "#;
        let ids = harvest_ado_ids(text);
        assert!(ids.contains(&"1129017".to_string()), "{ids:?}");
        assert!(ids.contains(&"1125575".to_string()), "{ids:?}");
        assert!(ids.contains(&"1129517".to_string()), "{ids:?}");
        assert!(!ids.iter().any(|id| id == "12345"));
        assert!(!ids.iter().any(|id| id.len() > 8));
    }

    #[test]
    fn merge_dedupes_and_adds_glossary() {
        let mut doc = BriefDoc::default();
        merge_work_items_into_doc(&mut doc, &["1129017".into()], "transcript");
        merge_work_items_into_doc(&mut doc, &["1129017".into(), "1125575".into()], "caption");
        assert_eq!(doc.work_items.len(), 2);
        assert_eq!(doc.work_items[0].id, "1129017");
        assert_eq!(doc.work_items[1].id, "1125575");
        assert!(doc.glossary.iter().any(|g| g.term.contains("1129017")));
    }
}
