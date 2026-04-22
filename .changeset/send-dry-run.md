---
"postagent": minor
"postagent-darwin-arm64": minor
"postagent-darwin-x64": minor
"postagent-linux-arm64-gnu": minor
"postagent-linux-x64-gnu": minor
---

feat(send): add `--dry-run` to preview the final request without sending it.

- `postagent send ... --dry-run` runs the full preprocessing pipeline (`$POSTAGENT.*` template substitution, method inference, header merging, User-Agent injection) and prints the resolved method, URL, headers, and body, but makes no outbound request.
- Auto-injected headers (e.g. `User-Agent`) are marked in the output with `[auto-injected]`.
- Sensitive headers (`Authorization`, `Cookie`, `Set-Cookie`, `Proxy-Authorization`, `x-api-key`, and any name matching `*secret*`, `*password*`, `*-token`, `*-key`, `*-auth`) are redacted; `Bearer`/`Basic`/`Digest`/`Token` scheme prefixes are preserved.
- Sensitive URL query parameters (`token`, `access_token`, `api_key`, `password`, `secret`, `client_secret`, `sig`/`signature`, and any `*_key`/`*-key` name) are redacted. URL userinfo passwords are redacted (`alice:***@host`). Benign query params pass through unchanged.
- JSON bodies have top-level keys matching `password`, `token`, `secret`, `credential`, or `api_key` redacted.
- Exit code is `0` for a successful dry run and non-zero for invalid templates, invalid URLs, or other prepare-time errors.
- Request preparation is factored into a shared `request_preview::PreparedRequest` so future commands can reuse the preview pipeline.
