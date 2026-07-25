# Diarize sidecar (post-ASR pyannote)

Opt-in speaker labels for **any** ASR language (Russian, Ukrainian, …) without forcing English.

## Setup (Windows + CUDA)

```powershell
cd D:\Projects\v2t\tools\diarize
python -m venv .venv
.\.venv\Scripts\python.exe -m pip install --upgrade pip
.\.venv\Scripts\python.exe -m pip install torch==2.6.0+cu124 torchaudio==2.6.0+cu124 --index-url https://download.pytorch.org/whl/cu124
.\.venv\Scripts\python.exe -m pip install -r requirements.txt
```

You can reuse the spike venv at `spikes/diarize-pyannote-20260725\.venv` if it already works.

## Hugging Face

1. Token: https://huggingface.co/settings/tokens  
2. Accept: https://huggingface.co/pyannote/speaker-diarization-3.1 (and linked models)

## v2t Preferences

1. Essentials → Also save… → **Export WebVTT**
2. Advanced → **Speakers (pyannote diarize)**  
   - Hugging Face token  
   - Python path → `…\Scripts\python.exe`  
   - Optional script path (default: `tools/diarize/run_diarize.py`)  
   - Enable **Diarize speakers after transcription**  
   - Fail policy: keep transcript (default) or fail job

ASR (large-v3 + `-l ru`) is unchanged; Person tags are merged after.

## Manual CLI test

```powershell
$env:HF_TOKEN = "hf_..."
.\.venv\Scripts\python.exe .\run_diarize.py --wav sample.wav --out-json out.json
```
