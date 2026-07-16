# Architecture

## Shape

Sanymar is a local modular monolith: one Tauri process hosts a React webview and a Rust core. This avoids deployment and authentication complexity while retaining explicit internal seams that can be tested or replaced. There is no separate backend.

## React–Tauri boundary

React owns presentation and transient view state. `src/services/sanymar.ts` is the typed IPC boundary and supplies a clearly labeled browser mock during frontend-only development. Tauri command handlers deserialize requests, delegate to application services, and serialize stable view models. They do not orchestrate broadcast behavior.

## Rust boundaries

- `music_provider`: normalized playback domain and provider interface; mock playback remains the default.
- `spotify`: PKCE authorization, refresh, and Web API adapter that normalizes playback/queue responses.
- `music_facts`: attributed fact model, explicit verification methods, offline mock, and cache-first MusicBrainz adapter.
- `rj_engine`: profiles, deterministic content direction, script validation, state machine, coordination, cancellation/stale-job protection.
- `llm`: the `ScriptGenerator` interface, offline mock, normalized prompt builder, and local Ollama adapter.
- `tts`: cancellation-aware provider interface with typed delivery intent, offline mock, validated Sherpa-ONNX/Kokoro and loopback Parler adapters, and bounded WAV artifacts.
- `audio`: mock routing plus validated, cancellation-aware WAV playback on the default OS device.
- `security`: credential and security adapters.
- `playback`: application-level playback/coordinator models.
- `database`: connection, migration, and repositories.
- `settings`: typed safe defaults and validation.
- `commands`: thin Tauri surface.
- `errors`: categorized, redacted application errors.

External API models must live inside adapters and be normalized before entering the domain.

## Provider flow

```text
React -> typed command -> application service/coordinator
                              |-> SpotifyAuthService -> CredentialStore
                              |-> MusicProvider
                              |-> MusicFactProvider
                              |-> ScriptGenerator -> ScriptValidator
                              |-> TextToSpeechProvider -> AudioPlayer
                              `-> Repository / broadcast memory
