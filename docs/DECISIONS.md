# Architecture decision log

## ADR-001: Tauri 2 over Electron — accepted

Use the Windows webview and a Rust native core for smaller distribution and explicit native/security boundaries.

## ADR-002: React + Vite over Next.js — accepted

The desktop UI needs no SSR or web backend. Vite keeps the application local and simple.

## ADR-003: Rust modular monolith — accepted

One local process is operationally appropriate; modules and traits provide isolation without microservices.

## ADR-004: SQLite + SQLx migrations — accepted

SQLite matches local-first deployment. SQLx provides async access, typed rows, pooling, and repository-owned migrations.

## ADR-005: Provider abstractions and normalized music types — accepted

Spotify, Ollama, facts, TTS, audio, and credentials sit behind domain interfaces. External response models never leak inward.

## ADR-006: No vector database or separate backend in MVP — accepted

Recent-history queries and curated fact lookup fit normalized SQLite; additional infrastructure has no demonstrated need.

## ADR-007: OS credential storage for OAuth tokens — accepted

SQLite and browser storage are prohibited for raw tokens. The foundation supplies only an in-memory mock; a vault adapter is required before real OAuth.

## ADR-008: Mock-first provider development — accepted

Offline deterministic mocks allow UI/domain development and never claim integration availability.

## ADR-009: Plain CSS and local React state — accepted

The first UI does not justify a component framework or global state dependency.

## ADR-010: Deterministic seeded editorial selection — accepted

The content director receives an RNG, enabling reproducible tests while production can use entropy.

Production selection uses entropy rather than restarting a fixed seed for each segment. Humour is an editorial decision: no fixed callback text is supplied, and `ShortJoke` is independently eligible for approximately 8% of spoken segments with recent-segment suppression.

## ADR-011: Spotify desktop authorization via PKCE and Windows Credential Manager — accepted

Use Authorization Code with PKCE, a fixed explicit IPv4 loopback callback, random state, a one-shot bounded listener, and the system browser. The public Client ID may live in SQLite settings; access and refresh tokens live only in Windows Credential Manager behind `CredentialStore`. `oauth2` supplies the reviewed PKCE/token protocol, `reqwest` supplies bounded HTTPS without redirects, `keyring` supplies the Windows vault adapter, and Tauri's maintained opener plugin opens the authorization URL without shell execution. Mock mode remains the default until the separately tested Spotify Web API adapter is complete.

## ADR-012: Loopback Ollama with structured validated output - accepted

Keep Ollama behind `ScriptGenerator` and independently selectable from Spotify mode. Use the existing `reqwest` stack against an HTTP loopback URL only, call `/api/chat` without streaming, request a JSON-schema object, and validate the result before it crosses the adapter boundary. `/api/version` and `/api/tags` provide an explicit health/model check; Sanymar never pulls a model. The coordinator supplies `tokio-util` cancellation tokens so a track change can abort in-flight HTTP generation. `tokio-util` is the maintained Tokio companion crate and avoids a custom cancellation primitive. `wiremock` is development-only and makes HTTP behavior deterministic without requiring a local Ollama installation.

The schema is repeated in the system instruction because model adherence varies even when Ollama receives `format`; generation uses temperature zero. One validation-only corrective retry is allowed. The retry repeats the sanitized request and safe constraints but never includes the rejected model response, complete failure content, or provider credentials. This improves unattended recovery while keeping the existing strict validator as the acceptance boundary.

## ADR-013: Unattended confidence-gated MusicBrainz metadata - accepted

Use MusicBrainz as an authoritative structured metadata source, not a narrative fact source. The adapter remains behind `MusicFactProvider`, uses the existing `reqwest`, `tokio-util`, SQLx, URL, UUID, and `wiremock` dependencies, and requires the user to configure the contact demanded by MusicBrainz policy. It is cache-first and shares a one-request-per-second limiter. ISRC searches are preferred; title/artist/duration searches require high confidence and reject near-equal candidates. Automatic results use a distinct `authoritative_metadata` verification method rather than claiming human review. Persist normalized facts and positive/negative cache markers, never raw responses. Ambiguity or recoverable provider failure returns no facts, allowing unattended non-factual commentary without a review UI.

