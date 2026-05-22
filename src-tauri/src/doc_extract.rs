//! Offline text extraction from PDF and DOCX files.

use std::io::Read;
use std::path::Path;

/// Extract text from a PDF. Returns `None` if no text layer found (< 100 chars total = treat as scan).
pub fn extract_pdf_text(path: &Path) -> Result<Option<String>, String> {
    let doc = lopdf::Document::load(path).map_err(|e| format!("Failed to load PDF: {e}"))?;
    let pages: Vec<u32> = doc.get_pages().keys().copied().collect();

    let mut all_text = String::new();
    for page_num in &pages {
        match doc.extract_text(&[*page_num]) {
            Ok(text) => {
                if !text.is_empty() {
                    if !all_text.is_empty() {
                        all_text.push('\n');
                    }
                    all_text.push_str(&text);
                }
            }
            Err(_) => {
                // Page has no text layer — skip silently
            }
        }
    }

    if all_text.trim().len() < 100 {
        return Ok(None);
    }

    Ok(Some(all_text))
}

/// Extract text from a `.docx` file (ZIP + `word/document.xml` `<w:t>` nodes).
pub fn extract_docx_text(path: &Path) -> Result<String, String> {
    let file =
        std::fs::File::open(path).map_err(|e| format!("Failed to open docx: {e}"))?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|e| format!("Not a valid docx (ZIP): {e}"))?;

    let mut xml_content = String::new();
    {
        let mut entry = archive
            .by_name("word/document.xml")
            .map_err(|e| format!("word/document.xml not found in docx: {e}"))?;
        entry
            .read_to_string(&mut xml_content)
            .map_err(|e| format!("Failed to read word/document.xml: {e}"))?;
    }

    // Extract all <w:t> element content with a simple string scan (no XML crate).
    let mut text_parts: Vec<String> = Vec::new();
    let mut remaining = xml_content.as_str();

    while let Some(start_pos) = remaining.find("<w:t") {
        remaining = &remaining[start_pos..];
        // Find the end of the opening tag (could be <w:t> or <w:t xml:space="preserve">)
        let Some(tag_end) = remaining.find('>') else {
            break;
        };
        remaining = &remaining[tag_end + 1..];
        // Self-closing tag: <w:t/>
        if remaining.starts_with('/') || tag_end > 0 && remaining[..tag_end].ends_with('/') {
            continue;
        }
        let Some(close_pos) = remaining.find("</w:t>") else {
            break;
        };
        let content = &remaining[..close_pos];
        if !content.is_empty() {
            text_parts.push(content.to_string());
        }
        remaining = &remaining[close_pos + 6..];
    }

    let raw = text_parts.join("\n");

    // Strip consecutive blank lines
    let mut result = String::new();
    let mut prev_blank = false;
    for line in raw.lines() {
        let blank = line.trim().is_empty();
        if blank && prev_blank {
            continue;
        }
        if !result.is_empty() {
            result.push('\n');
        }
        result.push_str(line);
        prev_blank = blank;
    }

    Ok(result)
}