```

Mocks implement every external seam. Spotify authorization uses a one-shot loopback listener, PKCE state/challenge verification, bounded callback input, a no-redirect HTTP client, and Windows Credential Manager. The playback adapter uses bounded HTTPS, refreshes expiring access tokens, normalizes current/next tracks, validates artwork URLs, and maps authentication, account, timeout, and rate-limit failures. Live mode is explicitly user-selected and currently read-only in the UI.

The Ollama adapter implements `ScriptGenerator` through the configured loopback-only base URL. It calls `/api/chat` with `stream: false`, temperature zero, and a strict JSON schema supplied both through Ollama's `format` field and the system instruction. It then parses and validates dialogue, fact IDs, word count, recent phrases, and formatting. A failed output-contract or dialogue-validation attempt receives one corrective retry containing the typed rule category but not the rejected response; over-length dialogue receives a 75%-of-limit retry target. Final failures retain a typed, non-content-bearing reason. At the application boundary, a final invalid model output becomes an intentional-silence segment so unattended broadcasting continues safely; provider, configuration, timeout, and cancellation errors are not hidden by this policy. Health checks call `/api/version` and `/api/tags` to distinguish reachability from model availability. The adapter uses a redirect-disabled client, connect/request timeouts, typed errors, and a cancellation token owned by the broadcast coordinator. It never installs a model.

The prompt boundary sends only normalized display fields: the selected DJ profile, previous/next track names, segment constraints, a typed spoken-delivery guide, explicitly verified facts, recent exclusions, and selected station lore. Fixed running-joke text is not sent. The delivery guide shapes tone through sentence rhythm rather than TTS markup: energetic segments use short forward-moving sentences, warm segments stay conversational, reflective segments use measured clauses, jokes use a compact understated payoff, and station identification stays concise. The system contract requires breath-sized speech, natural contractions, deliberate punctuation, and no title quotation marks, speaker labels, emoji, Markdown, bracketed tags, SSML, or page-oriented asides. A deterministic domain normalizer removes those non-spoken artifacts before validation and storage, and the same boundary is applied before TTS as defense in depth. It never invents or paraphrases content. The editorial director permits model-created light humour only through an infrequently selected `ShortJoke` segment; other segment types discourage forced jokes. Spotify provider IDs, ISRCs, artwork URLs, OAuth material, and unrelated settings are excluded. Prompt contents are not logged.

MusicBrainz lookup is unattended and cache-first. The adapter uses a required contact-bearing User-Agent, a shared one-request-per-second limiter, a redirect-disabled client, a request timeout, cancellation, and typed failures. ISRC is preferred; otherwise normalized title, artist, score, and duration must agree. Near-equal candidates are ambiguous and yield a negative cache entry. Only constructed authoritative metadata facts cross the adapter boundary; raw MusicBrainz payloads are neither persisted nor sent to Ollama. Provider failures yield an empty fact set so editorial selection falls back to non-factual segments.

English speech synthesis uses the pinned Sherpa-ONNX Rust API and a user-supplied Kokoro model directory. A prepared script retains its segment type, which the application maps to a typed `DeliveryStyle` on `VoiceSettings`. Speech-first dialogue supplies the primary phrasing and pause contour; the Kokoro adapter then applies a small bounded factor around the user's base speed: neutral, warm, energetic, playful, reflective, or authoritative. Voice identity never changes automatically, and this combination is not represented as semantic emotion control. The adapter validates and canonicalizes fixed required assets, then converts Windows verbatim path strings to native-compatible DOS/UNC form only when crossing the Sherpa FFI boundary; canonical paths remain authoritative for containment checks. It loads CPU inference on a blocking worker, checks the selected speaker against model metadata, uses a progress callback for cancellation, and writes mono 16-bit PCM through a create-new temporary file followed by an atomic rename. It validates samples, WAV headers, lengths, rate, job identity, and track identity before returning an artifact. Output is confined to the Tauri application cache. Reference audio and voice cloning are not exposed.

Parler-TTS Mini implements the same `TextToSpeechProvider` trait through a user-started, IPv4-loopback-only Python/PyTorch process. This process is an external local provider like Ollama, not an application backend: Sanymar never launches, installs, supervises, or grants it network-facing access. It keeps the user-installed model resident for repeat synthesis. The Rust adapter sends only dialogue plus allowlisted speaker, typed delivery style, bounded rate, and volume; it follows no redirects, applies connect/request timeouts, caps response bytes, parses PCM RIFF/WAVE defensively, and writes the artifact atomically under the application cache. The service maps typed style to fixed reviewed descriptions and rejects reference audio, arbitrary descriptions, and unknown fields. Client cancellation closes the request and stale identity checks reject the result; because PyTorch generation is not interruptible through this HTTP contract, server-side work already inside inference may finish before its single worker accepts another request.

Local audio routing keeps mock artifacts silent and sends real artifacts to a Rodio adapter using the default OS output device. The adapter accepts only canonical absolute `.wav` files within a bounded size, decodes on a blocking worker, waits for completion, and polls the broadcast cancellation token so a stale track can stop speech. Rodio is built with only playback, WAV, and structured-tracing features. Output-device selection and Spotify pause/resume orchestration are separate future boundaries.

Windows release builds produce the optimized Tauri executable plus WiX and NSIS installers. The bundle manifest places the pinned Sherpa C/C++ and ONNX Runtime shared libraries beside `sanymar.exe`, which is both Tauri's Windows resource directory and the location searched by the native loader. Kokoro models, Ollama, Parler, Python, and generated audio are never bundled. Installers are currently unsigned development artifacts.

## Broadcast state machine

The inspectable states are `Idle`, `Monitoring`, `FetchingFacts`, `SelectingSegment`, `GeneratingScript`, `ValidatingScript`, `SynthesizingSpeech`, `WaitingForTransition`, `PausingMusic`, `Speaking`, `ResumingMusic`, `Cancelled`, and `Failed`. Transitions carry a job ID and track identity. A track/queue change cancels the active token; every later step checks both identities before output can play. Shutdown cancels outstanding work.

Mock or Ollama generation exercises monitoring through waiting-for-transition. Mock, Kokoro, or Parler synthesis then enters `SynthesizingSpeech`; real artifacts enter `Speaking` while default-device playback runs, whereas mock audio remains silent. Prepared scripts retain job, track, and segment identity. Dashboard polling compares the active track identity and cancels in-flight generation, synthesis, or speech when it changes; stale job validation remains mandatory before an artifact is accepted.

When `automaticTransitionSpeech` is enabled in live mode, a Rust application service polls normalized Spotify playback independently of React. It identifies the current/queued track pair, begins one generation-and-synthesis attempt as soon as that pair is stable, and keeps the validated WAV in memory until its duration fits the remaining track time plus a small boundary buffer. Preparing early prevents local LLM and model-loading latency from consuming the transition slot. Automatic mode requires a spoken segment for every stable pair rather than applying editorial silence; recent segment, fact, and opening memory still limits repetition. Playback is scheduled to finish near the transition. The service cancels on current-track changes, queue changes, disabled automation, or stale identity. It never invokes Spotify pause, resume, or skip; therefore speech overlays the music rather than creating a true music gap.

The automation worker runs beneath a restart supervisor. Outer watchdogs bound each Spotify poll, end-to-end generation/synthesis preparation, audio stop, and playback even when an adapter's own timeout or cancellation fails to return. A preparation failure or watchdog expiry cancels the coordinator and permits one retry after a fixed cooldown; exhausting that pair does not consume the next pair's fresh attempt budget. Playback expiry stops audio and never replays the same artifact. Provider-specific timeouts remain the first line of failure mapping, while these deadlines protect scheduler liveness.

## Data and privacy

SQLite stores settings, normalized catalog metadata, attributed facts, positive/negative fact lookup cache markers, history, and generated scripts. Fact verification distinguishes human review from authoritative automated metadata. It stores the public Spotify Client ID and the user-supplied MusicBrainz contact but never access or refresh tokens. Spotify tokens use Windows Credential Manager behind `CredentialStore`. Logs are structured by module, state transitions include correlation/job IDs, and sensitive strings are redacted. Complete prompts are not logged by default.

## Cancellation and failure

Async providers accept cancellation context where work can be long-lived. The coordinator binds artifacts to a job and track fingerprint and refuses stale artifacts. Typed failures distinguish provider, authentication, playback, fact, LLM, TTS, audio, validation, database, configuration, cancellation, and internal categories. Recoverable provider outages do not prevent mock-mode startup.

Aborting an async wrapper cannot forcibly terminate native blocking inference. Native providers therefore retain cooperative cancellation, while the automation watchdog releases scheduler state so subsequent track pairs can proceed.
