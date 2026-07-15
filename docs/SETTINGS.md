# Settings

Sanymar persists typed, non-secret application settings in local SQLite. Spotify OAuth tokens are stored separately in Windows Credential Manager. Complete prompts and credentials are not settings and are not logged.

## Music playback

- `mockMode`: when enabled, the dashboard uses deterministic fixture tracks. When disabled, it reads the connected Spotify account. This setting does not select the script generator.
- `spotifyClientId`: the public Spotify application Client ID. Do not enter a client secret.
- `spotifyRedirectUri`: fixed to `http://127.0.0.1:43821/callback` and validated exactly.

## Script generation

- `useOllama`: when disabled, deterministic `MockScriptGenerator` remains active. When enabled, the local Ollama adapter implements the same `ScriptGenerator` trait.
- `ollamaBaseUrl`: must be an HTTP loopback URL (`127.0.0.1`, `localhost`, or another loopback IP) with an explicit port and no path, credentials, query, or fragment. Default: `http://127.0.0.1:11434`.
- `ollamaModel`: exact name of an already installed model, including a tag where applicable. Sanymar checks `/api/tags` but never downloads a missing model.
- `maximumSegmentWords`: hard validation limit from 1 to 150 words. The editorial target may be shorter.

Use **Check Ollama** after saving the base URL and model. A ready result means `/api/version` was reachable and `/api/tags` contained the exact configured model name. Enabling Ollama requires a model name, but saving does not automatically contact, install, or start Ollama.

Generation is deterministic (`temperature: 0`) and strictly validated. If a draft violates the JSON contract, word limit, recent-phrase exclusions, formatting rules, or verified-fact references, Sanymar makes one corrective retry. Over-length drafts retry at 75% of `maximumSegmentWords`. If the corrected output remains locally invalid, the segment safely becomes silence; provider and configuration failures remain visible. Rejected drafts are never displayed, resent, or logged.

## Other foundation settings

`automaticTransitionSpeech` enables the backend transition monitor in live Spotify mode. It is off by default. When enabled, Sanymar prepares one spoken commentary segment as soon as a stable current/queued track pair is available, synthesizes it ahead of time, and starts the voice late enough to finish around the Spotify track boundary. Automatic mode does not editorially select silence; a track or queue change can still cancel stale generation, synthesis, or playback. Spotify is not paused or ducked.

`talkFrequency` controls target commentary length and editorial pacing in automatic mode; manual/editorial selection may still use it to choose silence. Recent segment/fact/opening memory is preserved to limit repetition. `musicbrainzContact` is an email address or HTTPS contact URL stored locally and sent only in MusicBrainz's required identifying User-Agent. If it is empty, live metadata lookup is skipped. `cacheRetentionDays` controls positive and negative fact-cache freshness from 1 to 3650 days.

## English speech synthesis

- `ttsProvider`: `mock` by default, `sherpa_kokoro` for in-process Kokoro, or `parler_mini` for the manually started local Parler service.
- `ttsModelDirectory`: absolute path to an extracted `kokoro-en-v0_19` directory containing `model.onnx`, `voices.bin`, `tokens.txt`, and `espeak-ng-data/`. Files are canonicalized and must remain inside this directory.
- `ttsVoiceId`: zero-based speaker ID reported by the selected Kokoro package. The health check rejects IDs outside the model's speaker count.
- `parlerBaseUrl`: strict HTTP loopback address for the user-managed Parler service. Default: `http://127.0.0.1:43822`. Credentials, paths, queries, fragments, and non-loopback hosts are rejected.
- `parlerSpeaker`: one of the service's reviewed built-in speakers: `Jon`, `Gary`, `Mike`, `Lea`, or `Jenna`. Default: `Jon`. This is not a voice-cloning or reference-audio field.
- `ttsSpeedPercent`: base synthesis speed from 50 to 200; default 100. Sanymar applies a small segment-aware delivery factor and clamps the effective rate to the same safe range.

Generated WAV files are stored beneath the application cache, not in the model directory. Models are never downloaded automatically. Real speech uses the Windows default output device; `audioOutputDevice` remains a placeholder for future explicit selection. Debug logging never enables complete prompt, provider-response, dialogue, or credential logging.

Delivery style is derived from the editorial segment and is not a persisted user setting. It never switches the selected speaker: next-song teases, one-line reactions, and simple transitions are energetic; short jokes are playful; listener observations stay warm; stories and lore are reflective; and station identification is authoritative. Kokoro realizes this as bounded pacing only. Parler receives a fixed, reviewed style description and can synthesize more expressive delivery; users cannot inject arbitrary voice descriptions.

On Windows, ordinary paths and Explorer's quoted **Copy as path** form are both accepted; one surrounding pair of double quotes is removed when settings are saved. Sanymar canonicalizes the resulting directory for containment checks and removes the `\\?\` verbatim prefix only before passing paths to Sherpa-ONNX, whose native asset validator expects regular DOS or UNC paths.
