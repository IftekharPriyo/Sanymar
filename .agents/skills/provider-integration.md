# Provider integration reference

- Define the normalized interface first and preserve the mock.
- Keep external payloads inside the adapter; validate and normalize responses.
- Add timeouts, cancellation, rate-limit behavior, and provider-specific tests.
- Never expose credentials or log sensitive request/response bodies.
- Report a provider as real only after its actual health and contract are verified.
