# Settings

Sanymar persists typed, non-secret application settings in local SQLite. Spotify OAuth tokens are stored separately in Windows Credential Manager. Complete prompts and credentials are not settings and are not logged.

## Music playback

- `mockMode`: when enabled, the dashboard uses deterministic fixture tracks. When disabled, it reads the connected Spotify account. This setting does not select the script generator.
- `spotifyClientId`: packaged public Spotify application metadata. It is intentionally hidden from the normal UI; Sanymar never uses or accepts a client secret.
- `spotifyRedirectUri`: fixed to `http://127.0.0.1:43821/callback` and validated exactly.

## Script generation

- `scriptGeneratorProvider`: normal setup writes `groq_qwen`. The `mock` and `ollama` values are retained only for development/backward compatibility while the Groq endpoint is tested.
- `useOllama`: legacy compatibility flag retained for older settings. Normalization turns it off and moves old Ollama selections to `groq_qwen`.
- `ollamaBaseUrl` and `ollamaModel`: legacy fields retained for compatibility. They are not shown in normal setup.
- `groqBaseUrl`: HTTPS Groq OpenAI-compatible base URL. Default: `https://api.groq.com/openai/v1/`.
- `groqModel`: Groq-hosted Qwen chat model ID. Default: `qwen/qwen3.6-27b`.
- `maximumSegmentWords`: hard validation limit from 1 to 150 words. The editorial target may be shorter.

Use **Save Groq key** to store the Groq API key in Windows Credential Manager, then **Check Groq Qwen** to verify authentication and model access. The key is not part of `AppSettings`, is never returned to React, and is not stored in SQLite.

Generation is deterministic (`temperature: 0`) and strictly validated for Groq. Sanymar sends `reasoning_effort: "none"` for Qwen so the provider returns the usable answer instead of `<think>...</think>` traces. Sanymar asks for JSON in the prompt and enforces the schema locally instead of using Groq provider-enforced JSON mode for Qwen. If a draft violates the JSON contract, word limit, recent-phrase exclusions, formatting rules, or verified-fact references, Sanymar makes one corrective retry. Over-length drafts retry at 75% of `maximumSegmentWords`. If the corrected output remains locally invalid, the segment safely becomes silence; provider, authentication, rate-limit, timeout, cancellation, and configuration failures remain visible. Rejected drafts are never displayed, resent, or logged.

## Other foundation settings

`automaticTransitionSpeech` enables the backend transition monitor in live Spotify mode. It is off by default. When enabled, Sanymar prepares one spoken commentary segment as soon as a stable current/queued track pair is available and synthesizes it ahead of time. Near the reported boundary it sends pause slightly early to compensate for Spotify command latency, advances the outgoing track while paused if necessary, rewinds the expected next track to position zero, plays commentary alone, and resumes Spotify. Automatic mode does not editorially select silence; unrelated track or queue changes still cancel stale work. Spotify ducking and overlapping audio are not supported.

`talkFrequency` controls target commentary length and editorial pacing in automatic mode; manual/editorial selection may still use it to choose silence. Recent segment/fact/opening memory is preserved to limit repetition. `musicbrainzContact` is an email address or HTTPS contact URL stored locally and sent only in MusicBrainz's required identifying User-Agent. If it is empty, live metadata lookup is skipped. `cacheRetentionDays` controls positive and negative fact-cache freshness from 1 to 3650 days.

## English speech synthesis

- `ttsProvider`: `sherpa_kokoro` is selected automatically when a listener connects Spotify. `mock` and `parler_mini` remain development overrides visible only when debug logging is enabled; they are not part of normal setup.
- `ttsModelDirectory`: legacy/development-only absolute-path override. Packaged builds prefer the installer-managed `models/kokoro-en-v0_19` resource and do not expose this field in the normal UI. Override files still pass canonical containment validation.
- `ttsVoiceId`: zero-based speaker ID reported by the selected Kokoro package. The health check rejects IDs outside the model's speaker count.
- `parlerBaseUrl`: strict HTTP loopback address for the user-managed Parler service. Default: `http://127.0.0.1:43822`. Credentials, paths, queries, fragments, and non-loopback hosts are rejected.
- `parlerSpeaker`: one of the service's reviewed built-in speakers: `Jon`, `Gary`, `Mike`, `Lea`, or `Jenna`. Default: `Jon`. This is not a voice-cloning or reference-audio field.
- `ttsSpeedPercent`: base synthesis speed from 50 to 200; default 100. Sanymar applies a small segment-aware delivery factor and clamps the effective rate to the same safe range.
- `ttsVolumePercent`: provider-neutral RJ loudness from 0 to 100; default 75. It scales generated speech only and does not change Spotify or Windows system volume.

Generated WAV files are stored beneath the application cache, never in the installed model directory. The reviewed Kokoro pack ships with the installer and is never downloaded or replaced at runtime. The normal UI does not expose voice-engine selection, Kokoro paths, model-health details, or speaker-count diagnostics. A versioned settings normalization moves existing live installations away from the former Parler/manual-provider flow to bundled Kokoro once; developers may deliberately enable the override again. Real speech uses the Windows default output device; `audioOutputDevice` remains a placeholder for future explicit selection. Debug logging never enables complete prompt, provider-response, dialogue, or credential logging.

Delivery style is derived from the editorial segment and is not a persisted user setting. It never switches the selected speaker: next-song teases, one-line reactions, and simple transitions are energetic; short jokes are playful; listener observations stay warm; stories and lore are reflective; and station identification is authoritative. Kokoro realizes this as bounded pacing only. Parler receives a fixed, reviewed style description and can synthesize more expressive delivery; users cannot inject arbitrary voice descriptions.

On Windows, ordinary paths and Explorer's quoted **Copy as path** form are both accepted; one surrounding pair of double quotes is removed when settings are saved. Sanymar canonicalizes the resulting directory for containment checks and removes the `\\?\` verbatim prefix only before passing paths to Sherpa-ONNX, whose native asset validator expects regular DOS or UNC paths.
