# AGENTS.md

Agent collaboration rules for this repository mirror [`CLAUDE.md`](./CLAUDE.md) — please read it for the full picture, including:

- Repository layout and common commands
- Commit message conventions (Conventional Commits)
- Pull Request template and requirements
- Changesets-based versioning and release flow

## Core conventions at a glance

- **Any change that affects user-visible behavior must ship with a `pnpm changeset`** file. CI-only, docs-only, or internal refactors may skip it.
- **Commit format**: `<type>(<scope>): <subject>` — imperative mood, lowercase, ≤72 characters.
- **PR title** follows the commit format. **PR description** contains three sections: Summary / Changes / Test Plan.
- **Version consistency**: the 5 npm packages plus `postagent-core`'s `Cargo.toml` must share the same version. This is maintained automatically by the CI Version Packages PR — do not edit versions by hand.
- **Release entry point**: push to `main` → Changesets Version PR → merge the PR → CI tags `v{version}` → `publish.yml` runs the multi-platform build and publishes to npm.
