# Threat model

| Threat                         | Foundation control                                                           | Required integration review                                                      |
| ------------------------------ | ---------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| OAuth token theft              | Tokens excluded from SQLite/UI/logs and stored in Windows Credential Manager | Refresh lifecycle and release audit                                              |
| Loopback callback interception | PKCE, random state, fixed loopback port/path, one-shot bounded listener      | Live adversarial callback testing                                                |
| Malicious redirect             | Exact scheme/host/port/path validation; query/fragment/userinfo rejection    | Re-check registered URI before release                                           |
| Leaked logs                    | Structured categories; secret-redaction helper                               | Audit HTTP error bodies and tracing fields                                       |
| Prompt injection in facts      | Facts are labeled data; generator has no tools/credentials                   | Escape/structure prompts; adversarial tests; no instruction following from facts |
| Malformed API responses        | Typed adapter boundary                                                       | Size limits, strict deserialization, timeouts, validation                        |
| Unsafe file paths              | Canonical fixed model assets and bounded internal WAV artifacts              | App-data-only generated paths, canonicalization, random names, cleanup           |
| Dependency compromise          | Narrow dependency set and lockfile                                           | Audit updates, provenance, vulnerability scanning                                |
| Build-asset substitution       | Fixed official Kokoro URL, byte length, archive and extracted-file SHA-256   | Review manifest changes and preserve model/eSpeak attribution                    |
| Excessive permissions          | Minimal Tauri capability                                                     | Review every plugin/capability addition                                          |
| Data retention                 | Schema separates history/scripts                                             | User-visible retention and deletion transaction                                  |

Trust boundaries are the webview IPC, external provider responses, OAuth callback, local LLM output, generated audio path, OS credential vault, and SQLite file. A simple script validator reduces risk but cannot guarantee factual correctness.
