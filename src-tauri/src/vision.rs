//! Vision/OCR pipeline using Gemini API for images and scanned PDFs/DOCX.

use base64::Engine;
use chrono::Utc;
use serde::Serialize;
use std::fs;
use std::path::Path;
use tokio_util::sync::CancellationToken;

use crate::doc_extract;
use crate::job::{require_output_dir, ProcessQueueItemOutcome};
use crate::output_template;
use crate::progress::{JobEvent, QueueJobProgressEvent, SinkHandle};
use crate::settings::{AppSettings, VisionMode};

const GEMINI_BASE: &str = "https://generativelanguage.googleapis.com/v1beta";

const MODEL_SUCCESSORS: &[(&str, &str)] = &[
    ("gemini-pro", "gemini-2.0-flash"),
    ("gemini-1.0-pro", "gemini-2.0-flash"),
    ("gemini-1.5-flash", "gemini-2.5-flash"),
    ("gemini-1.5-pro", "gemini-2.5-flash"),
    ("gemini-1.5-flash-8b", "gemini-2.5-flash"),
];

const MAX_INLINE_BYTES: u64 = 15 * 1024 * 1024; // 15 MB

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelValidationResult {
    pub is_valid: bool,
    pub suggested_replacement: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CostEstimate {
    pub image_count: u32,
    pub flash_cost_usd: f64,
    pub flash_lite_cost_usd: f64,
    pub free_tier_seconds: u64,
}

/// Returns `true` if the source path has an extension that indicates a vision/OCR input.
pub fn is_vision_input(source: &str) -> bool {
    let ext = Path::new(source)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();

    matches!(
        ext.as_str(),
        "jpg" | "jpeg" | "png" | "gif" | "webp" | "bmp" | "tiff" | "tif" | "pdf" | "docx"
    )
}

/// Returns the MIME type for a vision source, or `None` for offline-handled types (docx).
pub fn vision_mime_type(source: &str) -> Option<&'static str> {
    let ext = Path::new(source)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();

    match ext.as_str() {
        "jpg" | "jpeg" => Some("image/jpeg"),
        "png" => Some("image/png"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        "bmp" => Some("image/bmp"),
        "tiff" | "tif" => Some("image/tiff"),
        "pdf" => Some("application/pdf"),
        "docx" => None,
        _ => None,
    }
}

/// Pure cost calculation for a batch of images.
pub fn estimate_cost(image_count: u32) -> CostEstimate {
    CostEstimate {
        image_count,
        flash_cost_usd: (image_count as f64) * 0.00058,
        flash_lite_cost_usd: (image_count as f64) * 0.0001,
        free_tier_seconds: ((image_count as u64 + 9) / 10) * 60,
    }
}

/// Validate a Gemini model name against the API. Fails open on network errors.
pub async fn validate_model(
    http: &reqwest::Client,
    api_key: &str,
    model: &str,
) -> ModelValidationResult {
    let url = format!("{GEMINI_BASE}/models/{model}?key={api_key}");

    let resp = match http.get(&url).send().await {
        Ok(r) => r,
        Err(_) => {
            // Connection error — fail open
            return ModelValidationResult {
                is_valid: true,
                suggested_replacement: None,
                error: None,
            };
        }
    };

    match resp.status().as_u16() {
        200 => ModelValidationResult {
            is_valid: true,
            suggested_replacement: None,
            error: None,
        },
        404 => {
            let successor = MODEL_SUCCESSORS
                .iter()
                .find(|(old, _)| *old == model)
                .map(|(_, new)| new.to_string());
            ModelValidationResult {
                is_valid: false,
                suggested_replacement: successor,
                error: Some("Model not found".to_string()),
            }
        }
        _ => {
            // Other HTTP error — fail open
            ModelValidationResult {
                is_valid: true,
                suggested_replacement: None,
                error: None,
            }
        }
    }
}

/// Send bytes to Gemini Vision API and return extracted text.
async fn ocr_bytes(
    http: &reqwest::Client,
    api_key: &str,
    model: &str,
    bytes: &[u8],
    mime_type: &str,
    language: Option<&str>,
) -> Result<String, String> {
    let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);

    let prompt_base = "Extract all text from this image/document. Return plain text only, preserving line breaks and paragraph structure. If there is no text visible, return an empty string.";
    let prompt = if let Some(lang) = language {
        format!("Language hint: {lang}. {prompt_base}")
    } else {
        prompt_base.to_string()
    };

    let body = serde_json::json!({
        "contents": [{
            "parts": [
                {"text": prompt},
                {"inline_data": {"mime_type": mime_type, "data": b64}}
            ]
        }]
    });

    let url = format!("{GEMINI_BASE}/models/{model}:generateContent?key={api_key}");

    let resp = http
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Gemini request failed: {e}"))?;

    let status = resp.status();
    if status.as_u16() == 429 {
        return Err(
            "Gemini rate limit hit (free tier). Wait and retry, or enable billing.".to_string(),
        );
    }
    if !status.is_success() {
        let body_excerpt = resp
            .text()
            .await
            .unwrap_or_default()
            .chars()
            .take(300)
            .collect::<String>();
        return Err(format!("Gemini error {status}: {body_excerpt}"));
    }

    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse Gemini response: {e}"))?;

    let text = json["candidates"][0]["content"]["parts"][0]["text"]
        .as_str()
        .unwrap_or("")
        .to_string();

    Ok(text)
}

