# Rust core reference

- Keep domain types independent of external API models.
- Use typed errors and structured, redacted logs.
- Accept cancellation for long work and bind artifacts to job/track identity.
- Keep Tauri handlers thin; orchestration belongs in `rj_engine`/application services.
- Add unit tests for meaningful rules; avoid runtime `unwrap()` and `expect()`.
