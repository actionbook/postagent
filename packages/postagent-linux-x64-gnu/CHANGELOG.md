# postagent-linux-x64-gnu

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