fn emit_progress(sink: &SinkHandle, job_id: &str, phase: &str, message: &str) {
    sink.emit(JobEvent::QueueJobProgress(QueueJobProgressEvent {
        job_id: job_id.to_string(),
        phase: phase.to_string(),
        message: message.to_string(),
        subtask_index: None,
        subtask_total: None,
        subtask_percent: None,
    }));
}

/// Main entry point for Vision/OCR jobs. Routes image/document files through
/// Gemini Vision or offline extraction before the audio pipeline even starts.
#[allow(clippy::too_many_arguments)]
pub async fn run_vision_job(
    _app: &tauri::AppHandle,
    sink: &SinkHandle,
    job_id: &str,
    job_index: u32,
    source: &str,
    display_label: &str,
    settings: &AppSettings,
    cancel: &CancellationToken,
) -> Result<ProcessQueueItemOutcome, String> {
    emit_progress(sink, job_id, "prepare", "Detecting file type…");

    let out_dir = require_output_dir(settings)?;
    let path = Path::new(source);

    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();

    if cancel.is_cancelled() {
        return Err(crate::pipeline::JOB_CANCELLED_MSG.to_string());
    }

    let text: String = match ext.as_str() {
        "docx" => {
            // Offline extraction
            doc_extract::extract_docx_text(path)?
        }
        "pdf" => {
            match doc_extract::extract_pdf_text(path)? {
                Some(t) => t,
                None => {
                    // Scanned PDF — fall through to Gemini OCR
                    ocr_via_gemini(sink, job_id, source, path, settings, cancel).await?
                }
            }
        }
        _ => {
            // Image file — Gemini OCR
            ocr_via_gemini(sink, job_id, source, path, settings, cancel).await?
        }
    };

    emit_progress(sink, job_id, "save", "Writing transcript…");

    let date = Utc::now().format("%Y-%m-%d").to_string();
    let filename = output_template::format_output_filename(
        &settings.filename_template,
        display_label,
        &date,
        job_index,
        1,
        source,
        "txt",
    );
    let dest_path = out_dir.join(&filename);
    if let Some(parent) = dest_path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("create_dir_all: {e}"))?;
    }
    fs::write(&dest_path, text.as_bytes())
        .map_err(|e| format!("Failed to write transcript: {e}"))?;

    let transcript_path = dest_path
        .canonicalize()
        .map_err(|e| e.to_string())?
        .to_str()
        .ok_or("Transcript path UTF-8")?
        .to_string();

    let summary = format!("Saved: {transcript_path}");
    emit_progress(sink, job_id, "done", &summary);

    Ok(ProcessQueueItemOutcome::Done {
        transcript_path,
        summary,
    })
}

/// Internal helper: check settings, read file bytes, call Gemini OCR.
async fn ocr_via_gemini(
    sink: &SinkHandle,
    job_id: &str,
    source: &str,
    path: &Path,
    settings: &AppSettings,
    cancel: &CancellationToken,
) -> Result<String, String> {
    if settings.vision_mode != VisionMode::Gemini {
        return Err(
            "Vision/OCR is disabled. Enable it in Settings → Vision / OCR.".to_string(),
        );
    }

    let file_size = fs::metadata(source)
        .map_err(|e| format!("Cannot read file metadata: {e}"))?
        .len();
    if file_size > MAX_INLINE_BYTES {
        return Err("File too large for inline OCR (15 MB limit).".to_string());
    }

    let mime_type = vision_mime_type(source)
        .ok_or_else(|| format!("Unsupported file type for OCR: {source}"))?;

    if cancel.is_cancelled() {
        return Err(crate::pipeline::JOB_CANCELLED_MSG.to_string());
    }

    emit_progress(sink, job_id, "ocr", "Sending to Gemini Vision…");

    let bytes = fs::read(path).map_err(|e| format!("Failed to read file: {e}"))?;

    let http = reqwest::Client::new();
    ocr_bytes(
        &http,
        &settings.gemini_api_key,
        &settings.gemini_model,
        &bytes,
        mime_type,
        settings.language.as_deref(),
    )
    .await
}
