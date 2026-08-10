# Sanymar

Sanymar is a Windows desktop AI radio jockey. It observes authorized Spotify playback, writes short English radio dialogue with either a mock, local Ollama, or cloud Qwen provider, synthesizes it with a local voice provider, and plays the result through the Windows default audio device.

## Current status

The application currently supports:

- offline mock Spotify, script, TTS, and audio providers for development;
- Spotify Authorization Code with PKCE, Windows Credential Manager token storage, and normalized current/queued-track monitoring;
- cloud Qwen generation through Groq's OpenAI-compatible chat-completions API, with prompt minimization, validation, retry, timeout, and cancellation;
- unattended, cache-first MusicBrainz first-release metadata with conservative matching;
- bundled English Kokoro synthesis in-process through Sherpa-ONNX, with Parler retained only as a development override;
- validated WAV playback on the Windows default audio device; and
- opt-in automatic transition speech that pre-generates one segment, pauses Spotify at the handoff, plays the RJ alone, and resumes the next track from its beginning.

Sanymar never mixes RJ speech over Spotify. In automatic mode it uses device-targeted pause, skip, seek, and resume commands so commentary plays separately between songs.

## Architecture

Sanymar is a Tauri 2 modular monolith. React owns views and typed IPC calls; Tauri commands delegate to Rust application and domain modules. SQLite stores non-secret configuration, normalized catalog/fact data, cache markers, and generated-script history. Short-term repetition memory remains inside the running application.

Spotify, facts, script generation, TTS, audio, and credential storage are behind explicit provider boundaries with offline mocks. OAuth tokens are excluded from SQLite and frontend storage. See [Architecture](docs/ARCHITECTURE.md), [Decisions](docs/DECISIONS.md), and [Threat model](docs/THREAT_MODEL.md).

## Requirements

- Windows 10 or 11 with WebView2
- Node.js 20.19+ or 22.12+ and npm
- Rust stable with the MSVC target
- Microsoft C++ Build Tools with the **Desktop development with C++** workload

Real providers are optional. Spotify requires a Spotify developer application. Groq Qwen requires a user-provided Groq API key. The old Ollama adapter is retained in code but disabled from normal setup while the Groq endpoint is tested. Parler additionally requires a compatible Python, PyTorch, and CUDA environment.

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

Release builds produce a portable executable directory plus unsigned MSI and NSIS installers. The four pinned Sherpa/ONNX Runtime DLLs are installed beside `sanymar.exe`, and the reviewed English Kokoro pack is installed under `models/kokoro-en-v0_19`. Parler, Python, and cloud API keys are not bundled.

Build the Windows packages with:

```powershell
npm.cmd run tauri build
```

The first clean release build downloads the pinned official `kokoro-en-v0_19.tar.bz2` into the ignored `.local` build cache, verifies its exact byte length and SHA-256, extracts it with the `tar`/`bzip2` tools shipped by Git for Windows, and verifies the required model files before Tauri packages them. Later builds reuse the verified local assets. The large model and language-data files are intentionally excluded from Git; the source URL, hashes, licenses, and attribution remain committed. This is release-build preparation only—the installed application never downloads a voice model.

Outputs are written beneath `src-tauri/target/release/`:

- `sanymar.exe` and its required DLLs form the portable release directory;
- `bundle/msi/` contains the WiX installer; and
- `bundle/nsis/` contains the setup executable.

Keep the portable `.exe` together with its four native DLLs and `models/kokoro-en-v0_19` resource directory. Moving only `sanymar.exe` will break Kokoro startup. The installers are unsigned development artifacts, so Windows may show an unknown-publisher warning.

## Spotify setup

1. The project owner registers the exact redirect URI `http://127.0.0.1:43821/callback` in the packaged Spotify application. Do not use `localhost` or the former `/oauth/callback` path.
2. Run the native app and open **Settings > Spotify connection**.
3. Select **Connect Spotify** and finish authorization in the system browser. Live playback is enabled automatically after connection.
4. Start playback on an active Spotify device.

Sanymar reads the current track, progress, active device, and queue. It does not download music. The packaged Spotify Client ID is public application metadata and is intentionally hidden from normal settings. No client secret is used. Access and refresh tokens are stored in Windows Credential Manager, never SQLite, `.env`, logs, or frontend storage. Disconnecting Spotify removes the stored credential.

