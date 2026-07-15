# Development plan

## Phase 1 — foundation (current)

Architecture/docs, Tauri/React skeleton, domain types/traits, mocks, deterministic editorial logic, validation, cancellation model, SQLite migrations/repository, mock UI, and foundation tests.

## Phase 2 — secure Spotify connection

Register a user-owned Spotify application; implement Authorization Code + PKCE, strict loopback callback validation, OS credential storage, refresh, normalized playback/queue APIs, policy-aware error handling, and integration tests with recorded non-sensitive fixtures.

## Phase 3 — local generation and facts

Implement Ollama health/model selection and generation, curated facts, unattended confidence-gated MusicBrainz lookup/cache, prompt construction with trust labels, stronger validation, and timeouts/cancellation. A mandatory review UI is intentionally excluded because unattended operation must fall back safely on ambiguity.

## Phase 4 — speech and coordination

English Kokoro and user-managed loopback Parler-TTS Mini now produce validated, cancellation-aware WAV artifacts, and Rodio plays real artifacts on the Windows default device. Next implement cache cleanup, transition timing, Spotify pause/resume recovery, and end-to-end stale-output tests. Parler packaging and automatic process management remain intentionally out of scope.

## Phase 5 — hardening

Retention controls, deletion flows, richer telemetry controls, Playwright/Tauri UI coverage, packaging/signing, accessibility, dependency/security review, and failure recovery exercises.

Each phase requires human review before the next begins.
