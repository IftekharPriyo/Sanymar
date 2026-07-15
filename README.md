# Sanymar

Sanymar is a local-first Windows desktop AI radio jockey. It observes authorized Spotify playback, writes short English radio dialogue locally, synthesizes it with a local voice provider, and plays the result through the Windows default audio device.

## Current status

The application currently supports:

- offline mock Spotify, script, TTS, and audio providers for development;
- Spotify Authorization Code with PKCE, Windows Credential Manager token storage, and normalized current/queued-track monitoring;
- optional loopback-only Ollama generation through `/api/chat` with structured output, validation, one bounded corrective retry, and cancellation;
- unattended, cache-first MusicBrainz first-release metadata with conservative matching;
- English Kokoro synthesis in-process through Sherpa-ONNX, or an explicitly user-started Parler-TTS Mini loopback provider;
- validated WAV playback on the Windows default audio device; and
- opt-in automatic transition speech that pre-generates and pre-renders one spoken segment for every stable current/next track pair.

Sanymar does not currently pause, duck, resume, or otherwise control Spotify during commentary. Automatic speech plays over the music near the reported track boundary.

## Architecture

Sanymar is a Tauri 2 modular monolith. React owns views and typed IPC calls; Tauri commands delegate to Rust application and domain modules. SQLite stores non-secret configuration, normalized catalog/fact data, cache markers, and generated-script history. Short-term repetition memory remains inside the running application.

Spotify, facts, script generation, TTS, audio, and credential storage are behind explicit provider boundaries with offline mocks. OAuth tokens are excluded from SQLite and frontend storage. See [Architecture](docs/ARCHITECTURE.md), [Decisions](docs/DECISIONS.md), and [Threat model](docs/THREAT_MODEL.md).

## Requirements

- Windows 10 or 11 with WebView2
- Node.js 20.19+ or 22.12+ and npm
- Rust stable with the MSVC target
- Microsoft C++ Build Tools with the **Desktop development with C++** workload

Real providers are optional. Spotify requires a Spotify developer application. Ollama and voice models are installed and managed separately by the user. Parler additionally requires a compatible Python, PyTorch, and CUDA environment.

## Run locally

Install dependencies and start the frontend-only browser mock:

```powershell
npm.cmd install
npm.cmd run dev
```

Vite prints the browser URL. This mode uses explicit in-process mocks and does not exercise native Tauri, Spotify credentials, real TTS, or device audio.

Run the native desktop application with:

```powershell
npm.cmd run tauri dev
```

Mock mode is enabled by default and needs no account, model, TTS engine, or audio device. The UI labels mocked providers, and mock speech produces no sound.

Release builds produce a portable executable directory plus unsigned MSI and NSIS installers. The four pinned Sherpa/ONNX Runtime DLLs are installed beside `sanymar.exe`; user-managed Ollama, Kokoro models, Parler, and Python are not bundled.

Build the Windows packages with:

```powershell
npm.cmd run tauri build
```

Outputs are written beneath `src-tauri/target/release/`:

- `sanymar.exe` and its required DLLs form the portable release directory;
- `bundle/msi/` contains the WiX installer; and
- `bundle/nsis/` contains the setup executable.

Keep the portable `.exe` together with `onnxruntime.dll`, `onnxruntime_providers_shared.dll`, `sherpa-onnx-c-api.dll`, and `sherpa-onnx-cxx-api.dll`. Moving only `sanymar.exe` will break Kokoro startup. The installers are unsigned development artifacts, so Windows may show an unknown-publisher warning.

## Spotify setup

1. Create a desktop application in the Spotify developer dashboard.
2. Register the exact redirect URI `http://127.0.0.1:43821/callback`. Do not use `localhost` or the former `/oauth/callback` path.
3. Run the native app and open **Settings > Spotify connection**.
4. Paste the public Spotify Client ID. Do not enter or create a client secret for Sanymar.
5. Select **Connect Spotify** and finish authorization in the system browser.
6. Enable **Use live Spotify playback on the dashboard**, then save settings.
7. Start playback on an active Spotify device.

