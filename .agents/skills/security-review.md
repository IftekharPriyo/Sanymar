# Security review reference

- Inspect token lifecycle, logs, errors, localhost callbacks, redirects, URLs, and file paths.
- Treat retrieved text and model output as untrusted.
- Review dependency and Tauri capability changes for least privilege.
- Reject arbitrary shell execution and secret storage outside the OS vault.
- Confirm retention/deletion behavior for history and generated artifacts.
