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
- Provider-backed OAuth now links the site to the shared provider `app.yaml` as soon as BYO app credentials are saved, so `auth status` / `reset` can see that staged shared app during dry-runs or failed browser flows without hiding any still-active site-local token until the shared OAuth token actually exists; `logout` / `reset` also clear any stale site-local auth that would otherwise come back after the shared provider token is removed.
- `postagent auth <site> --token` and the interactive static-token prompts reject blank / whitespace-only credentials before writing `auth.yaml`.
- OAuth `client_id` / `client_secret` prompts and flags now reject blank / whitespace-only values before writing `app.yaml`.
- OAuth now reuses a saved `client_secret` only when it belongs to the same auth method as the selected `client_id`, so switching methods cannot silently mix credentials.
- Required OAuth `--param key=value` inputs now fail fast when the value is blank, and the `client_id` prompt no longer consumes an extra stdin line before reading piped credentials.
- `postagent auth <site> --dry-run` and browser-open failures now write the full authorize URL to a 0600 temp file instead of echoing it to stderr, and `--dry-run` exits immediately instead of waiting for a localhost callback that will never arrive.
- The localhost OAuth callback listener now binds port `9876` before the browser is opened, avoiding race conditions where a fast provider redirect could hit a closed port before the CLI starts listening.
- OAuth scope selection supports an interactive multi-select picker, preserves default scopes even when the registry catalog is incomplete, and the CLI prints the final scope set before opening the browser.
- OAuth descriptor `authorize.extra_params` can add provider-specific query params, but it can no longer override reserved OAuth safety params like `state`, `redirect_uri`, or `code_challenge`.
- Interactive OAuth pickers now buffer split arrow-key escape sequences before treating `Esc` as cancel, so slow terminals do not accidentally dismiss the method or scope selector.
- The local loopback callback listener now ignores stray localhost hits that are not valid `/callback` OAuth responses instead of failing the auth flow early.
- Invalid `auth_methods` payloads from `/api/manual` now fail fast instead of silently downgrading OAuth sites into the legacy static-token prompt flow.
- `postagent manual <site>` renders every advertised auth method with the right template per method, including OAuth `injects[].in` locations for headers, query params, and cookies, plus `$POSTAGENT.<SITE>.EXTRAS.<NAME>` for extra token fields.
- `$POSTAGENT.<SITE>.TOKEN.EXTRA`-style typos now fail fast; only `EXTRAS.<NAME>` accepts a suffix.