## ADR-014: Native English Kokoro synthesis through Sherpa-ONNX - accepted

Implement real TTS behind `TextToSpeechProvider` with the pinned Apache-2.0 `sherpa-onnx` 1.13.4 Rust crate and its Windows shared runtime. Use an explicitly user-installed English Kokoro model; Sanymar never downloads models. Shared linking avoids the Windows static-CRT conflict observed with the static archive. Inference runs on a blocking worker and uses Sherpa's progress callback plus broadcast cancellation. Only fixed canonical model assets are accepted, and PCM artifacts are written atomically beneath the application cache. Keep voice ID and speed typed, preserve mock TTS, prohibit reference-audio/voice-cloning features, and defer actual device playback to the audio-provider phase. Model and voice-pack licenses remain separate assets that must be audited before distribution.

## ADR-015: Rodio default-device WAV playback - accepted

Implement local voice playback behind `AudioPlayer` using pinned `rodio` 0.22.2 with default features disabled and only `playback`, `wav`, and `tracing` enabled. Rodio is maintained, Windows-compatible, uses CPAL for the device boundary, and avoids custom WASAPI code. Accept only bounded canonical local WAV artifacts, perform decoding/playback off the async runtime, wait for completion, and stop cooperatively when broadcast cancellation fires. Route mock artifacts to the preserved silent mock. Use the OS default device initially; explicit device selection and Spotify pause/resume require separate recovery-aware orchestration.

## ADR-016: Typed segment-aware delivery profiles - accepted

Extend `VoiceSettings` with provider-neutral delivery intent and preserve the selected voice identity. Map editorial segment types to neutral, warm, energetic, playful, reflective, or authoritative delivery before crossing the `TextToSpeechProvider` boundary. The current Kokoro adapter realizes that intent only as a small factor around the user's validated base speed, clamped to the existing 0.5–2.0 range. This is a conservative pacing improvement, not semantic emotion synthesis. At this decision point, do not add a Python sidecar or an experimental in-process Qwen port: the official Qwen3-TTS implementation targets Python/PyTorch, while reviewed Rust ports do not present a production-ready Windows library boundary. ADR-017 records the later, benchmark-driven decision to support an explicitly user-managed Parler provider process. No dependency is added by this decision.

## ADR-017: User-managed loopback Parler-TTS Mini provider - accepted

Offer Parler-TTS Mini as a richer English speech option behind the existing `TextToSpeechProvider` trait after a local probe found the Jon energetic voice materially more expressive than Kokoro, with measured real-time factors of about 1.30–1.53 on the target machine. Keep PyTorch and the user-installed model out of Sanymar's application process and distribution. The user starts a persistent provider bound to `127.0.0.1`; Sanymar treats it like the existing Ollama process and never starts Python, downloads assets, or exposes arbitrary execution. This is a narrow external-provider exception to ADR-006, not a web application backend.

Minimize and allowlist the contract: dialogue, a reviewed built-in speaker, provider-neutral delivery style, bounded rate, and volume are the only synthesis inputs. The service converts style to fixed descriptions and prohibits reference audio, voice cloning, and custom descriptions. The Rust adapter uses redirect-free loopback HTTP, typed timeouts/errors, cancellation, bounded response reads, strict PCM WAV validation, atomic cache writes, and existing stale job/track checks. Preserve mock and Kokoro modes. Automated tests mock HTTP and never require Python, CUDA, or a real model. The Python environment pins Parler-TTS and its Git-sourced audiotools dependency; PyTorch/torchaudio remain an explicitly matched user installation. Model and dependency licenses require review before distribution.

## ADR-018: Speech-first dialogue contract - accepted

Treat the generated script as spoken dialogue rather than display copy. Extend the sanitized Ollama input with a fixed segment-specific delivery guide and require breath-sized sentences, natural contractions, restrained punctuation, and speech-shaped rhythm. Prohibit speaker labels, quotation marks around music names, emoji, hashtags, Markdown, parenthetical delivery notes, bracketed emotion tags, SSML, and pronunciation annotations. Apply a deterministic domain normalizer before local validation and storage, then again before TTS as defense in depth. The normalizer may remove formatting, convert safe symbols to spoken equivalents, and collapse whitespace, but it must never paraphrase dialogue or introduce factual claims. This improves Kokoro phrasing without claiming semantic emotion synthesis, preserves the typed delivery intent for other providers, and adds no dependency.

