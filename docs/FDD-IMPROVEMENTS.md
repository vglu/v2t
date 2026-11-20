# FDD in v2t — recommendations (post video-to-spec compare)

Default document mode is **FDD** (`briefDocMode: "fdd"`). Brief remains available in Settings.

## Done in this pass

- Setting **FDD | Brief**, default FDD; chip label follows mode; PDF suffix `.fdd.pdf` / `.brief.pdf`
- FDD schema/prompt aligned with video-to-spec (purpose, glossary, BR-xx, formulas, examples, edge cases, open questions, release notes)
- FDD always asks LLM for product anchors (snap to scene-changes); skips Teams chrome in anchor prompt
- Model default: current multimodal Flash (`gemini-3.1-flash-lite`)

## Done: caption-gate + dense anchors + MD

- After frame extract in **FDD** mode, Gemini classifies each screenshot (`gate_frames`); meeting chrome is dropped before Pass 2.
- Heuristic fallback on LLM failure (`brief_gate.rs`); product-UI phrases force-keep.
- Progress phases: `brief-gate` / `brief-gate-done` (lists dropped labels).
- **FDD anchors**: LLM timestamps are authoritative (10–20), +2s settle offset, soft scene snap ≤3s — no longer limited to ~7 ffmpeg scenes.
- **Markdown primary**: `{stem}.fdd.md` + `screenshots/` (video-to-spec layout); PDF fixed (underline under headings).

## Transcription note (ARB compare)

ARB FDD vs Spec used the **same Teams `.vtt/.txt`**, not v2t Whisper. Quality gap was anchors/layout, not ASR.
Teams itself had a “let me share my screen” loop (~00:02–00:07). When v2t must ASR itself, prefer larger Whisper / HTTP models and keep **Use subtitles when available** on.

## Queue (Анатолий, по одной) — см. `docs/TASKS.md`

| ID | Задача | Статус |
|---|---|---|
| TASK-FDD-Q1 | Stronger model profile for FDD | ✅ done |
| TASK-FDD-Q2 | Inline screenshots under BR (PDF) | ✅ done |
| TASK-FDD-Q3 | ADO / work-item harvest | ✅ done |
| TASK-FDD-Q4 | Window-aware crop | ✅ done |

## Non-goals (for now)

- Calling out to the Python video-to-spec CLI from v2t (option C) — deferred unless packaging that tool as a sidecar is requested.
