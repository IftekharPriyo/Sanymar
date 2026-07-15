# Product specification

Sanymar is a personal, local-first desktop radio jockey. It observes authorized music playback, sometimes prepares short original commentary, synthesizes the commentary locally, coordinates a transition, and records enough recent history to avoid repetition.

## Product principles

- The fictional host is human-sounding, concise, original, and music-aware—not a metadata announcer.
- Silence is a valid editorial choice.
- The language model is a writer, never a factual source or playback controller.
- Factual claims require supplied, attributed facts; subjective reactions and clearly fictional station lore are separate inputs.
- Unattended metadata lookup must reject ambiguity and fall back to non-factual commentary; it must not wait for manual fact review.
- Tokens remain in OS-backed credential storage. Listening history and generation remain local by default.

## MVP outcome

The eventual MVP connects Spotify with Authorization Code + PKCE, observes current/next tracks, prepares optional commentary, validates and speaks it, safely resumes playback, and retains anti-repetition memory. It uses normalized interfaces for Spotify, facts, LLM, TTS, audio, and credentials.

## Foundation-phase scope

This repository currently provides the modular monolith, domain models, provider traits and mocks, deterministic content director, validator, cancellation/staleness model, SQL migrations/repository, Spotify metadata, Ollama, MusicBrainz, Kokoro and Parler synthesis, default-device voice playback, and a runnable UI. Automated Spotify transition control remains deferred.

## Non-goals

No cloud accounts, mobile app, broadcasting, music storage/mixing, voice cloning, recommendation engine, scraping, vector database, microservices, separate web backend, Next.js, containers, autonomous shell access, or general agent framework.

## Default personality

Mira Vale hosts fictional station Night Current. She is calm, curious, lightly dry, and attentive to musical texture. Her default segments are 12–42 words, she avoids encyclopedia phrasing and forced excitement, and her lore concerns a tiny studio above a late-night tea shop. This is an original development profile, not an imitation.
