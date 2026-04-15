# Postagent

Postman CLI, but for AI agents. An HTTP API tool built for agent workflows.

This document is the shared handbook for the repository — conventions, commands, and release flow — intended for both human contributors and AI agents. `AGENTS.md` points here.

## Repository layout

pnpm monorepo (`pnpm@10.32.1`) with one Rust core crate and several npm packages:

```
packages/
  postagent/                # TS CLI entry; pulls the right platform binary via optionalDependencies
  postagent-core/           # Rust implementation; produces the postagent-core executable
  postagent-darwin-arm64/   # Platform binary package (macOS arm64)
  postagent-darwin-x64/     # Platform binary package (macOS x64)
  postagent-linux-arm64-gnu/# Platform binary package (Linux arm64)
  postagent-linux-x64-gnu/  # Platform binary package (Linux x64)
```

All npm packages share a **single synchronized version** (enforced via the Changesets `fixed` group). The Rust crate's `Cargo.toml` version is kept in lockstep with the npm packages.

## Common commands

```bash
pnpm install                  # Install all dependencies
pnpm build                    # Recursively build all npm packages
pnpm dev:watch                # cargo watch on the Rust core during development
pnpm --filter postagent dev   # Run the TS CLI in dev mode
pnpm --filter postagent typecheck

# Version management
pnpm changeset                # Create a new changeset (required for release-worthy changes)
pnpm changeset status         # Inspect pending changesets
pnpm version-packages         # Preview version bumps locally (normally run by CI)
```

## Commit conventions

Follow [Conventional Commits](https://www.conventionalcommits.org/). Format:

```
<type>(<scope>): <subject>

<body>

<footer>
```

- **type**: `feat`, `fix`, `chore`, `docs`, `refactor`, `test`, `build`, `ci`, `perf`, `style`, `revert`.
- **scope** (optional): the affected module or package. Common values: `manual`, `search`, `send`, `auth`, `core`, `postagent`, `release`.
- **subject**: imperative mood, lowercase, no trailing period, ≤72 characters.
- **body** (optional): explain *why* the change was made, not what it does — the diff already shows the "what".
- **footer** (optional): `Closes #N`, `Refs #N`, `BREAKING CHANGE: ...`.
- Any change that affects user-visible npm package behavior **must** ship with a changeset file produced by `pnpm changeset`. Internal-only work (CI, docs, tooling) can skip it.

Examples from the repo history:

```
fix(manual): cap walk_schema recursion depth to guard pathological specs
feat(manual): expand nested object schemas with dot notation
chore: bump version to 0.2.1
```

## Pull Request conventions

- **Title**: same format as a commit message (`<type>(<scope>): <subject>`). It becomes the squash-merge subject, so treat it as a commit message.
- **Body**: use the structure below. Keep each section tight; skip one only if it genuinely does not apply.
- Keep each PR focused on a single purpose. Do not bundle unrelated refactors or formatting.
- CI must be green before merge.
- Breaking changes must be called out explicitly in both the PR body and the changeset, using a `BREAKING CHANGE:` footer.

### PR body format

```markdown
## Summary
One or two sentences: what this PR does and why it is needed.

## Changes
- Bullet list of the concrete changes
- Group by module / package when it helps readability

## Test Plan
- How this PR was verified (commands run, scenarios exercised)
- Link to CI run or screenshots when relevant

Closes #<issue-number>
```

Rules of thumb when filling it out:

- **Summary** answers "why", not "what". The diff already shows what.
- **Changes** is a scannable bullet list, not prose. One bullet per logical change.
- **Test Plan** must be concrete and reproducible. "Tested manually" is not enough — say what you ran.

## Versioning and release

This project uses [Changesets](https://github.com/changesets/changesets) for version management and changelogs.

### Automated release flow

Every push to `main` triggers `.github/workflows/release.yml`:

- **When there are pending changesets**: `changesets/action` opens (or updates) a `chore(release): version packages` PR. That PR runs `changeset version` to bump every npm package version, syncs the Rust `Cargo.toml` version, updates `CHANGELOG.md`, and refreshes the lockfile.
- **When there are no pending changesets** (i.e. the Version PR was just merged): the workflow reads the new version number, creates and pushes a `v{version}` tag, and then dispatches `publish.yml` explicitly via `gh workflow run publish.yml --ref v{version}`. `publish.yml` performs the multi-platform build and publishes to npm.

> Note: tags pushed by the default `GITHUB_TOKEN` do NOT cascade into other workflow triggers, so the release workflow has to dispatch `publish.yml` explicitly.

The real gate for "should we publish now?" is not literally "no changesets", and it is not literally "the tag does not exist" either. The workflow queries the actual run history of `publish.yml` filtered by `headBranch == v{version}`:

- No prior run → create the tag if missing and dispatch.
- Latest run is `in_progress` / `queued` → do nothing, let it finish.
- Latest run `completed / success` → no-op.
- Latest run `completed / failure` (or cancelled / timed out) → **do nothing, require manual recovery**.

`npm publish` is not idempotent here: `publish.yml` publishes 5 packages sequentially, and a partial publish (some packages released, others not) would wedge any retry on "version already exists". Auto-redispatching a failed run would turn a recoverable partial-publish into a permanently stuck release. Instead, the workflow logs the failure URL and stops; a human must inspect which packages actually landed on npm, manually publish the stragglers (or cut a new version bump), and only then re-enable the pipeline.

Merging an unrelated no-changeset PR after a successful release is still a safe no-op because the last `publish.yml` run for that tag is marked successful.