The authorization includes `user-modify-playback-state`. Automatic commentary uses it only for device-targeted pause, expected-track advancement, seek-to-start, and resume around the prepared RJ segment; it does not expose general playback controls.

## Cloud Qwen via Groq

1. Create a Groq API key in your Groq account.
2. Open **Settings > Dialogue model**.
3. Keep the base URL as `https://api.groq.com/openai/v1/` unless Groq changes its documented endpoint.
4. Keep `qwen/qwen3.6-27b` or enter another Groq-hosted Qwen chat model ID.
5. Paste the API key into **Groq API key**, select **Save Groq key**, then select **Check Groq Qwen**.
6. Save settings.

The Groq key is stored in Windows Credential Manager, never SQLite, frontend storage, logs, or Git. Sanymar sends the same minimized script request used by Ollama: display names, segment constraints, recent-memory exclusions, selected station lore, and explicitly verified facts. Spotify tokens, provider IDs, artwork URLs, ISRCs, and unrelated settings are excluded.

Groq uses `/chat/completions`, `stream: false`, temperature zero, `reasoning_effort: "none"`, and a prompt-level JSON contract validated locally by Sanymar. Provider-enforced JSON mode is intentionally disabled for Qwen because Groq can reject a whole generation when the model's draft fails its JSON validator. Disabling reasoning mode also prevents Qwen from returning `<think>...</think>` traces before the usable JSON. Malformed or locally invalid output receives one validation-only corrective retry. A second locally invalid result becomes safe silence; authentication, rate-limit, provider, timeout, and cancellation failures remain visible.

Dialogue is written for speech: breath-sized phrases, natural contractions, restrained punctuation, and segment-specific rhythm. Speaker labels, title quotation marks, emoji, Markdown, bracketed emotion tags, and SSML are removed before validation and again before TTS.

The default profile contains no hard-coded running joke. A short-joke segment is infrequent and suppressed by recent-segment memory; other segments discourage forced punchlines.

## Automatic MusicBrainz facts

Enter an email address or HTTPS contact URL under **Settings > MusicBrainz contact**. MusicBrainz requires this in the identifying User-Agent.

With live Spotify playback active, Sanymar checks its local cache first, prefers ISRC matching, and otherwise requires a strong title, artist, duration, and score match. Weak or ambiguous results produce no facts, allowing non-factual commentary instead of risky metadata. Lookups are unattended and there is no mandatory review queue.

MusicBrainz currently supplies only strongly matched first-release-date metadata. It is not used for recording stories, song meanings, quotations, chart positions, collaborations, or awards.

## English Kokoro setup

1. Install Sanymar with the NSIS or MSI package; the reviewed `kokoro-en-v0_19` assets are included.
2. Open **Settings > RJ voice**. The bundled English voice pack is already selected automatically.
3. Choose the voice actor, speech speed, and RJ volume, then save. No voice model path or provider process is required.
4. Use **Generate with model** and **Speak test segment** for a manual end-to-end check.

Sherpa-ONNX `1.13.4` is pinned and uses its Windows shared runtime. Sanymar resolves the installed pack automatically and never downloads or replaces voice assets at runtime. Generated mono PCM WAV files are validated and written only beneath the application cache before default-device playback. The model's Apache-2.0 notice and the eSpeak NG GPL-3.0-or-later notice/source reference travel with the resource pack.

Kokoro receives speech-first dialogue plus a typed delivery style. The current adapter realizes delivery through small, bounded pacing changes while preserving the selected voice. This improves phrasing but is not genuine semantic emotion, pitch, or emphasis control.

## Development-only Parler-TTS Mini override

Parler is retained only as an advanced development adapter and is not part of listener setup. Normal packaged use selects bundled Kokoro automatically. Parler runs as a user-managed loopback process so the GPU model remains loaded between segments. Sanymar never starts Python, downloads the model, accepts reference audio, or exposes arbitrary voice descriptions.

