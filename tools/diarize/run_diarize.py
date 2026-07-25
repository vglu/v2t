"""
v2t post-ASR diarize sidecar (pyannote).

Usage:
  set HF_TOKEN=hf_...
  python run_diarize.py --wav path.wav --out-json out.json [--model pyannote/speaker-diarization-3.1]

Stdout: progress lines (English). Exit 0 on success; non-zero on failure.
Does NOT run Whisper — only speaker turns for merge into TimedTranscript.
"""
from __future__ import annotations

import argparse
import json
import os
import sys
import time
from pathlib import Path


def _patch_hf_hub_auth_token_alias() -> None:
    """Map legacy use_auth_token= to token= for huggingface_hub>=1."""
    import huggingface_hub as hub

    original = hub.hf_hub_download

    def hf_hub_download(*args, **kwargs):
        if "use_auth_token" in kwargs and "token" not in kwargs:
            kwargs["token"] = kwargs.pop("use_auth_token")
        elif "use_auth_token" in kwargs:
            kwargs.pop("use_auth_token")
        return original(*args, **kwargs)

    hub.hf_hub_download = hf_hub_download
    try:
        import pyannote.audio.core.pipeline as pap

        pap.hf_hub_download = hf_hub_download
    except Exception:
        pass


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="v2t pyannote diarize sidecar")
    parser.add_argument("--wav", required=True, type=Path, help="16 kHz mono WAV path")
    parser.add_argument("--out-json", required=True, type=Path, help="Output JSON path")
    parser.add_argument(
        "--model",
        default=os.environ.get("PYANNOTE_MODEL", "pyannote/speaker-diarization-3.1"),
        help="Hugging Face model id",
    )
    args = parser.parse_args(argv)

    token = (os.environ.get("HF_TOKEN") or os.environ.get("HUGGING_FACE_HUB_TOKEN") or "").strip()
    if not token:
        print("ERROR: set HF_TOKEN (Hugging Face token with pyannote license accepted).", file=sys.stderr)
        print("  https://huggingface.co/settings/tokens", file=sys.stderr)
        print("  https://huggingface.co/pyannote/speaker-diarization-3.1", file=sys.stderr)
        return 2

    wav: Path = args.wav
    if not wav.is_file():
        print(f"ERROR: missing wav: {wav}", file=sys.stderr)
        return 2

    out_json: Path = args.out_json
    out_json.parent.mkdir(parents=True, exist_ok=True)

    import torch
    from pyannote.audio import Pipeline

    # torch 2.6 defaults weights_only=True; pyannote checkpoints need full unpickle.
    _torch_load = torch.load

    def _torch_load_trusted(*args, **kwargs):
        kwargs["weights_only"] = False
        return _torch_load(*args, **kwargs)

    torch.load = _torch_load_trusted

    print(f"torch {torch.__version__} cuda={torch.cuda.is_available()}", flush=True)
    if torch.cuda.is_available():
        print(f"gpu: {torch.cuda.get_device_name(0)}", flush=True)
    else:
        print("WARN: CUDA not available — running on CPU (slow).", flush=True)

    model_id = args.model
    print(f"Loading pipeline {model_id} …", flush=True)
    t0 = time.perf_counter()
    os.environ["HF_TOKEN"] = token
    os.environ["HUGGING_FACE_HUB_TOKEN"] = token
    _patch_hf_hub_auth_token_alias()
    pipeline = Pipeline.from_pretrained(model_id, use_auth_token=token)
    if pipeline is None:
        print("ERROR: Pipeline.from_pretrained returned None (gated model?).", file=sys.stderr)
        print(f"  Accept access at: https://huggingface.co/{model_id}", file=sys.stderr)
        return 2
    device = torch.device("cuda" if torch.cuda.is_available() else "cpu")
    pipeline.to(device)
    print(f"Loaded in {time.perf_counter() - t0:.1f}s on {device}", flush=True)

    print(f"Diarizing {wav.name} ({wav.stat().st_size} bytes) …", flush=True)
    t1 = time.perf_counter()
    annotation = pipeline(str(wav))
    elapsed = time.perf_counter() - t1

    if hasattr(annotation, "speaker_diarization"):
        diar = annotation.speaker_diarization
    else:
        diar = annotation

    speaker_ids: list[str] = []
    turns: list[dict] = []
    for turn, _, speaker in diar.itertracks(yield_label=True):
        if speaker not in speaker_ids:
            speaker_ids.append(speaker)
        person = f"Person {speaker_ids.index(speaker) + 1}"
        turns.append(
            {
                "start_ms": int(round(turn.start * 1000)),
                "end_ms": int(round(turn.end * 1000)),
                "speaker_raw": speaker,
                "speaker": person,
            }
        )

    peak_vram_mb = None
    if torch.cuda.is_available():
        peak_vram_mb = round(torch.cuda.max_memory_allocated() / (1024 * 1024), 1)

    result = {
        "wav": str(wav),
        "model": model_id,
        "device": str(device),
        "elapsed_sec": round(elapsed, 3),
        "duration_sec": round(max((t["end_ms"] for t in turns), default=0) / 1000, 3),
        "num_speakers": len(speaker_ids),
        "speakers": speaker_ids,
        "peak_vram_mb": peak_vram_mb,
        "turns": turns,
    }
    out_json.write_text(json.dumps(result, ensure_ascii=False, indent=2), encoding="utf-8")
    print(
        json.dumps({k: result[k] for k in result if k != "turns"}, ensure_ascii=False),
        flush=True,
    )
    print(f"Wrote {out_json} ({len(turns)} turns)", flush=True)
    return 0


if __name__ == "__main__":
    sys.exit(main())
