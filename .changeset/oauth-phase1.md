---
"postagent": minor
"postagent-darwin-arm64": minor
"postagent-darwin-x64": minor
"postagent-linux-arm64-gnu": minor
"postagent-linux-x64-gnu": minor
---

feat(auth): add OAuth 2.0 support with BYO apps, multi-method selection, and backward-compatible templates.

- `postagent auth <site>` now reads the descriptor from the registry and drives a BYO OAuth 2.0 Authorization Code + PKCE flow when the site advertises OAuth. Static API-key specs prompt exactly as before.
- When a site lists multiple methods (e.g. GitHub PAT + OAuth App), the CLI offers an interactive picker on TTYs and falls back to a numbered prompt on non-TTY input; `--method <id>` still selects non-interactively.
- New flags on `auth`: `--method`, `--client-id`, `--client-secret`, `--dry-run`, `--param K=V` (repeatable), `--scope` (repeatable).
- New subcommands: `postagent auth <site> logout`, `postagent auth <site> reset`, `postagent auth <site> status`, `postagent auth <site> scopes`, `postagent auth list`.
- New templates in `postagent send`: `$POSTAGENT.<SITE>.TOKEN` and `$POSTAGENT.<SITE>.ACCESS_TOKEN` (OAuth), `$POSTAGENT.<SITE>.EXTRAS.<NAME>` for provider-specific values like Notion `bot_id`. Existing `$POSTAGENT.<SITE>.API_KEY` templates continue to resolve; for OAuth sites they fall back to the access token with a one-time warning.
- Local storage adds `~/.postagent/profiles/default/<site>/app.yaml` for OAuth app credentials alongside the existing `auth.yaml`. Legacy `auth.yaml` files with only `api_key: xxx` keep loading unchanged.
- `postagent send` hints at re-authenticating on HTTP 401 / 403 responses and names the saved sites referenced in the request.
- `postagent send` refuses to resolve `$POSTAGENT.*` credentials into non-HTTPS requests except loopback `http://localhost` / `127.0.0.1` / `[::1]` URLs for local testing, and it no longer auto-forwards the global Actionbook `x-api-key` header to third-party APIs.
- Provider-backed OAuth keeps the old site-local credentials active until the browser flow succeeds, then links the site into shared provider storage; switching that site back to a static token detaches it from the shared provider store instead of overwriting sibling sites' shared OAuth tokens, and `logout` / `reset` now warn when they will clear shared provider credentials for sibling sites.
- `postagent auth <site> --token` and the interactive static-token prompts reject blank / whitespace-only credentials before writing `auth.yaml`.
- Required OAuth `--param key=value` inputs now fail fast when the value is blank, and the `client_id` prompt no longer consumes an extra stdin line before reading piped credentials.
- `postagent auth <site> --dry-run` and browser-open failures now write the full authorize URL to a 0600 temp file instead of echoing it to stderr.
- OAuth scope selection supports an interactive multi-select picker, preserves default scopes even when the registry catalog is incomplete, and the CLI prints the final scope set before opening the browser.
- The local loopback callback listener now ignores stray localhost hits that are not valid `/callback` OAuth responses instead of failing the auth flow early.
- `postagent manual <site>` renders every advertised auth method with the right template per method (bearer headers for static, OAuth `inject.value_template`, and `$POSTAGENT.<SITE>.EXTRAS.<NAME>` for extra token fields).
- `$POSTAGENT.<SITE>.TOKEN.EXTRA`-style typos now fail fast; only `EXTRAS.<NAME>` accepts a suffix.
