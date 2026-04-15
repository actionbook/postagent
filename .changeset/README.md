# Changesets

This directory holds unreleased change descriptions (changesets).

## How to add a changeset

```bash
pnpm changeset
```

Follow the prompts to pick the affected packages, the bump type (patch / minor / major), and a one-line summary. A `*.md` file is generated in this directory — commit it alongside your code changes.

See [`CLAUDE.md`](../CLAUDE.md#versioning-and-release) in the repo root for the full release flow.

## Conventions

- This repo uses `fixed` mode: the 5 npm packages share a single locked version, and bumping any one of them bumps them all.
- The Rust crate `postagent-core` (`Cargo.toml`) is not managed directly by Changesets; `scripts/sync-versions.mjs` keeps it in lockstep during `pnpm version-packages`.
- **Do not hand-edit the `version` field in `packages/*/package.json`** — let the Version Packages PR handle it.
