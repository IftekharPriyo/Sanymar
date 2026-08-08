# Sanymar agent guide

## Purpose and phase

Sanymar is a Windows desktop AI radio jockey. The current phase includes the reviewable foundation, Spotify playback monitoring through PKCE, active cloud Qwen generation through Groq, a retained development-only Ollama adapter, unattended confidence-gated MusicBrainz metadata, bundled English Kokoro synthesis, development-only Parler-TTS Mini synthesis, default-device voice playback, and opt-in pre-rendered transition speech with recovery-aware pause/skip/seek/resume. General Spotify controls are not exposed. Listener setup should require only Spotify authorization plus a user-provided Groq API key; provider modes remain independently selectable in development and all offline mocks must be preserved.

Before changing code, read this file, `docs/ARCHITECTURE.md`, `docs/DECISIONS.md`, and the relevant module. Project-focused references live in `.agents/skills/` even if the active agent does not discover them automatically.

## Stack and structure

- Tauri 2, Rust, Tokio, Serde, SQLx/SQLite
- React, strict TypeScript, Vite, plain CSS
- Rust code: `src-tauri/src/`; migrations: `src-tauri/migrations/`
- UI: `src/`; documentation: `docs/`
- Domain modules: music provider, facts, RJ engine, LLM, TTS, audio, playback, database, security, settings, commands, errors

The application is a modular monolith. React renders state and calls typed commands. Thin commands delegate to Rust application/domain services. Provider-specific payloads must be normalized at adapter boundaries. Do not move orchestration into React, Tauri handlers, `main.rs`, or `lib.rs`.

## Required commands

```powershell
npm.cmd install
npm.cmd run dev
npm.cmd run tauri dev
npm.cmd run test
npm.cmd run lint
npm.cmd run format:check
npm.cmd run build
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --all-features -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
cargo check --manifest-path src-tauri/Cargo.toml
```

Run formatting, linting, tests, and compilation checks appropriate to a change. Report checks that the environment prevents.

## Security and architecture rules

1. Do not change architecture silently; record significant choices in `docs/DECISIONS.md`.
2. Explain every new dependency and prefer existing dependencies or the standard library.
3. Never log credentials, tokens, authorization codes, complete prompts, or secrets.
4. Never store OAuth tokens in SQLite or frontend `localStorage`; production providers must use OS credential storage.
5. Do not implement arbitrary command or shell execution.
6. Domain logic must not depend on Spotify response types.
7. Keep Tauri commands and React components thin.
8. Do not use `unwrap()` or `expect()` in normal runtime paths.
9. Treat retrieved facts as untrusted input and retain attribution.
10. Validate redirects, API responses, URLs, and generated file paths at trust boundaries.
11. Use least-privilege Tauri capabilities.
12. Preserve user-written content and unrelated changes.

## Provider and migration rules

- Define or update the domain interface before adding an external adapter.
- Normalize external data, use typed errors, timeouts, cancellation, and provider-specific tests.
- Keep a mock implementation so development works offline; never imply the mock is real.
- Ollama adapters must remain loopback-only, must not auto-install models, and must never log complete prompts.
- Ollama output correction is limited to one validation-only retry; never include the rejected model response in that retry or weaken the final validator.
- A final locally invalid Ollama result must fail soft to an explicitly silent segment; do not hide provider, configuration, timeout, or cancellation failures under that policy.
- Groq Qwen adapters must use HTTPS, store API keys only in OS credential storage, never log or return keys, send only the sanitized script request, and preserve the same strict validation/correction policy as Ollama.
- MusicBrainz lookup is cache-first, contact-identified, rate-limited, and unattended. Ambiguous matches fall back to no facts; do not add a mandatory review workflow.
- The reviewed English Kokoro pack is an installer-managed asset with retained third-party notices. Release tooling may stage only its fixed official archive after pinned size/SHA-256 verification; never commit its binary pack to ordinary Git. The installed application must validate it at runtime and never replace or download it. Other TTS models remain explicit user-managed assets. Never accept voice cloning/reference audio or write generated audio outside the application cache.
- Keep TTS delivery intent typed and provider-neutral. Preserve the user's selected voice, bound adapter-specific prosody controls, and do not describe pacing-only Kokoro output as genuine emotion synthesis.
- Parler is an explicitly user-started, loopback-only provider process. The application must never launch or manage Python, accept reference audio, expose arbitrary descriptions, or download the model. Keep its request surface allowlisted and its returned audio bounded and validated.
- Native audio may play only validated local artifacts from internal providers. Spotify pause/skip/seek/resume is authorized only by the explicit automatic-transition setting, must target the observed device, must never overlap audio, and must retain resume recovery.
- Add all schema changes as new SQLx migration files. Do not edit migrations after shared use.
- Ask before destructive migrations. Do not store duplicated provider payloads without justification.

## Testing and dependency expectations

Add tests for meaningful business rules, cancellation, normalization, error mapping, and secret redaction. Prefer deterministic clocks/randomness. Frontend business logic belongs in tested services or hooks. New dependencies must be maintained, non-duplicative, Windows-compatible, and documented if architecturally meaningful.

## Prohibited shortcuts

- No Next.js, separate backend, web scraping, vector database, cloud default, music downloading, voice cloning, or hidden auto-installation.
- No `any`, raw-string error surfaces, token-bearing logs, invented integration success, or all-purpose service files.
- Do not bypass script validation or play a result whose job/track identity is stale.

## Definition of done

A scoped change is implemented through the correct boundary, has meaningful tests, preserves mock-mode launch, passes available format/lint/test/build checks, labels mocked behavior, updates relevant documentation, and reports limitations honestly. Architecture changes also update `docs/ARCHITECTURE.md` and `docs/DECISIONS.md`.
