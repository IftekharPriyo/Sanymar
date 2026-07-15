# Parler-TTS Mini local provider

This is Sanymar's optional, user-managed expressive speech provider. It binds to IPv4 loopback only, keeps an already installed `parler-tts-mini-v1` model loaded, and returns mono PCM16 WAV. Sanymar never starts this process or downloads its model.

## Start

From the repository root, after creating the environment described in [`../parler-probe/README.md`](../parler-probe/README.md):

```powershell
$env:PYTHONUTF8 = "1"
.\.venv-parler\Scripts\python.exe tools\parler-provider\server.py --model-dir "C:\AI\Models\parler-tts-mini-v1"
```

The default address is `http://127.0.0.1:43822`. The model directory must already contain `config.json` and local model assets. No Hugging Face download is attempted. Keep this window open while using Parler and stop it with `Ctrl+C`.

## Contract

- `GET /health` reports readiness, the fixed model identity, supported speakers, and sample rate.
- `POST /synthesize` accepts exactly `text`, `speaker`, `deliveryStyle`, `rate`, and `volume`.
- Speakers and delivery styles are fixed allowlists. Unknown fields, reference audio, custom voice descriptions, oversized requests, and invalid controls are rejected.
- Dialogue is not logged. The process listens only on `127.0.0.1` and is not an authenticated network service; do not expose or proxy its port.

The service serializes generation because one model/GPU is shared. Disconnecting cancels Sanymar's request and its stale result, but cannot interrupt PyTorch after model generation has begun. A queued request waits until that generation returns.

## Tests

The pure contract tests do not load a model:

```powershell
.\.venv-parler\Scripts\python.exe -m unittest discover -s tools\parler-provider -p "test_*.py"
```

Rust adapter tests use a mocked HTTP server and likewise require no Python or model.
