//! Post-ASR pyannote diarization sidecar: spawn Python CLI → merge Person labels.

use crate::pipeline::JOB_CANCELLED_MSG;
use crate::process_kill;
use crate::settings::{AppSettings, DiarizeFailPolicy};
use crate::timed_transcript::{TimedSegment, TimedTranscript};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command as TokioCommand;
use tokio_util::sync::CancellationToken;

/// One speaker turn from `tools/diarize/run_diarize.py` JSON.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct DiarizeTurn {
    pub start_ms: u64,
    pub end_ms: u64,
    pub speaker: String,
}

#[derive(Debug, Deserialize)]
struct DiarizeJson {
    turns: Vec<DiarizeTurn>,
}

/// True when post-ASR diarize may run (settings intent + secrets present).
pub fn diarize_speakers_effective(settings: &AppSettings) -> bool {
    settings.export_webvtt
        && settings.diarize_speakers
        && !settings.hf_token.trim().is_empty()
        && settings
            .diarize_python_path
            .as_ref()
            .map(|p| !p.trim().is_empty())
            .unwrap_or(false)
}

/// Overlap in milliseconds between [a0,a1) and [b0,b1).
fn overlap_ms(a0: u64, a1: u64, b0: u64, b1: u64) -> u64 {
    let start = a0.max(b0);
    let end = a1.min(b1);
    end.saturating_sub(start)
}

/// Assign each segment the Person label with maximum time overlap.
pub fn merge_diarize_turns(segments: &mut [TimedSegment], turns: &[DiarizeTurn]) {
    if turns.is_empty() {
        return;
    }
    for segment in segments.iter_mut() {
        let mut best_speaker: Option<&str> = None;
        let mut best_overlap = 0u64;
        for turn in turns {
            let o = overlap_ms(segment.start_ms, segment.end_ms, turn.start_ms, turn.end_ms);
            if o > best_overlap {
                best_overlap = o;
                best_speaker = Some(turn.speaker.as_str());
            }
        }
        if let Some(speaker) = best_speaker {
            segment.speaker = Some(speaker.to_string());
        }
    }
}

fn apply_win_no_window(cmd: &mut TokioCommand) {
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    {
        let _ = cmd;
    }
}

/// Resolve `run_diarize.py`: settings override, then next to python, then `tools/diarize` near cwd/exe.
pub fn resolve_diarize_script(settings: &AppSettings, python: &Path) -> Option<PathBuf> {
    if let Some(p) = settings.diarize_script_path.as_ref() {
        let trimmed = p.trim();
        if !trimmed.is_empty() {
            let pb = PathBuf::from(trimmed);
            if pb.is_file() {
                return Some(pb);
            }
        }
    }
    if let Some(dir) = python.parent() {
        let beside = dir.join("run_diarize.py");
        if beside.is_file() {
            return Some(beside);
        }
        let scripts = dir.join("Scripts").join("run_diarize.py");
        if scripts.is_file() {
            return Some(scripts);
        }
    }
    // Dev / portable: tools/diarize next to repo or next to the executable.
    let candidates = [
        PathBuf::from("tools/diarize/run_diarize.py"),
        std::env::current_exe()
            .ok()
            .and_then(|e| e.parent().map(|p| p.join("tools/diarize/run_diarize.py")))
            .unwrap_or_default(),
        std::env::current_exe()
            .ok()
            .and_then(|e| {
                e.parent()
                    .and_then(|p| p.parent())
                    .and_then(|p| p.parent())
                    .map(|p| p.join("tools/diarize/run_diarize.py"))
            })
            .unwrap_or_default(),
    ];
    for c in candidates {
        if c.is_file() {
            return Some(c);
        }
    }
    None
}

