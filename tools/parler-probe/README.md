# Parler-TTS Mini evaluation probe

This is an isolated development probe, not a Sanymar provider. It compares five radio-delivery descriptions with one named Parler speaker and records synthesis latency, real-time factor, and peak allocated GPU memory. It never downloads a model, accepts reference audio, starts a service, or changes application settings.

## Prerequisites

1. Close Sanymar, Ollama models, games, and other GPU-heavy applications. The probe requires CUDA and intentionally refuses CPU inference.
2. Install 64-bit Python 3.12 from [python.org](https://www.python.org/downloads/windows/). Select the installer option that adds Python to `PATH`, then open a new PowerShell window.
3. From the repository root, create an isolated environment:

   ```powershell
   python -m venv .venv-parler
   .\.venv-parler\Scripts\python.exe -m pip install --upgrade pip
   ```

4. Use the official [PyTorch Windows selector](https://pytorch.org/get-started/locally/) with **Stable / Windows / Pip / Python / CUDA**, then run its displayed command through the environment's Python. For example, replace `pip` in the displayed command with:

   ```powershell
   .\.venv-parler\Scripts\python.exe -m pip
   ```

5. Install the pinned Parler probe dependencies:

   ```powershell
   .\.venv-parler\Scripts\python.exe -m pip install torchaudio==2.5.1 --index-url https://download.pytorch.org/whl/cu124
   .\.venv-parler\Scripts\python.exe -m pip install -r tools\parler-probe\requirements.txt
   ```

## Explicit model download

The model is a user-managed asset. Review its [Apache-2.0 model card](https://huggingface.co/parler-tts/parler-tts-mini-v1), then download `parler-tts/parler-tts-mini-v1` yourself to a permanent directory such as `C:\AI\Models\parler-tts-mini-v1`. The probe requires a complete local directory containing `config.json`, `tokenizer_config.json`, and Safetensors weights, and loads it with `local_files_only=True`.

One explicit option, using the compatible Hugging Face client installed with Parler, is:

```powershell
$env:PYTHONUTF8 = "1"
.\.venv-parler\Scripts\huggingface-cli.exe download parler-tts/parler-tts-mini-v1 --local-dir "C:\AI\Models\parler-tts-mini-v1"
```

Sanymar and the probe never issue this download command themselves.

## Run

```powershell
.\.venv-parler\Scripts\python.exe tools\parler-probe\probe.py --model-dir "C:\AI\Models\parler-tts-mini-v1" --speaker Jon
```

Outputs are written to the ignored `.local\parler-probe` directory. Listen to all five WAV files for consistent speaker identity, natural emphasis, unwanted noise, pronunciation, and whether the emotional difference is useful rather than theatrical. The JSON report records performance only and does not contain credentials or application data.

The Mini model's strongest consistency scores include Jon, Lea, Gary, Jenna, and Mike, so the probe intentionally limits its speaker choices to those five. Run another voice with `--speaker Gary`, or generate one case with `--case energetic`.

## Acceptance target

- No malformed, empty, clipped, or unexpectedly noisy WAV files.
- A consistent recognizable speaker across all five styles.
- Warm, energetic, playful, and reflective samples are audibly distinct without sounding exaggerated.
- Real-time factor below `1.0` for every sample after model load.
- Peak allocated GPU memory leaves enough headroom for the intended Ollama workflow; measure concurrent use separately rather than assuming both models fit.

Passing this probe does not authorize or prove a production integration. A real provider would still require a reviewed runtime boundary, health checks, timeouts, track-change cancellation, artifact confinement, mocked provider tests, packaging, and explicit model settings.
