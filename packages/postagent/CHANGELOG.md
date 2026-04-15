# postagent

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
