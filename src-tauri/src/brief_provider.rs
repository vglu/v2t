//! BriefProvider: pluggable LLM abstraction for the Brief (Design Doc) pipeline.
//!
//! Pass 1 — detect_moments: transcript text → list of visual anchor timestamps.
//! Pass 2 — generate_doc: transcript + frame images → structured design document.

use std::path::PathBuf;

use base64::Engine;
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use crate::settings::BriefDocMode;

const GEMINI_BASE: &str = "https://generativelanguage.googleapis.com/v1beta";

// ── Domain types ─────────────────────────────────────────────────────────────

/// A timestamped visual moment worth screenshotting (output of pass 1).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BriefAnchor {
    /// Position in the recording (seconds from start).
    pub seconds: f64,
    /// Short snake_case label, e.g. "architecture_diagram".
    pub label: String,
    /// Why this moment is relevant (for the LLM prompt; kept for diagnostics).
    pub reason: String,
    /// Relative importance 1–5. Higher = keep when cap is reached.
    pub importance: u8,
}

/// One extracted video frame, ready for the LLM vision call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BriefFrame {
    pub index: usize,
    /// Seconds from start, as extracted.
    pub seconds: f64,
    pub label: String,
    pub path: PathBuf,
    /// Brief LLM-generated caption placed in the Visual appendix.
    pub caption: String,
}

impl BriefFrame {
    pub fn timestamp_str(&self) -> String {
        let total = self.seconds as u64;
        format!("{:02}:{:02}:{:02}", total / 3600, (total % 3600) / 60, total % 60)
    }
}

