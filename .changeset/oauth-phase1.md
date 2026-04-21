---
"postagent": minor
"postagent-darwin-arm64": minor
"postagent-darwin-x64": minor
"postagent-linux-arm64-gnu": minor
"postagent-linux-x64-gnu": minor
---

feat(auth): add OAuth 2.0 support with BYO apps, multi-method selection, and backward-compatible templates.

- `postagent auth <site>` now reads the descriptor from the registry and drives a BYO OAuth 2.0 Authorization Code + PKCE flow when the site advertises OAuth. Static API-key specs prompt exactly as before.
- When a site lists multiple methods (e.g. GitHub PAT + OAuth App), the CLI prompts to pick one, or `--method <id>` selects non-interactively.
- New flags on `auth`: `--method`, `--client-id`, `--client-secret`, `--dry-run`, `--param K=V` (repeatable), `--scope` (repeatable).
- New subcommands: `postagent auth <site> logout`, `postagent auth <site> reset-app`, `postagent auth <site> status`, `postagent auth list`.
- New templates in `postagent send`: `$POSTAGENT.<SITE>.TOKEN` and `$POSTAGENT.<SITE>.ACCESS_TOKEN` (OAuth), `$POSTAGENT.<SITE>.EXTRAS.<NAME>` for provider-specific values like Notion `bot_id`. Existing `$POSTAGENT.<SITE>.API_KEY` templates continue to resolve; for OAuth sites they fall back to the access token with a one-time warning.
- Local storage adds `~/.postagent/profiles/default/<site>/app.yaml` for OAuth app credentials alongside the existing `auth.yaml`. Legacy `auth.yaml` files with only `api_key: xxx` keep loading unchanged.
- `postagent send` hints at re-authenticating on HTTP 401 / 403 responses and names the saved sites referenced in the request.
- `postagent auth <site> --token` and the interactive static-token prompts reject blank / whitespace-only credentials before writing `auth.yaml`.
- `postagent auth <site> --dry-run` and browser-open failures now write the full authorize URL to a 0600 temp file instead of echoing it to stderr.
- `postagent manual <site>` renders every advertised auth method with the right template per method (bearer headers for static, OAuth `inject.value_template`, and `$POSTAGENT.<SITE>.EXTRAS.<NAME>` for extra token fields).
- `$POSTAGENT.<SITE>.TOKEN.EXTRA`-style typos now fail fast; only `EXTRAS.<NAME>` accepts a suffix.