/// Run the Python sidecar and return speaker turns.
pub async fn run_diarize_sidecar(
    settings: &AppSettings,
    wav: &Path,
    out_json: &Path,
    cancel: &CancellationToken,
    timeout: Duration,
) -> Result<Vec<DiarizeTurn>, String> {
    let python = settings
        .diarize_python_path
        .as_ref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| {
            "Diarize enabled but Python path is empty. Set Preferences → Speakers (pyannote) → Python path."
                .to_string()
        })?;
    if !python.is_file() {
        return Err(format!(
            "Diarize Python not found: {}. Point Settings to a venv python.exe with pyannote installed.",
            python.display()
        ));
    }
    let script = resolve_diarize_script(settings, &python).ok_or_else(|| {
        "run_diarize.py not found. Set Preferences → Speakers (pyannote) → Script path, or keep tools/diarize/run_diarize.py next to the repo."
            .to_string()
    })?;

    let token = settings.hf_token.trim();
    if token.is_empty() {
        return Err(
            "Hugging Face token required for diarize. Add it in Preferences → Speakers (pyannote)."
                .to_string(),
        );
    }

    if let Some(parent) = out_json.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create diarize out dir: {e}"))?;
    }

    let mut cmd = TokioCommand::new(&python);
    cmd.arg("-u")
        .arg(&script)
        .arg("--wav")
        .arg(wav)
        .arg("--out-json")
        .arg(out_json)
        .env("HF_TOKEN", token)
        .env("HUGGING_FACE_HUB_TOKEN", token)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    apply_win_no_window(&mut cmd);

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Failed to spawn diarize Python: {e}"))?;
    let pid = child.id();

    let stderr = child.stderr.take();
    let stdout = child.stdout.take();

    let stderr_task = tokio::spawn(async move {
        let mut full = String::new();
        if let Some(err) = stderr {
            let mut reader = BufReader::new(err);
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line).await {
                    Ok(0) => break,
                    Ok(_) => full.push_str(&line),
                    Err(_) => break,
                }
            }
        }
        full
    });

    let stdout_task = tokio::spawn(async move {
        let mut full = String::new();
        if let Some(out) = stdout {
            let mut reader = BufReader::new(out);
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line).await {
                    Ok(0) => break,
                    Ok(_) => full.push_str(&line),
                    Err(_) => break,
                }
            }
        }
        full
    });

    let status = tokio::select! {
        _ = cancel.cancelled() => {
            if let Some(p) = pid {
                process_kill::kill_process_tree(p);
            }
            stderr_task.abort();
            stdout_task.abort();
            return Err(JOB_CANCELLED_MSG.to_string());
        }
        r = tokio::time::timeout(timeout, async { child.wait().await.map_err(|e| e.to_string()) }) => {
            match r {
                Ok(Ok(st)) => st,
                Ok(Err(e)) => return Err(e),
                Err(_) => {
                    if let Some(p) = pid {
                        process_kill::kill_process_tree(p);
                    }
                    stderr_task.abort();
                    stdout_task.abort();
                    return Err("Diarize timed out".to_string());
                }
            }
        }
    };

    let stderr_text = stderr_task.await.unwrap_or_default();
    let _stdout_text = stdout_task.await.unwrap_or_default();

    if !status.success() {
        let code = status.code().unwrap_or(-1);
        let detail = stderr_text.trim();
        if detail.is_empty() {
            return Err(format!("Diarize exited with code {code}"));
        }
        return Err(format!("Diarize failed (exit {code}): {detail}"));
    }

    let raw = std::fs::read_to_string(out_json)
        .map_err(|e| format!("Failed to read diarize JSON {}: {e}", out_json.display()))?;
    let parsed: DiarizeJson = serde_json::from_str(&raw)
        .map_err(|e| format!("Invalid diarize JSON: {e}"))?;
    Ok(parsed.turns)
}

/// Apply diarize to a transcript according to settings / fail policy.
/// Returns Ok(transcript) always unless fail policy says FailJob.
pub async fn apply_diarize_to_transcript(
    settings: &AppSettings,
    wav: &Path,
    work_dir: &Path,
    mut transcript: TimedTranscript,
    cancel: &CancellationToken,
    on_warning: impl Fn(&str),
) -> Result<TimedTranscript, String> {
    if !diarize_speakers_effective(settings) {
        return Ok(transcript);
    }

    if transcript.segments.is_empty() {
        let msg = "Diarize skipped: transcript has no timed segments (Person tags need WebVTT timings). Plain text is kept.";
        on_warning(msg);
        if matches!(settings.diarize_fail_policy, DiarizeFailPolicy::FailJob) {
            return Err(msg.to_string());
        }
        return Ok(transcript);
    }

    let out_json = work_dir.join("diarize_turns.json");
    // Generous timeout: spike was ~25× realtime on CUDA; CPU is much slower.
    let timeout = Duration::from_secs(30 * 60);

    match run_diarize_sidecar(settings, wav, &out_json, cancel, timeout).await {
        Ok(turns) => {
            if turns.is_empty() {
                on_warning(
                    "Diarize returned no speaker turns; transcript kept without Person tags.",
                );
            } else {
                merge_diarize_turns(&mut transcript.segments, &turns);
                on_warning(&format!(
                    "Diarize assigned speakers from {} turn(s).",
                    turns.len()
                ));
            }
            Ok(transcript)
        }
        Err(e) if e == JOB_CANCELLED_MSG => Err(e),
        Err(e) => {
            match settings.diarize_fail_policy {
                DiarizeFailPolicy::KeepTranscript => {
                    on_warning(&format!(
                        "Diarize failed; transcript kept without Person tags. Detail: {e}"
                    ));
                    Ok(transcript)
                }
                DiarizeFailPolicy::FailJob => Err(format!("Diarize failed: {e}")),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_picks_max_overlap() {
        let turns = vec![
            DiarizeTurn {
                start_ms: 0,
                end_ms: 5000,
                speaker: "Person 1".into(),
            },
            DiarizeTurn {
                start_ms: 5000,
                end_ms: 10000,
                speaker: "Person 2".into(),
            },
        ];
        let mut segments = vec![
            TimedSegment {
                start_ms: 1000,
                end_ms: 4000,
                text: "hello".into(),
                speaker: None,
            },
            TimedSegment {
                start_ms: 6000,
                end_ms: 9000,
                text: "world".into(),
                speaker: None,
            },
            TimedSegment {
                start_ms: 4500,
                end_ms: 6500,
                text: "bridge".into(),
                speaker: None,
            },
        ];
        merge_diarize_turns(&mut segments, &turns);
        assert_eq!(segments[0].speaker.as_deref(), Some("Person 1"));
        assert_eq!(segments[1].speaker.as_deref(), Some("Person 2"));
        // 4500-5000 = 500ms P1; 5000-6500 = 1500ms P2 → Person 2
        assert_eq!(segments[2].speaker.as_deref(), Some("Person 2"));
    }

    #[test]
    fn diarize_effective_requires_token_and_python() {
        let mut s = AppSettings::default();
        s.export_webvtt = true;
        s.diarize_speakers = true;
        assert!(!diarize_speakers_effective(&s));
        s.hf_token = "hf_x".into();
        assert!(!diarize_speakers_effective(&s));
        s.diarize_python_path = Some("C:\\venv\\python.exe".into());
        assert!(diarize_speakers_effective(&s));
    }
}
