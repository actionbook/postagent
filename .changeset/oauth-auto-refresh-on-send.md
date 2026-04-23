---
"postagent": minor
"postagent-darwin-arm64": minor
"postagent-darwin-x64": minor
"postagent-linux-arm64-gnu": minor
"postagent-linux-x64-gnu": minor
---

feat(send): auto-refresh expired OAuth tokens once on 401/403.

- When `postagent send` gets a 401 or 403 from the upstream API and the request used a `$POSTAGENT.<SITE>.TOKEN` (or `.ACCESS_TOKEN`) backed by an OAuth credential with a stored refresh_token, the CLI now silently exchanges the refresh_token for a new access_token, persists it to `auth.yaml`, re-resolves the templates, and retries the request once.
- Static (PAT-style) credentials are unaffected — they have nothing to refresh, so the existing "your access token may be expired" hint is printed as before.
- Provider-shared sites (e.g. `google-drive` and `google-docs` both pointing at `providers/google`) refresh at most once per provider so a rotating refresh_token is not spent twice in a single retry.
- Refresh failures (no saved refresh_token, descriptor mismatch, token endpoint 4xx/5xx) are reported on stderr but do not loop — the original 401/403 response is surfaced as before.
- The OAuth token-endpoint POST (body_encoding, client_auth, response_map) is now shared between the initial authorization_code exchange and the refresh path, so descriptor edits propagate without code changes.
