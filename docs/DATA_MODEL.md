# Data model

The initial migration is `src-tauri/migrations/0001_initial.sql`.

- `application_settings`: non-secret typed setting values.
- `dj_profiles`: serializable personality JSON and active status.
- `artists`, `albums`, `tracks`, `track_artists`: normalized catalog identity and ordered credits.
- `fact_sources`, `music_facts`, `track_facts`: attributed facts and track association.
- `music_facts.verification_method`: distinguishes unverified, human-reviewed, and authoritative automated metadata.
- `fact_lookup_cache`: fresh positive/negative lookup markers; raw provider responses are not stored.
- `broadcast_history`: track, artist summary, segment type, opening phrase, job, and outcome.
- `used_facts`: when a fact was used in a broadcast.
- `generated_scripts`: dialogue, validation state, source-fact references, and job identity.
- `provider_auth_metadata`: connection state, scopes, expiry, and vault key reference only.

OAuth access/refresh tokens are forbidden in this database. Timestamps use UTC ISO-8601 text for portability. Foreign keys and targeted lookup indexes support recent-history and fact-cache queries without prematurely optimizing.