## ADR-019: Pre-rendered automatic transition speech - accepted

Run transition scheduling as a Rust application service rather than React orchestration. In explicitly enabled live mode, poll normalized Spotify playback, prepare one spoken segment as soon as a stable current/queued track pair is available, synthesize ahead of the playback deadline, and use the validated artifact duration to align voice completion with the reported track boundary. Automatic mode does not select intentional silence because enabling it represents an explicit request for commentary at each stable transition; repetition memory and configured word targets still apply. Early preparation is preferred because local LLM and TTS latency can exceed a short end-of-track window. Preserve manual generation/speech controls as diagnostics and keep mock launch silent. A changed track or queue cancels the job and rejects stale artifacts. This phase does not authorize Spotify playback control: voice overlays music, and pause/duck/resume remains a separately tested future decision. No dependency is added.

## ADR-020: Windows installer and native TTS runtime layout - accepted

Use Tauri's standard WiX and NSIS bundlers for local Windows packages. Keep the existing generated icon set in the bundle manifest and declare the four pinned Sherpa/ONNX shared libraries as Windows resources whose destination is the executable directory. This matches the native loader requirement without modifying `PATH` or adding runtime DLL search code. User-managed Ollama, Kokoro models, Parler, Python, and cached audio remain outside the installer. Packages are unsigned development artifacts until code-signing identity, release automation, third-party notices, and model/runtime distribution review are complete. No dependency is added.

Superseded for Kokoro model assets by ADR-023 and ADR-025: the reviewed English Kokoro pack is now staged by release tooling and bundled into MSI/NSIS packages, while Ollama, Parler, Python, and cached/generated audio remain external.

## ADR-021: Watchdog-supervised transition automation - accepted

Keep recovery inside the Rust application service. Run the transition worker beneath a restart supervisor and apply outer deadlines to Spotify polling, end-to-end generation/synthesis preparation, audio stopping, and playback. These liveness deadlines complement provider-specific typed timeouts and protect later track pairs if an adapter or blocking wrapper never returns. On preparation failure or expiry, cancel the coordinator and allow one retry after a fixed cooldown; after exhaustion, skip that pair and reset the attempt budget when the current/next identity changes. Never replay timed-out audio. Native inference remains cooperatively cancellable because forcibly terminating a foreign-library thread is unsafe. No dependency is added.

## ADR-022: Separated Spotify commentary handoff - accepted

Replace overlapping transition speech with a device-targeted pause/skip/seek/speak/resume sequence owned by the Rust broadcast service. Prepare audio early, arm within a bounded end-of-track window, send pause slightly ahead of the reported boundary to compensate for request latency, advance the outgoing track while paused when necessary, seek the expected next track to position zero, rebind the coordinator job to that handoff, play only the validated local RJ artifact, and resume the same device. Empty Spotify control requests must include an explicit zero content length; live diagnosis showed Spotify returning HTTP 411 without it. Persist interruption state in application memory before issuing pause and clear it only after an accepted resume so task failures, cancellation, watchdog expiry, and worker restart can recover. Do not expose general player controls or implement ducking: Spotify policy prohibits mixing or overlapping Spotify content with other audio. Spotify's Web API is not sample-accurate and does not guarantee Player command ordering, so document the brief-tail/brief-preview limitation. Existing `reqwest`, `url`, and `wiremock` dependencies cover the implementation and tests; no dependency is added.

## ADR-023: Installer-managed English Kokoro pack - accepted

Bundle the already integration-tested `kokoro-en-v0_19` pack as a Tauri resource so ordinary Windows users do not locate or download ONNX assets. Resolve `models/kokoro-en-v0_19` beneath the runtime resource directory and prefer it over the existing optional absolute-path value, which remains only as a development fallback for unpackaged builds. Keep the existing canonical containment checks, speaker validation, cache-only output, mock provider, and prohibition on runtime model downloads or replacement. Retain the pack's Apache-2.0 notice and add the eSpeak NG GPL-3.0-or-later license/source notice alongside `espeak-ng-data`. This increases the uncompressed application payload by about 369 MB; installer compression and clean-machine verification are required. No dependency is added.