Sanymar reads the current track, progress, active device, and queue. It does not download music. Access and refresh tokens are stored in Windows Credential Manager, never SQLite, `.env`, logs, or frontend storage. Disconnecting Spotify removes the stored credential.

The authorization currently includes playback-control scope, but this phase only observes playback. Sanymar does not invoke Spotify pause, resume, or skip during automatic commentary.

## Local Ollama setup

1. Install and start Ollama separately.
2. Install a model with Ollama's own tooling. Sanymar never downloads or installs models.
3. Open **Settings > Local Ollama**.
4. Keep `http://127.0.0.1:11434` unless Ollama uses another loopback port.
5. Enter the exact installed model name and select **Check Ollama**.
6. Enable **Use real local Ollama instead of the mock script generator**, then save.

Spotify credentials are not needed to test Ollama manually. Model requests include normalized DJ and track display fields, segment constraints, recent-memory exclusions, selected station lore, and explicitly verified facts. Provider IDs, artwork URLs, ISRCs, Spotify tokens, credentials, and unrelated user data are excluded.

Generation uses `/api/chat`, `stream: false`, temperature zero, and a strict JSON schema. Malformed or locally invalid output receives one validation-only corrective retry. The rejected response is not included in that retry or logged. A second locally invalid result becomes safe silence; provider, configuration, timeout, and cancellation failures remain visible.

Dialogue is written for speech: breath-sized phrases, natural contractions, restrained punctuation, and segment-specific rhythm. Speaker labels, title quotation marks, emoji, Markdown, bracketed emotion tags, and SSML are removed before validation and again before TTS.

The default profile contains no hard-coded running joke. A short-joke segment is infrequent and suppressed by recent-segment memory; other segments discourage forced punchlines.

## Automatic MusicBrainz facts

Enter an email address or HTTPS contact URL under **Settings > MusicBrainz contact**. MusicBrainz requires this in the identifying User-Agent.

With live Spotify playback active, Sanymar checks its local cache first, prefers ISRC matching, and otherwise requires a strong title, artist, duration, and score match. Weak or ambiguous results produce no facts, allowing non-factual commentary instead of risky metadata. Lookups are unattended and there is no mandatory review queue.

MusicBrainz currently supplies only strongly matched first-release-date metadata. It is not used for recording stories, song meanings, quotations, chart positions, collaborations, or awards.

## English Kokoro setup