/// Structured output of the design-doc generation (pass 2).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BriefDoc {
    /// Which PDF layout / schema was requested.
    #[serde(default)]
    pub doc_mode: BriefDocMode,
    pub title: String,
    pub date: String,
    pub duration_secs: f64,
    pub source: String,
    pub summary: String,
    pub attendees: Vec<String>,
    pub decisions: Vec<BriefItem>,
    pub action_items: Vec<BriefActionItem>,
    pub open_questions: Vec<BriefItem>,
    pub scope_in: Vec<String>,
    pub scope_out: Vec<String>,
    pub acceptance_criteria: Vec<String>,
    pub risks: Vec<BriefRisk>,
    /// FDD: purpose & scope narrative (1–3 paragraphs).
    #[serde(default)]
    pub purpose: String,
    /// FDD: glossary / data dictionary.
    #[serde(default)]
    pub glossary: Vec<BriefGlossaryEntry>,
    /// FDD: numbered business rules.
    #[serde(default)]
    pub business_rules: Vec<BriefBusinessRule>,
    /// FDD: calculations & formulas.
    #[serde(default)]
    pub formulas: Vec<BriefFormula>,
    /// FDD: end-to-end worked examples.
    #[serde(default)]
    pub worked_examples: Vec<BriefWorkedExample>,
    /// FDD: edge cases & validation.
    #[serde(default)]
    pub edge_cases: Vec<BriefEdgeCase>,
    /// FDD: release / process notes.
    #[serde(default)]
    pub release_notes: Vec<String>,
    /// FDD: Azure DevOps / work-item ids explicitly mentioned or shown.
    #[serde(default)]
    pub work_items: Vec<BriefWorkItem>,
    /// LLM-provided captions for each screenshot in the visual appendix.
    pub screenshot_annotations: Vec<BriefScreenshotAnnotation>,
    /// Warning shown when video is unavailable and screenshots are skipped.
    pub no_video_warning: Option<String>,
    /// True when transcript had detectable ASR gaps / hallucination loops.
    pub has_transcript_gaps: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BriefGlossaryEntry {
    pub term: String,
    pub meaning: String,
    pub source: String,
    pub type_format: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BriefBusinessRule {
    pub id: String,
    pub text: String,
    pub timestamp: String,
    /// Frame indices from extracted screenshots that visually evidence this rule.
    /// Populated by the LLM during Pass 2; [] when no screenshot directly evidences the rule.
    #[serde(default)]
    pub screenshot_refs: Vec<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BriefFormula {
    pub name: String,
    pub formula: String,
    pub inputs: String,
    pub example: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BriefWorkedExample {
    pub title: String,
    pub steps: Vec<String>,
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BriefEdgeCase {
    pub case: String,
    pub expected: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BriefWorkItem {
    /// Numeric ADO id, e.g. "1129017".
    pub id: String,
    /// Optional title if stated; else empty.
    #[serde(default)]
    pub title: String,
    /// Where the id was seen: transcript, screenshot caption, etc.
    #[serde(default)]
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BriefItem {
    pub text: String,
    /// "HH:MM:SS" or empty if not found in transcript.
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BriefActionItem {
    pub what: String,
    pub who: String,
    pub deadline: String,
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BriefRisk {
    pub text: String,
    pub severity: String, // "high" | "medium" | "low"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BriefScreenshotAnnotation {
    pub frame_index: usize,
    pub caption: String,
}

// ── Trait ─────────────────────────────────────────────────────────────────────

#[async_trait::async_trait]
pub trait BriefProvider: Send + Sync {
    /// Pass 1: given a transcript, return timestamps of moments worth screenshotting.
    async fn detect_moments(
        &self,
        transcript: &str,
        min_anchors: usize,
        max_anchors: usize,
        cancel: &CancellationToken,
    ) -> Result<Vec<BriefAnchor>, String>;

    /// Pass 2: given transcript + (optionally) frames, return a structured design doc.
    async fn generate_doc(
        &self,
        transcript: &str,
        frames: &[BriefFrame],
        source_name: &str,
        cancel: &CancellationToken,
    ) -> Result<BriefDoc, String>;

    /// FDD caption-gate: decide which extracted frames are product-relevant.
    /// One decision per input frame (same order). Default: keep all.
    async fn gate_frames(
        &self,
        frames: &[BriefFrame],
        cancel: &CancellationToken,
    ) -> Result<Vec<FrameGateDecision>, String> {
        let _ = cancel;
        Ok(frames
            .iter()
            .map(|f| FrameGateDecision {
                keep: true,
                caption: f.caption.clone(),
                reason: String::new(),
            })
            .collect())
    }
}

/// Per-frame decision from the FDD caption-gate.
#[derive(Debug, Clone)]
pub struct FrameGateDecision {
    pub keep: bool,
    pub caption: String,
    pub reason: String,
}

// ── Gemini implementation ─────────────────────────────────────────────────────

pub struct GeminiProvider {
    pub http: reqwest::Client,
    pub api_key: String,
    pub model: String,
    pub mode: BriefDocMode,
}

impl GeminiProvider {
    pub fn new(api_key: String, model: String, mode: BriefDocMode) -> Self {
        Self {
            http: reqwest::Client::new(),
            api_key,
            model,
            mode,
        }
    }

    async fn post_json(
        &self,
        endpoint: &str,
        body: serde_json::Value,
        cancel: &CancellationToken,
    ) -> Result<serde_json::Value, String> {
        let url = format!("{GEMINI_BASE}/models/{}{endpoint}?key={}", self.model, self.api_key);
        let req = self.http.post(&url).json(&body);

        let resp = tokio::select! {
            _ = cancel.cancelled() => return Err(crate::pipeline::JOB_CANCELLED_MSG.to_string()),
            r = req.send() => r.map_err(|e| format!("Gemini request failed: {e}"))?,
        };

        let status = resp.status();
        if status.as_u16() == 429 {
            return Err("Gemini rate limit hit. Wait and retry, or enable billing.".to_string());
        }
        if !status.is_success() {
            let excerpt: String = resp
                .text()
                .await
                .unwrap_or_default()
                .chars()
                .take(400)
                .collect();
            return Err(format!("Gemini error {status}: {excerpt}"));
        }
        resp.json::<serde_json::Value>()
            .await
            .map_err(|e| format!("Failed to parse Gemini response: {e}"))
    }

    fn extract_text(resp: &serde_json::Value) -> Result<String, String> {
        resp["candidates"][0]["content"]["parts"][0]["text"]
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| "Gemini returned no text in response".to_string())
    }
}

// Pass-1 system prompt (adapted from video-to-spec anchors.py)
const ANCHOR_SYSTEM: &str =
    "You are a meeting analyst preparing visual documentation from a recorded developer meeting. \
     You read a timestamped transcript and decide exactly which moments need a screenshot of \
     the presenter's shared screen. You reply with JSON only.";

const ANCHOR_SYSTEM_FDD: &str =
    "You are a requirements analyst selecting screenshots for a Functional Design Document. \
     Prefer product UI, ADO work items, system parameters, diagrams, tables, and demos. \
     Cover the FULL meeting timeline — early setup alone is not enough; include mid and late \
     discussion when the presenter shows AL / F&O / ADO screens. Aim for dense coverage \
     (about one screenshot per distinct product topic). \
     Skip Teams/Meet chrome: attendee lists, calendars, SIS dashboards, chat panels, \
     idle waiting, and 'let me share my screen' moments. You reply with JSON only.";

// Pass-2 system prompts
const DOC_SYSTEM_BRIEF: &str =
    "You are a senior technical business analyst. You turn recorded developer meetings into \
     structured meeting design briefs precise enough that a developer can act on them without \
     watching the recording. Read values and decisions directly from the transcript and screenshots. \
     Never invent names, numbers, or decisions that are not explicitly stated. \
     If something is unclear, place it in Open Questions. \
     You reply with JSON only.";

const DOC_SYSTEM_FDD: &str =
    "You are a senior technical business analyst. You turn recorded requirements walkthroughs \
     into Functional Design Documents precise enough that a developer can implement from them \
     without watching the recording. Read values and formulas from screenshots; never invent a number. \
     Prefer testable business rules over meeting minutes. No filler, no restating the agenda. \
     If something is unclear, place it in Open Questions. You reply with JSON only.";

#[async_trait::async_trait]
impl BriefProvider for GeminiProvider {
    async fn detect_moments(
        &self,
        transcript: &str,
        min_anchors: usize,
        max_anchors: usize,
        cancel: &CancellationToken,
    ) -> Result<Vec<BriefAnchor>, String> {
        let system = match self.mode {
            BriefDocMode::Fdd => ANCHOR_SYSTEM_FDD,
            BriefDocMode::Brief => ANCHOR_SYSTEM,
        };
        let skip_extra = match self.mode {
            BriefDocMode::Fdd => {
                "Also skip: attendee rosters, calendar views, generic SIS/dashboard chrome, \
                 meeting chat sidebars, and any frame that does not show product UI or work items."
            }
            BriefDocMode::Brief => "",
        };
        let prompt = format!(
            r#"{system}

Pick between {min_anchors} and {max_anchors} moments in the transcript below where a screenshot
of the shared screen would be most valuable for a design document.

Choose timestamps where relevant content is ALREADY on screen and settled — not the moment
the presenter says "let me open this", but a few seconds later when it is visible.

Prioritise moments that show:
- architecture diagrams, system designs, component diagrams
- code that is being discussed or reviewed
- data models, schemas, or configuration
- UI mockups or demos being walked through
- metrics, numbers, or thresholds being referenced
- ADO / Azure DevOps work items and system parameters screens

Skip: greetings, audio checks, tangential discussion with nothing on screen.
{skip_extra}

Rules:
- timestamp must be HH:MM:SS from recording start and must fall inside the transcript
- For meetings under one hour, hours are almost always 00 (example: eight minutes 55s → 00:08:55, NEVER 08:55:00)
- label must be lower_snake_case under 40 chars describing the content
- order anchors chronologically

Respond with JSON only:
{{"anchors": [{{"timestamp": "HH:MM:SS", "label": "example_label", "reason": "why this matters", "importance": 3}}]}}

Transcript:
{transcript}
"#
        );

        let body = serde_json::json!({
            "contents": [{"parts": [{"text": prompt}]}],
            "generationConfig": {
                "responseMimeType": "application/json",
                "maxOutputTokens": 4096,
                "temperature": 0.1
            }
        });

        let resp = self.post_json(":generateContent", body, cancel).await?;
        let text = Self::extract_text(&resp)?;
        let parsed: serde_json::Value =
            serde_json::from_str(&text).map_err(|e| format!("Failed to parse anchor JSON: {e}\nRaw: {text}"))?;

        let anchors_raw = parsed["anchors"]
            .as_array()
            .ok_or("No 'anchors' array in response")?;

        let anchors: Vec<BriefAnchor> = anchors_raw
            .iter()
            .filter_map(|a| {
                let ts = a["timestamp"].as_str()?;
                let seconds = parse_timestamp(ts)?;
                Some(BriefAnchor {
                    seconds,
                    label: a["label"].as_str().unwrap_or("frame").to_string(),
                    reason: a["reason"].as_str().unwrap_or("").to_string(),
                    importance: a["importance"].as_u64().unwrap_or(3).clamp(1, 5) as u8,
                })
            })
            .collect();

        if anchors.is_empty() {
            return Err("LLM returned no valid anchors — check transcript quality".to_string());
        }
        Ok(anchors)
    }

    async fn generate_doc(
        &self,
        transcript: &str,
        frames: &[BriefFrame],
        source_name: &str,
        cancel: &CancellationToken,
    ) -> Result<BriefDoc, String> {
        let (system, schema, rules) = match self.mode {
            BriefDocMode::Fdd => (
                DOC_SYSTEM_FDD,
                FDD_SCHEMA,
                r#"- Write for a developer who was not in the meeting.
- business_rules: number as BR-01, BR-02, …; each rule must be a testable statement. \
  Include screenshot_refs: array of SCREENSHOT N indices (from the headers above) that visually \
  evidence this rule — e.g. show the UI field, work item, or table the rule describes. \
  Use [] when no screenshot directly shows the rule in context.
- glossary: every domain term / field / flag mentioned (Term | Meaning | Source | Type/Format).
- formulas: ALWAYS extract logical selection / status-mapping rules discussed in the meeting \
  as formulas (e.g. eligible = (IsSyncedERP==0) AND Status IN {{Completed, StatementCreated}}). \
  If a business rule implies a formula, put it here — do not leave formulas empty when BR text \
  contains selection logic. Include plain notation, inputs, and a short example.
- worked_examples: end-to-end paths the presenter demonstrated (at least one when a hotfix/path was walked).
- edge_cases: exceptions, limits, error handling as case → expected behavior.
- open_questions: ambiguities, contradictions, "we'll come back to that".
- action_items / release_notes: only when explicitly stated; who/deadline = "Not stated" if missing.
- Prefer tables of facts over meeting summary; do not invent numbers or ADO ids.
- Timestamps MUST be HH:MM:SS from recording start (e.g. 00:08:55), never MM:SS:00 mislabeled as hours.
- work_items: list every Azure DevOps / work-item id explicitly spoken or readable on screen \
  (e.g. 1129017). Never invent ids. title only if stated.
- screenshot_annotations: caption each frame; note if the frame is non-product chrome.
- "Screen is authority" when transcript and screenshot disagree on a readable value."#,
            ),
            BriefDocMode::Brief => (
                DOC_SYSTEM_BRIEF,
                BRIEF_SCHEMA,
                r#"- For decisions, action items, open questions, risks: timestamp "HH:MM:SS" when available; else "".
- action_items.who and .deadline: "Not stated" when not explicit — do NOT invent names.
- If a section has no items, use [].
- screenshot_annotations: 1–2 sentence caption per frame.
- "Screen is authority" for garbled names/numbers visible on screen.
- Never invent numbers, names, or decisions not stated in the meeting."#,
            ),
        };

        let intro = format!(
            r#"{system}

Source recording: {source_name}

Produce a JSON design document from the transcript and screenshots below.
Schema:
{schema}

Rules:
{rules}

Transcript:
{transcript}

"#
        );

        let mut parts: Vec<serde_json::Value> = vec![serde_json::json!({"text": intro})];

        for frame in frames {
            let header = format!(
                "--- SCREENSHOT {} ---\npath: {}\ntimestamp: {}\nlabel: {}\nexpected content: {}",
                frame.index,
                frame.path.file_name().and_then(|n| n.to_str()).unwrap_or("?"),
                frame.timestamp_str(),
                frame.label,
                frame.caption,
            );
            parts.push(serde_json::json!({"text": header}));

            // Encode image as base64
            match std::fs::read(&frame.path) {
                Ok(bytes) => {
                    let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
                    let mime = if frame.path.extension().and_then(|e| e.to_str()) == Some("jpg") {
                        "image/jpeg"
                    } else {
                        "image/png"
                    };
                    parts.push(serde_json::json!({
                        "inlineData": {"mimeType": mime, "data": b64}
                    }));
                }
                Err(e) => {
                    // Frame file missing — include a note, skip the image
                    parts.push(serde_json::json!({
                        "text": format!("[Screenshot not available: {e}]")
                    }));
                }
            }
        }

        parts.push(serde_json::json!({"text": "Now write the JSON design document. Return JSON only, no markdown fences."}));

        let body = serde_json::json!({
            "contents": [{"parts": parts}],
            "generationConfig": {
                "responseMimeType": "application/json",
                "maxOutputTokens": 16384,
                "temperature": 0.2
            }
        });

        let resp = self.post_json(":generateContent", body, cancel).await?;
        let text = Self::extract_text(&resp)?;

        // Strip markdown code fences if model wrapped output despite responseMimeType
        let clean = text.trim().trim_start_matches("```json").trim_start_matches("```").trim_end_matches("```").trim();
        let parsed: serde_json::Value =
            serde_json::from_str(clean).map_err(|e| format!("Failed to parse doc JSON: {e}\nRaw: {}", &clean[..clean.len().min(500)]))?;

        let mut doc = brief_doc_from_json(&parsed, source_name);
        doc.doc_mode = self.mode;
        Ok(doc)
    }

    async fn gate_frames(
        &self,
        frames: &[BriefFrame],
        cancel: &CancellationToken,
    ) -> Result<Vec<FrameGateDecision>, String> {
        if frames.is_empty() {
            return Ok(vec![]);
        }

        let intro = format!(
            r#"You classify screenshots from a recorded developer meeting for a Functional Design Document.

For each screenshot decide keep=true only if it shows product / engineering content:
- application UI, system parameters, forms, grids, timesheets
- Azure DevOps / work items, diagrams, schemas, code, spreadsheets with domain data

keep=false for meeting chrome only:
- attendee / participant lists, calendar, lobby, waiting
- Teams/Meet chat sidebars without product UI
- generic SIS/dashboard chrome with no product screen
- blank or "sharing screen" placeholders

Return JSON only:
{{"frames":[{{"frame_index":0,"keep":true,"reason":"short reason","caption":"1 sentence of what is visible"}}]}}

There are {n} screenshots (frame_index 0..{last}). Classify every one."#,
            n = frames.len(),
            last = frames.len().saturating_sub(1),
        );

        let mut parts: Vec<serde_json::Value> = vec![serde_json::json!({"text": intro})];
        for frame in frames {
            let header = format!(
                "--- FRAME {} ---\ntimestamp: {}\nlabel: {}",
                frame.index,
                frame.timestamp_str(),
                frame.label,
            );
            parts.push(serde_json::json!({"text": header}));
            match std::fs::read(&frame.path) {
                Ok(bytes) => {
                    let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
                    let mime = if frame.path.extension().and_then(|e| e.to_str()) == Some("jpg") {
                        "image/jpeg"
                    } else {
                        "image/png"
                    };
                    parts.push(serde_json::json!({
                        "inlineData": {"mimeType": mime, "data": b64}
                    }));
                }
                Err(e) => {
                    parts.push(serde_json::json!({
                        "text": format!("[Screenshot not available: {e}] — prefer keep=false")
                    }));
                }
            }
        }
        parts.push(serde_json::json!({
            "text": "Return the JSON classification now. No markdown fences."
        }));

        let body = serde_json::json!({
            "contents": [{"parts": parts}],
            "generationConfig": {
                "responseMimeType": "application/json",
                "maxOutputTokens": 4096,
                "temperature": 0.0
            }
        });

        let resp = self.post_json(":generateContent", body, cancel).await?;
        let text = Self::extract_text(&resp)?;
        let clean = text
            .trim()
            .trim_start_matches("```json")
            .trim_start_matches("```")
            .trim_end_matches("```")
            .trim();
        let parsed: serde_json::Value = serde_json::from_str(clean)
            .map_err(|e| format!("Failed to parse gate JSON: {e}\nRaw: {}", &clean[..clean.len().min(400)]))?;

        let mut decisions: Vec<FrameGateDecision> = frames
            .iter()
            .map(|f| FrameGateDecision {
                keep: true,
                caption: f.caption.clone(),
                reason: String::new(),
            })
            .collect();

        if let Some(arr) = parsed["frames"].as_array() {
            for item in arr {
                let Some(idx) = item["frame_index"].as_u64().map(|u| u as usize) else {
                    continue;
                };
                if idx >= decisions.len() {
                    continue;
                }
                if let Some(k) = item["keep"].as_bool() {
                    decisions[idx].keep = k;
                }
                if let Some(cap) = item["caption"].as_str() {
                    decisions[idx].caption = cap.to_string();
                }
                if let Some(reason) = item["reason"].as_str() {
                    decisions[idx].reason = reason.to_string();
                }
            }
        }

        // Merge heuristic on label + LLM caption/reason.
        for (i, frame) in frames.iter().enumerate() {
            let hay = format!(
                "{} {} {}",
                frame.label, decisions[i].caption, decisions[i].reason
            );
            if crate::brief_gate::looks_like_meeting_chrome(&hay, "")
                && !crate::brief_gate::looks_like_product_ui(&hay)
            {
                decisions[i].keep = false;
            }
            // Never drop frames the vision pass itself described as product UI.
            if !decisions[i].keep
                && crate::brief_gate::looks_like_product_ui(&format!(
                    "{} {}",
                    decisions[i].caption, decisions[i].reason
                ))
            {
                decisions[i].keep = true;
                decisions[i].reason = format!(
                    "kept: product content ({})",
                    decisions[i].reason
                );
            }
        }

        Ok(decisions)
    }
}

const BRIEF_SCHEMA: &str = r#"{
  "title": "string — meeting title inferred from discussion",
  "date": "string — YYYY-MM-DD if mentioned, else empty",
  "summary": "string — 2–4 sentence summary",
  "attendees": ["string — names mentioned, or [] if unknown"],
  "decisions": [{"text": "string", "timestamp": "HH:MM:SS or empty"}],
  "action_items": [{"what": "string", "who": "string or 'Not stated'", "deadline": "string or 'Not stated'", "timestamp": "HH:MM:SS or empty"}],
  "open_questions": [{"text": "string", "timestamp": "HH:MM:SS or empty"}],
  "scope_in": ["string — explicit items in scope"],
  "scope_out": ["string — explicit items out of scope"],
  "acceptance_criteria": ["string — explicit AC mentioned"],
  "risks": [{"text": "string", "severity": "high|medium|low"}],
  "screenshot_annotations": [{"frame_index": 0, "caption": "string — what is visible and why it matters"}]
}"#;

const FDD_SCHEMA: &str = r#"{
  "title": "string — Functional Specification: <subject>",
  "date": "string — YYYY-MM-DD if mentioned, else empty",
  "purpose": "string — 1–3 paragraphs: what is being built/changed",
  "scope_in": ["string"],
  "scope_out": ["string"],
  "glossary": [{"term": "string", "meaning": "string", "source": "string", "type_format": "string"}],
  "business_rules": [{"id": "BR-01", "text": "string — testable rule", "timestamp": "HH:MM:SS or empty", "screenshot_refs": [0]}],
  "formulas": [{"name": "string", "formula": "string", "inputs": "string", "example": "string"}],
  "worked_examples": [{"title": "string", "steps": ["string"], "timestamp": "HH:MM:SS or empty"}],
  "edge_cases": [{"case": "string", "expected": "string"}],
  "open_questions": [{"text": "string", "timestamp": "HH:MM:SS or empty"}],
  "action_items": [{"what": "string", "who": "string or 'Not stated'", "deadline": "string or 'Not stated'", "timestamp": "HH:MM:SS or empty"}],
  "release_notes": ["string — release/process notes explicitly stated"],
  "work_items": [{"id": "1129017", "title": "string or empty", "source": "transcript|screenshot"}],
  "risks": [{"text": "string", "severity": "high|medium|low"}],
  "screenshot_annotations": [{"frame_index": 0, "caption": "string — what is visible and why it matters"}]
}"#;

// ── Mock provider (for tests) ─────────────────────────────────────────────────

pub struct MockProvider {
    pub anchor_count: usize,
    /// If set, detect_moments returns this error.
    pub anchor_error: Option<String>,
    /// If set, generate_doc returns this error.
    pub doc_error: Option<String>,
}

impl MockProvider {
    pub fn ok(anchor_count: usize) -> Self {
        Self { anchor_count, anchor_error: None, doc_error: None }
    }

    pub fn failing_anchors(msg: &str) -> Self {
        Self { anchor_count: 0, anchor_error: Some(msg.to_string()), doc_error: None }
    }

    pub fn failing_doc(msg: &str) -> Self {
        Self { anchor_count: 2, anchor_error: None, doc_error: Some(msg.to_string()) }
    }
}

#[async_trait::async_trait]
impl BriefProvider for MockProvider {
    async fn detect_moments(
        &self,
        _transcript: &str,
        _min_anchors: usize,
        _max_anchors: usize,
        _cancel: &CancellationToken,
    ) -> Result<Vec<BriefAnchor>, String> {
        if let Some(err) = &self.anchor_error {
            return Err(err.clone());
        }
        Ok((0..self.anchor_count)
            .map(|i| BriefAnchor {
                seconds: (i as f64) * 60.0 + 10.0,
                label: format!("moment_{i:02}"),
                reason: format!("Mock anchor {i}"),
                importance: 3,
            })
            .collect())
    }

    async fn generate_doc(
        &self,
        transcript: &str,
        frames: &[BriefFrame],
        source_name: &str,
        _cancel: &CancellationToken,
    ) -> Result<BriefDoc, String> {
        if let Some(err) = &self.doc_error {
            return Err(err.clone());
        }
        let doc = BriefDoc {
            doc_mode: BriefDocMode::Brief,
            title: format!("Mock Brief for {source_name}"),
            date: "2026-11-20".to_string(),
            duration_secs: 0.0,
            source: source_name.to_string(),
            summary: format!("Mock summary. Transcript length: {} chars. Frames: {}.", transcript.len(), frames.len()),
            attendees: vec!["Person 1".to_string(), "Person 2".to_string()],
            decisions: vec![BriefItem {
                text: "Use Rust for the backend".to_string(),
                timestamp: "00:05:00".to_string(),
            }],
            action_items: vec![BriefActionItem {
                what: "Implement the feature".to_string(),
                who: "Not stated".to_string(),
                deadline: "Not stated".to_string(),
                timestamp: "00:10:00".to_string(),
            }],
            open_questions: vec![BriefItem {
                text: "What is the deployment target?".to_string(),
                timestamp: "".to_string(),
            }],
            scope_in: vec!["Backend API".to_string()],
            scope_out: vec!["Frontend".to_string()],
            acceptance_criteria: vec!["All endpoints return 200".to_string()],
            risks: vec![BriefRisk {
                text: "Tight timeline".to_string(),
                severity: "medium".to_string(),
            }],
            purpose: String::new(),
            glossary: vec![],
            business_rules: vec![],
            formulas: vec![],
            worked_examples: vec![],
            edge_cases: vec![],
            release_notes: vec![],
            work_items: vec![],
            screenshot_annotations: frames
                .iter()
                .map(|f| BriefScreenshotAnnotation {
                    frame_index: f.index,
                    caption: format!("Mock caption for frame {} at {}", f.index, f.timestamp_str()),
                })
                .collect(),
            no_video_warning: if frames.is_empty() {
                Some("No video available — visual appendix is empty.".to_string())
            } else {
                None
            },
            has_transcript_gaps: false,
        };
        Ok(doc)
    }
}

// ── Helpers ────────────────────────────────────────────────────────────────────

/// Parse "HH:MM:SS" or "MM:SS" into seconds.
pub fn parse_timestamp(ts: &str) -> Option<f64> {
    let parts: Vec<&str> = ts.trim().split(':').collect();
    match parts.as_slice() {
        [h, m, s] => {
            let h: f64 = h.parse().ok()?;
            let m: f64 = m.parse().ok()?;
            let s: f64 = s.parse().ok()?;
            Some(h * 3600.0 + m * 60.0 + s)
        }
        [m, s] => {
            let m: f64 = m.parse().ok()?;
            let s: f64 = s.parse().ok()?;
            Some(m * 60.0 + s)
        }
        _ => None,
    }
}

/// Clamp / repair LLM anchor times against the real video duration.
///
/// Models often emit `08:01:19` meaning **00:08:01** (minutes in the hours slot).
/// When the literal parse exceeds `duration`, reinterpret `HH:MM:SS` as
/// `HH*60 + MM + SS` seconds (same repair video-to-spec needs via range checks).
pub fn coerce_anchor_seconds(raw: f64, duration: f64) -> Option<f64> {
    if raw < 0.0 || duration <= 1.0 {
        return None;
    }
    let horizon = duration + 30.0;
    if raw <= horizon {
        return Some(raw.min((duration - 0.5).max(0.0)));
    }
    let total = raw as u64;
    let h = total / 3600;
    let m = (total % 3600) / 60;
    let s = total % 60;
    if h <= 59 && m <= 59 && s <= 59 {
        let alt = (h * 60 + m + s) as f64;
        if alt <= horizon {
            return Some(alt.min((duration - 0.5).max(0.0)));
        }
    }
    None
}

fn brief_doc_from_json(v: &serde_json::Value, source_name: &str) -> BriefDoc {
    let str_or_empty = |key: &str| v[key].as_str().unwrap_or("").to_string();

    let items = |key: &str| -> Vec<BriefItem> {
        v[key]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|item| {
                        let text = item["text"].as_str().or_else(|| item.as_str())?;
                        Some(BriefItem {
                            text: text.to_string(),
                            timestamp: item["timestamp"].as_str().unwrap_or("").to_string(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default()
    };

    let strings = |key: &str| -> Vec<String> {
        v[key]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|s| s.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
    };

    let action_items: Vec<BriefActionItem> = v["action_items"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|item| {
                    let what = item["what"].as_str()?;
                    Some(BriefActionItem {
                        what: what.to_string(),
                        who: item["who"].as_str().unwrap_or("Not stated").to_string(),
                        deadline: item["deadline"].as_str().unwrap_or("Not stated").to_string(),
                        timestamp: item["timestamp"].as_str().unwrap_or("").to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    let risks: Vec<BriefRisk> = v["risks"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|item| {
                    let text = item["text"].as_str()?;
                    Some(BriefRisk {
                        text: text.to_string(),
                        severity: item["severity"].as_str().unwrap_or("medium").to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    let screenshot_annotations: Vec<BriefScreenshotAnnotation> = v["screenshot_annotations"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|item| {
                    Some(BriefScreenshotAnnotation {
                        frame_index: item["frame_index"].as_u64()? as usize,
                        caption: item["caption"].as_str().unwrap_or("").to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    let glossary: Vec<BriefGlossaryEntry> = v["glossary"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|item| {
                    let term = item["term"].as_str()?;
                    Some(BriefGlossaryEntry {
                        term: term.to_string(),
                        meaning: item["meaning"].as_str().unwrap_or("").to_string(),
                        source: item["source"].as_str().unwrap_or("").to_string(),
                        type_format: item["type_format"]
                            .as_str()
                            .or_else(|| item["typeFormat"].as_str())
                            .unwrap_or("")
                            .to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    let business_rules: Vec<BriefBusinessRule> = v["business_rules"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .enumerate()
                .filter_map(|(i, item)| {
                    let text = item["text"].as_str()?;
                    let id = item["id"]
                        .as_str()
                        .map(str::to_string)
                        .unwrap_or_else(|| format!("BR-{:02}", i + 1));
                    Some(BriefBusinessRule {
                        id,
                        text: text.to_string(),
                        timestamp: item["timestamp"].as_str().unwrap_or("").to_string(),
                        screenshot_refs: item["screenshot_refs"]
                            .as_array()
                            .map(|arr| {
                                arr.iter()
                                    .filter_map(|x| x.as_u64().map(|u| u as usize))
                                    .collect()
                            })
                            .unwrap_or_default(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    let formulas: Vec<BriefFormula> = v["formulas"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|item| {
                    Some(BriefFormula {
                        name: item["name"].as_str().unwrap_or("").to_string(),
                        formula: item["formula"].as_str().unwrap_or("").to_string(),
                        inputs: item["inputs"].as_str().unwrap_or("").to_string(),
                        example: item["example"].as_str().unwrap_or("").to_string(),
                    })
                })
                .filter(|f| !f.formula.is_empty() || !f.name.is_empty())
                .collect()
        })
        .unwrap_or_default();

    let worked_examples: Vec<BriefWorkedExample> = v["worked_examples"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|item| {
                    let title = item["title"].as_str().unwrap_or("").to_string();
                    let steps = item["steps"]
                        .as_array()
                        .map(|s| {
                            s.iter()
                                .filter_map(|x| x.as_str().map(str::to_string))
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();
                    if title.is_empty() && steps.is_empty() {
                        return None;
                    }
                    Some(BriefWorkedExample {
                        title,
                        steps,
                        timestamp: item["timestamp"].as_str().unwrap_or("").to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    let edge_cases: Vec<BriefEdgeCase> = v["edge_cases"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|item| {
                    let case = item["case"].as_str()?;
                    Some(BriefEdgeCase {
                        case: case.to_string(),
                        expected: item["expected"].as_str().unwrap_or("").to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    BriefDoc {
        doc_mode: BriefDocMode::default(),
        title: str_or_empty("title"),
        date: str_or_empty("date"),
        duration_secs: 0.0,
        source: source_name.to_string(),
        summary: str_or_empty("summary"),
        attendees: strings("attendees"),
        decisions: items("decisions"),
        action_items,
        open_questions: items("open_questions"),
        scope_in: strings("scope_in"),
        scope_out: strings("scope_out"),
        acceptance_criteria: strings("acceptance_criteria"),
        risks,
        purpose: str_or_empty("purpose"),
        glossary,
        business_rules,
        formulas,
        worked_examples,
        edge_cases,
        release_notes: strings("release_notes"),
        work_items: {
            v["work_items"]
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .filter_map(|item| {
                            let id = item["id"].as_str()?.to_string();
                            if id.is_empty() {
                                return None;
                            }
                            Some(BriefWorkItem {
                                id,
                                title: item["title"].as_str().unwrap_or("").to_string(),
                                source: item["source"].as_str().unwrap_or("llm").to_string(),
                            })
                        })
                        .collect()
                })
                .unwrap_or_default()
        },
        screenshot_annotations,
        no_video_warning: None,
        has_transcript_gaps: false,
    }
}

#[cfg(test)]
mod fdd_parse_tests {
    use super::*;

    #[test]
    fn parses_fdd_business_rules_and_glossary() {
        let v = serde_json::json!({
            "title": "Functional Specification: Sync gate",
            "purpose": "Relax ERP sync before earnings.",
            "scope_in": ["Hotfix"],
            "scope_out": ["UI rewrite"],
            "glossary": [{"term": "AL", "meaning": "Advanced Labor", "source": "transcript", "type_format": "Product"}],
            "business_rules": [{"id": "BR-01", "text": "Allow earnings without sync", "timestamp": "00:08:45"}],
            "formulas": [{"name": "eligible", "formula": "IsSyncedERP==0", "inputs": "flag", "example": "unsynced row"}],
            "worked_examples": [{"title": "Hotfix path", "steps": ["Create ES", "Sync later"], "timestamp": "00:10:00"}],
            "edge_cases": [{"case": "Non-ERP client", "expected": "Unchanged"}],
            "open_questions": [{"text": "Popup copy?", "timestamp": ""}],
            "release_notes": ["Q2 hotfix"],
            "screenshot_annotations": []
        });
        let doc = brief_doc_from_json(&v, "rec.mp4");
        assert_eq!(doc.business_rules.len(), 1);
        assert_eq!(doc.business_rules[0].id, "BR-01");
        assert_eq!(doc.glossary[0].term, "AL");
        assert_eq!(doc.formulas[0].name, "eligible");
        assert_eq!(doc.worked_examples[0].steps.len(), 2);
        assert_eq!(doc.edge_cases[0].case, "Non-ERP client");
        assert_eq!(doc.release_notes[0], "Q2 hotfix");
    }

    #[test]
    fn parses_br_screenshot_refs() {
        let v = serde_json::json!({
            "business_rules": [
                {"id": "BR-01", "text": "No refs rule", "timestamp": "00:05:00", "screenshot_refs": []},
                {"id": "BR-02", "text": "Rule with refs", "timestamp": "00:10:00", "screenshot_refs": [0, 2]},
                {"id": "BR-03", "text": "No refs field at all", "timestamp": ""},
            ],
            "screenshot_annotations": []
        });
        let doc = brief_doc_from_json(&v, "test.mp4");
        assert_eq!(doc.business_rules[0].screenshot_refs, Vec::<usize>::new());
        assert_eq!(doc.business_rules[1].screenshot_refs, vec![0usize, 2usize]);
        assert_eq!(doc.business_rules[2].screenshot_refs, Vec::<usize>::new());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_timestamp_hms() {
        assert_eq!(parse_timestamp("01:30:00"), Some(5400.0));
        assert_eq!(parse_timestamp("00:05:30"), Some(330.0));
    }

    #[test]
    fn parse_timestamp_ms() {
        assert_eq!(parse_timestamp("05:30"), Some(330.0));
    }

    #[test]
    fn parse_timestamp_invalid() {
        assert_eq!(parse_timestamp("not_a_time"), None);
        assert_eq!(parse_timestamp(""), None);
    }

    #[test]
    fn coerce_anchor_repairs_hour_as_minute() {
        // 08:01:19 literal = 28879s; for a ~30min video reinterpret → 500s.
        let duration = 30.0 * 60.0;
        let raw = parse_timestamp("08:01:19").unwrap();
        assert!(raw > duration);
        let fixed = coerce_anchor_seconds(raw, duration).unwrap();
        assert!((fixed - 500.0).abs() < 0.1);
        // In-range hour mark on a long recording stays.
        let long = coerce_anchor_seconds(parse_timestamp("01:10:00").unwrap(), 2.0 * 3600.0).unwrap();
        assert!((long - 4200.0).abs() < 0.1);
    }

    #[tokio::test]
    async fn mock_provider_success() {
        let provider = MockProvider::ok(3);
        let cancel = CancellationToken::new();
        let anchors = provider.detect_moments("some transcript", 2, 5, &cancel).await.unwrap();
        assert_eq!(anchors.len(), 3);
        assert!(anchors[0].seconds > 0.0);
    }

    #[tokio::test]
    async fn mock_provider_anchor_error() {
        let provider = MockProvider::failing_anchors("network error");
        let cancel = CancellationToken::new();
        let result = provider.detect_moments("transcript", 2, 5, &cancel).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("network error"));
    }

    #[tokio::test]
    async fn mock_provider_doc_no_video() {
        let provider = MockProvider::ok(0);
        let cancel = CancellationToken::new();
        let doc = provider.generate_doc("transcript", &[], "test.mp4", &cancel).await.unwrap();
        assert!(doc.no_video_warning.is_some());
    }
}
