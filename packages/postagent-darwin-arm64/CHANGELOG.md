# postagent-darwin-arm64

## 0.4.0

### Minor Changes

- [#19](https://github.com/actionbook/postagent/pull/19) [`ed62224`](https://github.com/actionbook/postagent/commit/ed622245639d65a459f321d873d190a3c85c33f2) Thanks [@4bmis](https://github.com/4bmis)! - feat(send): add `--dry-run` to preview the final request without sending it.

  - `postagent send ... --dry-run` runs the full preprocessing pipeline (`$POSTAGENT.*` template substitution, method inference, header merging, User-Agent injection) and prints the resolved method, URL, headers, and body, but makes no outbound request.
  - Auto-injected headers (e.g. `User-Agent`) are marked in the output with `[auto-injected]`.
  - Sensitive headers (`Authorization`, `Cookie`, `Set-Cookie`, `Proxy-Authorization`, `x-api-key`, and any name matching `*secret*`, `*password*`, `*-token`, `*-key`, `*-auth`) are redacted; `Bearer`/`Basic`/`Digest`/`Token` scheme prefixes are preserved.
  - Sensitive URL query parameters (`token`, `access_token`, `api_key`, `password`, `secret`, `client_secret`, `sig`/`signature`, and any `*_key`/`*-key` name) are redacted. Opaque credential-like URL path segments are redacted, URL fragments are filtered through the same conservative redaction rules, and any URL userinfo credentials are masked (`***:***@host`, `***@host`). Benign query params pass through unchanged.
  - Bodies are redacted conservatively: JSON fields with sensitive names are masked, form-encoded sensitive fields are masked, and opaque secret-like raw body payloads are replaced with `***`.
  - Exit code is `0` for a successful dry run and non-zero for invalid templates, invalid URLs, or other prepare-time errors.
  - Request preparation is factored into a shared `request_preview::PreparedRequest` so future commands can reuse the preview pipeline.

### Patch Changes

- [#21](https://github.com/actionbook/postagent/pull/21) [`c80ae6b`](https://github.com/actionbook/postagent/commit/c80ae6bc110aadb4533fb4abe9b3006ca4ad144d) Thanks [@4bmis](https://github.com/4bmis)! - fix: surface friendlier errors when the postagent server is slow or unreachable.

  - `postagent search` and `postagent manual` now use a dedicated HTTP client with a 45s request timeout, so a hung connection fails with an actionable message instead of hanging indefinitely behind reqwest's default (no timeout).
  - Request failures are categorized: timeouts print "Request to postagent server timed out after 45s. The server may be busy; try again in a few seconds.", connect failures print "Could not reach postagent server. Check your network connection, then try again.", and other reqwest errors fall through to a labelled "Request to postagent server failed: …" line so unusual cases stay diagnosable.
  - HTTP 502/503/504 responses from the postagent server now print a transient-retry hint ("This is usually transient; try again in a few seconds.") instead of being surfaced as a generic API error, which was misleading when the underlying cause was a cold-start or upstream blip.

## 0.3.0

### Minor Changes

- [#17](https://github.com/actionbook/postagent/pull/17) [`ec19302`](https://github.com/actionbook/postagent/commit/ec19302acf93d4cc26e5f79a12910fadf7e908d2) Thanks [@4bmis](https://github.com/4bmis)! - feat(auth): add OAuth 2.0 support with BYO apps and multi-method selection.

  - `postagent auth <site>` drives an Authorization Code + PKCE flow when the site advertises OAuth; static API-key sites prompt as before.
  - Interactive picker for sites with multiple auth methods; `--method <id>` selects non-interactively.
  - New `auth` flags: `--method`, `--client-id`, `--client-secret`, `--scope`, `--param K=V`, `--dry-run`.
  - New subcommands: `auth list` / `<site> status` / `scopes` / `logout` / `reset`.
  - New `send` templates: `$POSTAGENT.<SITE>.TOKEN`, `.ACCESS_TOKEN`, `.EXTRAS.<NAME>`. Existing `.API_KEY` templates keep working (OAuth sites fall back to the access token with a one-time warning).
  - `send` refuses to resolve `$POSTAGENT.*` into non-HTTPS URLs except loopback, and no longer auto-forwards the Actionbook `x-api-key` header to third-party APIs.
  - OAuth app credentials stored at `~/.postagent/profiles/default/<site>/app.yaml`; legacy `auth.yaml` continues to load unchanged.

## 0.2.3

## 0.2.2

## 0.2.1

## 0.2.0