1. Download `kokoro-en-v0_19` yourself from the [Sherpa-ONNX Kokoro documentation](https://k2-fsa.github.io/sherpa/onnx/tts/pretrained_models/kokoro.html).
2. Extract it to a permanent local directory containing `model.onnx`, `voices.bin`, `tokens.txt`, and `espeak-ng-data/`.
3. Open **Settings > Local English voice** and select **Sherpa-ONNX Kokoro**.
4. Enter the absolute model directory and choose a voice ID and base speed.
5. Select **Check voice provider**, save, then use **Generate with Ollama** and **Speak test segment** for a manual end-to-end check.

Sherpa-ONNX `1.13.4` is pinned and uses its Windows shared runtime. Sanymar does not bundle or download the Kokoro model. Generated mono PCM WAV files are validated and written only beneath the application cache before default-device playback.

Kokoro receives speech-first dialogue plus a typed delivery style. The current adapter realizes delivery through small, bounded pacing changes while preserving the selected voice. This improves phrasing but is not genuine semantic emotion, pitch, or emphasis control.

## Parler-TTS Mini setup

Parler is the optional prompt-directed voice provider. It runs as a user-managed loopback process so the GPU model remains loaded between segments. Sanymar never starts Python, downloads the model, accepts reference audio, or exposes arbitrary voice descriptions.

1. Install Python 3.12 and create `.venv-parler` in the repository root.
2. Install matching PyTorch and torchaudio wheels for the local CUDA runtime.
3. Install [`tools/parler-probe/requirements.txt`](tools/parler-probe/requirements.txt) and keep the Parler model in a permanent local directory.
4. Start the provider in a separate PowerShell window:

   ```powershell
   $env:PYTHONUTF8 = "1"
   .\.venv-parler\Scripts\python.exe tools\parler-provider\server.py --model-dir "C:\AI\Models\parler-tts-mini-v1"
   ```

5. Wait for `Parler Mini ready on http://127.0.0.1:43822`.
6. Open **Settings > Local English voice**, select **Parler-TTS Mini local service**, keep the loopback URL, select a supported speaker, and choose **Check voice provider**.
7. Save and run a manual speech test before enabling automatic transition speech.

The provider must remain open while selected. Stop it with `Ctrl+C`. Sanymar sends only dialogue, the allowlisted speaker, typed delivery style, bounded speed, and volume. Returned PCM WAV data is size-bounded and validated before caching or playback. See [the provider contract](tools/parler-provider/README.md).

## Automatic transition speech

Automatic transition speech requires live Spotify playback and a real TTS provider for audible output. Real Ollama remains independently selectable; the script mock can still be used with live Spotify during development.

1. Configure and health-check Spotify, the desired script generator, and the desired voice provider.
2. Enable **Automatically prepare and play speech at Spotify transitions** in **Settings**, then save.
3. Keep Sanymar running with an active Spotify device and a known queued track.

For every stable current/next track pair, a Rust background service generates one spoken segment immediately, synthesizes and validates its WAV ahead of time, then holds that artifact until its measured duration fits the closing seconds of the current track. Automatic mode requires a spoken segment rather than editorial silence. Recent segment, fact, and opening memory still limits repetition.

A current-track or queue change cancels generation, synthesis, or playback and rejects stale artifacts. Timing uses Spotify's polled progress and the WAV duration, so fast skips, queue edits, provider latency, or network jitter can cancel a slot. Commentary overlaps the music because pause, duck, and resume are not implemented.

**Generate with Ollama** and **Speak test segment** remain available as manual diagnostics.

## Checks

```powershell
npm.cmd run test
npm.cmd run lint
npm.cmd run format:check
npm.cmd run build
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --all-features -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
cargo check --manifest-path src-tauri/Cargo.toml
```

Apply formatting with `npm.cmd run format` and `cargo fmt --manifest-path src-tauri/Cargo.toml`.

Provider tests use mocks or fake backends and do not require public services, a real Ollama installation, a Kokoro model, Python, CUDA, or an audio device. Native model loading, Spotify behavior, subjective voice quality, and default-device playback remain local integration checks.

## Configuration and logging

Application configuration is typed and persisted through desktop **Settings**. The `SANYMAR_*` names in `.env.example` are reserved documentation placeholders; the application does not load that file or use those variables at runtime.

Rust logging uses the standard `RUST_LOG` filter inherited by the process. For example:

```powershell
$env:RUST_LOG = "sanymar_lib=info"
npm.cmd run tauri dev
```

Complete prompts, dialogue, provider response bodies, Spotify tokens, authorization codes, and credentials are not logged by default. The **development debug logging** setting does not override those exclusions.

## Security and privacy

- Never place Spotify tokens, authorization codes, client secrets, or other credentials in `.env`, source files, SQLite, logs, screenshots, or frontend `localStorage`.
- Spotify uses Authorization Code with PKCE and needs no client secret.
- Ollama and Parler are restricted to loopback HTTP endpoints.
- Voice models are explicit user-managed assets and are never installed automatically.
- Reference audio and voice cloning are not supported.
- Generated audio is confined to the application cache and must pass local validation before playback.

## Repository map

```text
src/                      React UI, hooks, typed services, and tests
src-tauri/src/            Rust domain, application services, and providers
src-tauri/migrations/     Immutable SQLx migrations
src-tauri/capabilities/   Tauri least-privilege permissions
tools/parler-provider/    Optional user-started local Parler service
docs/                     Product, architecture, security, and plans
.agents/skills/           Focused project working references
AGENTS.md                 Rules for coding agents
```

## Known limitations

Sanymar schedules commentary near Spotify track boundaries but does not control the music transition. Voice overlaps active music because pause, duck, resume, and recovery behavior are not implemented. Timing is approximate.

Output-device selection, WAV cache cleanup, code signing, public redistribution review, release automation, and desktop end-to-end tests are unfinished. Script validation is defensive but cannot guarantee factual correctness or human-quality delivery. See [Known limitations](docs/KNOWN_LIMITATIONS.md) for the complete list.
