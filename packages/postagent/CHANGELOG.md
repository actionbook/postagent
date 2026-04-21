# postagent

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

### Patch Changes

- [#13](https://github.com/actionbook/postagent/pull/13) [`c41a12d`](https://github.com/actionbook/postagent/commit/c41a12d137a6fbec6d0d1fe29d5b0c5d9718d3ee) Thanks [@4bmis](https://github.com/4bmis)! - fix: manual will render ref types correctly in detail of actions

## 0.2.2

### Patch Changes

- 0d1f496: Manual rendering fixes (nested objects, oneOf/anyOf, enums, recursion guard) and backend auth fixes (x-api-key injection, blank key handling).

## 0.2.1

### Patch Changes

- Show `--token` parameter in `auth` command help output.
- Drop `win32-x64` platform support.
- Fix stale `pnpm-lock.yaml` introduced by the previous release.

## 0.2.0

### Minor Changes

- Initial public release of the `postagent` CLI.
- Commands: `search`, `manual`, `auth`, `send`, `config`.
- `send` auto-injects `x-api-key` from config and expands `$POSTAGENT.SITE.API_KEY` placeholders, keeping credentials out of the LLM context.
- Cross-platform prebuilt binaries via optional dependencies: darwin arm64/x64, linux x64/arm64-gnu.