## ADR-024: Consumer provider settings and bounded RJ gain - accepted

Treat the packaged Spotify OAuth application identity and bundled Kokoro asset health as implementation plumbing rather than normal listener configuration. Ship the public Spotify Client ID, retain the fixed PKCE redirect and Windows credential boundary, and expose only connect/disconnect state; never add a client secret. Select bundled Kokoro automatically when Spotify is connected and remove voice-engine selection plus health/model diagnostics from the normal UI. Keep mock and Parler choices behind development debug mode so provider seams remain testable without making listeners start another process. A versioned settings normalization moves existing live Parler selections to Kokoro once. Ollama configuration remains visible because its model and loopback process are intentionally user-managed.

Superseded for normal script-provider setup by ADR-027: Groq Qwen is now the active listener-facing script provider, and Ollama is retained only as a development/backward-compatible adapter.

Add a persisted 0-100 RJ-volume percentage with a conservative 75% default. Normalize it to the existing provider-neutral `VoiceSettings.volume` field so Kokoro and Parler share the same validated control. The gain affects generated speech only, not Spotify or system volume. This changes no schema migration because application settings are serialized JSON with a serde default, and adds no dependency.

## ADR-025: Checksum-pinned Kokoro release-build staging - accepted

Do not commit the approximately 369 MB extracted Kokoro pack to ordinary Git: its 345 MB ONNX file exceeds GitHub's normal per-file limit and would permanently inflate source clones. Keep the official Sherpa-ONNX release URL, archive byte length, SHA-256, required extracted-file hashes, Apache/eSpeak licenses, and Sanymar attribution in the repository. Tauri's release `beforeBuildCommand` runs a PowerShell staging script that reuses a verified ignored cache or downloads the fixed archive, uses Git for Windows' matched `tar`/`bzip2` pair with local-drive handling, verifies before copying into the ignored resource directory, and fails closed on any mismatch. The installed application retains the runtime prohibition on model downloads and receives the complete pack inside MSI/NSIS. No application dependency is added.

## ADR-026: Fresh-state Spotify boundary and pause reconciliation - accepted

Do not issue the commentary pause solely from the background worker's coarse two-second playback sample. Use that sample only to arm a bounded boundary waiter, fetch normalized playback again before control, and proceed only when the outgoing track is within 400 ms of its reported end or Spotify has already advanced to the expected next track. Reduce the fixed command lead from 750 ms to 250 ms. After sending pause, require observed paused state for the expected current/next identity; a timeout may mean Spotify accepted the command, so reconcile before recovery, and retry the idempotent pause at most once when it remains unconfirmed. Keep the interruption record and resume recovery around the whole control attempt. This reduces early trims and false non-pauses without claiming sample-accurate Web API control. No dependency is added.

## ADR-027: Optional Groq Qwen cloud script generation - accepted

Add Groq-hosted Qwen as the active real `ScriptGenerator` provider while retaining mock and local Ollama code paths for development/backward compatibility. Use Groq's OpenAI-compatible HTTPS `/chat/completions` endpoint with `stream: false`, temperature zero, `reasoning_effort: "none"`, and a prompt-level JSON contract. Do not use provider-enforced JSON mode for Qwen in this phase because live testing showed Groq can reject the whole generation with a JSON validation failure before Sanymar can apply its local corrective retry. Disable Qwen reasoning traces so `<think>...</think>` content is not mixed into the radio dialogue contract. Reuse the existing sanitized prompt builder, local dialogue normalizer, strict validator, one validation-only corrective retry, timeout handling, and cancellation boundary. Store only the provider selection, base URL, and model ID in typed settings; store the Groq API key in Windows Credential Manager and never return it to React, SQLite, logs, or generated history.

Groq is a cloud boundary, so requests must remain minimal: DJ profile, previous/next track display names, selected segment type, word limits, verified facts, recent phrase/fact exclusions, and selected station lore only. Spotify credentials, provider IDs, ISRCs, artwork URLs, and unrelated settings are excluded. Authentication and rate-limit failures stay visible; only final locally invalid model output may become intentional silence. The existing `reqwest`, `serde`, `tokio-util`, and `wiremock` dependencies cover this implementation, so no dependency is added.