1. Install Python 3.12 and create `.venv-parler` in the repository root.
2. Install matching PyTorch and torchaudio wheels for the local CUDA runtime.
3. Install [`tools/parler-probe/requirements.txt`](tools/parler-probe/requirements.txt) and keep the Parler model in a permanent local directory.
4. Start the provider in a separate PowerShell window:

   ```powershell
   $env:PYTHONUTF8 = "1"
   .\.venv-parler\Scripts\python.exe tools\parler-provider\server.py --model-dir "C:\AI\Models\parler-tts-mini-v1"
   ```

5. Wait for `Parler Mini ready on http://127.0.0.1:43822`.
6. Enable development debug logging, open **Settings > RJ voice > Development voice override**, select **User-managed Parler**, keep the loopback URL, and select a supported speaker.
7. Save and run a manual speech test before enabling automatic transition speech.

The provider must remain open while selected. Stop it with `Ctrl+C`. Sanymar sends only dialogue, the allowlisted speaker, typed delivery style, bounded speed, and volume. Returned PCM WAV data is size-bounded and validated before caching or playback. See [the provider contract](tools/parler-provider/README.md).

## Automatic transition speech

Automatic transition speech requires live Spotify playback and a real TTS provider for audible output. Groq Qwen is the active script generator while the cloud endpoint is tested.

1. Configure and health-check Spotify, the desired script generator, and the desired voice provider.
2. Enable **Automatically prepare and play speech at Spotify transitions** in **Settings**, then save.
3. Keep Sanymar running with an active Spotify device and a known queued track.

For every stable current/next track pair, a Rust background service generates one spoken segment immediately, synthesizes and validates its WAV ahead of time, then arms a transition near the reported end of the current track. It sends pause slightly ahead of the reported boundary to account for command latency, advances the outgoing track if necessary, rewinds the expected next track, plays the RJ WAV alone, and resumes Spotify. Automatic mode requires a spoken segment rather than editorial silence. Recent segment, fact, and opening memory still limits repetition.

The service is supervised and uses watchdog deadlines around Spotify polling, script/TTS preparation, audio stopping, and playback. A failed or timed-out preparation receives one cooldown retry for the same track pair; after that, the pair is skipped safely and the next stable pair starts with a fresh attempt budget. If the scheduler task itself fails, it is cleaned up and restarted automatically.

A current-track or queue change cancels stale generation, synthesis, or speech, except for the expected handoff into the prepared next track. A recorded interruption is resumed after success, failure, cancellation, watchdog expiry, or worker restart. Timing uses Spotify's polled progress and Web API commands, so a small part of the outgoing tail may be trimmed or the next track may be briefly audible before reset.

**Generate with model** and **Speak test segment** remain available as manual diagnostics.

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

Provider tests use mocks or fake backends and do not require public services, Groq access, a Kokoro model, Python, CUDA, or an audio device. Native model loading, Spotify behavior, subjective voice quality, and default-device playback remain local integration checks.

## Configuration and logging

Application configuration is typed and persisted through desktop **Settings**. The `SANYMAR_*` names in `.env.example` are reserved documentation placeholders; the application does not load that file or use those variables at runtime.

Rust logging uses the standard `RUST_LOG` filter inherited by the process. For example:

```powershell
$env:RUST_LOG = "sanymar_lib=info"
npm.cmd run tauri dev
```

Complete prompts, dialogue, provider response bodies, Spotify tokens, authorization codes, and credentials are not logged by default. The **development debug logging** setting does not override those exclusions.

## Security and privacy

- Never place Spotify tokens, authorization codes, Groq API keys, client secrets, or other credentials in `.env`, source files, SQLite, logs, screenshots, or frontend `localStorage`.
- Spotify uses Authorization Code with PKCE and needs no client secret.
- Parler is restricted to a loopback HTTP endpoint. Groq Qwen is the only active script provider currently supported in normal setup.
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

Sanymar uses Spotify pause, skip, seek, and resume around commentary, but the Web API is not sample-accurate and does not guarantee command ordering with other Player endpoints. Timing is approximate. Scheduler watchdogs recover stalled application tasks, but force-closing the process while Spotify is paused can still require manual playback recovery.

Output-device selection, WAV cache cleanup, code signing, public redistribution review, release automation, and desktop end-to-end tests are unfinished. Script validation is defensive but cannot guarantee factual correctness or human-quality delivery. See [Known limitations](docs/KNOWN_LIMITATIONS.md) for the complete list.
