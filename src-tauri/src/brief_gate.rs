//! Caption / product-frame gate for FDD screenshots.
//!
//! Drops Teams/Meet chrome (attendee lists, calendars, chat-only, idle dashboards)
//! so Pass 2 receives product UI / ADO / diagrams — closer to video-to-spec anchors.

use crate::brief_provider::{BriefFrame, FrameGateDecision};

/// Minimum frames to keep after gating (if any were extracted).
pub const MIN_KEEP_FRAMES: usize = 3;

/// Heuristic: true when label/caption looks like non-product meeting chrome.
pub fn looks_like_meeting_chrome(label: &str, caption: &str) -> bool {
    let hay = format!("{label} {caption}").to_ascii_lowercase();
    const NEEDLES: &[&str] = &[
        "attendee",
        "participants",
        "roster",
        "calendar",
        "agenda view",
        "meeting chat",
        "teams chat",
        "chat window",
        "chat panel",
        "sis severity",
        "sis dashboard",
        "waiting for",
        "let me share",
        "share my screen",
        "lobby",
        "people pane",
        "participant strip",
    ];
    NEEDLES.iter().any(|n| hay.contains(n))
}

/// Heuristic: true when text clearly refers to product / engineering UI.
pub fn looks_like_product_ui(text: &str) -> bool {
    let hay = text.to_ascii_lowercase();
    const NEEDLES: &[&str] = &[
        "ado",
        "azure devops",
        "work item",
        "system parameter",
        "timesheet",
        "advanced labor",
        "earnings",
        "synced to erp",
        "issyncederp",
        "finance and operations",
        "f&o",
        "formula",
        "spreadsheet",
        "acceptance criteria",
        "functional requirement",
    ];
    NEEDLES.iter().any(|n| hay.contains(n))
}

/// Apply gate decisions; copy captions onto survivors; reindex to 0..n-1.
///
/// If gating would drop everything (or below [`MIN_KEEP_FRAMES`]) while we had
/// more frames, keep the first `MIN_KEEP_FRAMES` (or all) as a safety net.
pub fn apply_gate_decisions(frames: &[BriefFrame], decisions: &[FrameGateDecision]) -> Vec<BriefFrame> {
    assert_eq!(
        frames.len(),
        decisions.len(),
        "gate decisions must align with frames"
    );

    let mut kept: Vec<BriefFrame> = frames
        .iter()
        .zip(decisions.iter())
        .filter(|(_, d)| d.keep)
        .map(|(f, d)| {
            let mut frame = f.clone();
            if !d.caption.is_empty() {
                frame.caption = d.caption.clone();
            }
            frame
        })
        .collect();

    // Only rescue when every frame was dropped — do not re-introduce chrome
    // just to hit a minimum count when product frames already exist.
    if kept.is_empty() && !frames.is_empty() {
        let n = MIN_KEEP_FRAMES.min(frames.len());
        kept = frames
            .iter()
            .zip(decisions.iter())
            .take(n)
            .map(|(f, d)| {
                let mut frame = f.clone();
                if !d.caption.is_empty() {
                    frame.caption = d.caption.clone();
                }
                frame
            })
            .collect();
    }

    for (i, frame) in kept.iter_mut().enumerate() {
        frame.index = i;
    }
    kept
}

/// Local-only gate using labels/captions (no LLM). Used as fallback.
pub fn heuristic_decisions(frames: &[BriefFrame]) -> Vec<FrameGateDecision> {
    let mut decisions: Vec<FrameGateDecision> = frames
        .iter()
        .map(|f| {
            let drop = looks_like_meeting_chrome(&f.label, &f.caption);
            FrameGateDecision {
                keep: !drop,
                caption: f.caption.clone(),
                reason: if drop {
                    "heuristic: meeting chrome".into()
                } else {
                    String::new()
                },
            }
        })
        .collect();
    if decisions.iter().all(|d| !d.keep) {
        for d in &mut decisions {
            d.keep = true;
            d.reason = "heuristic fallback: keep all".into();
        }
    }
    decisions
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn frame(index: usize, label: &str, caption: &str) -> BriefFrame {
        BriefFrame {
            index,
            seconds: index as f64 * 10.0,
            label: label.to_string(),
            path: PathBuf::from(format!("f{index}.png")),
            caption: caption.to_string(),
        }
    }

    #[test]
    fn chrome_detector_hits_attendees_and_calendar() {
        assert!(looks_like_meeting_chrome("scene_00", "List of meeting attendees."));
        assert!(looks_like_meeting_chrome("calendar_view", ""));
        assert!(!looks_like_meeting_chrome(
            "system_parameters",
            "Advanced Labor sync flag on system parameters page."
        ));
        assert!(looks_like_product_ui(
            "Shows a dashboard with domain data and ADO work item"
        ));
    }

    #[test]
    fn apply_keep_drops_chrome_and_reindexes() {
        let frames = vec![
            frame(0, "attendees", "List of meeting attendees."),
            frame(1, "calendar", "Calendar view for ARB."),
            frame(2, "ado_work_item", "ADO #1129017 details."),
            frame(3, "al_timesheet", "Advanced Labor timesheet grid."),
        ];
        let decisions = heuristic_decisions(&frames);
        assert!(!decisions[0].keep);
        assert!(!decisions[1].keep);
        assert!(decisions[2].keep);
        assert!(decisions[3].keep);
        let out = apply_gate_decisions(&frames, &decisions);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].index, 0);
        assert_eq!(out[1].index, 1);
        assert_eq!(out[0].label, "ado_work_item");
    }

    #[test]
    fn apply_keep_safety_net_when_all_dropped() {
        let frames = vec![
            frame(0, "attendees", "attendees"),
            frame(1, "calendar", "calendar"),
        ];
        let decisions = vec![
            FrameGateDecision {
                keep: false,
                caption: "attendees".into(),
                reason: "drop".into(),
            },
            FrameGateDecision {
                keep: false,
                caption: "calendar".into(),
                reason: "drop".into(),
            },
        ];
        let out = apply_gate_decisions(&frames, &decisions);
        assert_eq!(out.len(), 2); // safety: keep both (MIN_KEEP=3 but only 2 exist)
        assert_eq!(out[0].index, 0);
    }
}
