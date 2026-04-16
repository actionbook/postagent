---
name: vercel-seat-saver
description: Set up GitHub Actions to deploy Vercel apps via postagent. Uses postagent as the unified tool for both local setup and CI deployment — no Vercel CLI or GitHub CLI required.
version: 1.0.0
author: actionbook
license: MIT
metadata:
  hermes:
    tags: [vercel, postagent, deployment, github-actions, ci-cd]
    category: devops
---

# Vercel Deploy via GitHub Actions + Postagent

## Purpose

Help the user set up a GitHub Actions workflow that deploys their app to Vercel, using `postagent` as the unified tool in both the local setup phase and the CI runtime.

## When to use

- User wants CI/CD deployment to Vercel from GitHub Actions
- User wants to centralize deployment through a single API token instead of per-person Git integration
- User is migrating from Vercel's built-in GitHub connector to a self-managed pipeline

## Tools

`postagent` is your only tool for API operations, both locally and in CI. Key commands:

- `postagent manual <site>` — progressive API discovery (site → group → action → full schema)
- `postagent send <curl-style request>` — execute an API call; credential placeholders like `$POSTAGENT.VERCEL.API_KEY` are auto-resolved
- `postagent auth <site>` — save credentials interactively
- `postagent auth <site> --token <TOKEN>` — save credentials non-interactively (used in CI)

Available sites: `vercel` and `github`. Always use `postagent manual` to discover endpoints and parameters before building requests — do not guess.

## Prerequisites

Before starting, ensure locally:

1. `postagent` is installed — `npm i -g postagent@latest`
2. `postagent auth vercel` — Vercel API token is saved. To obtain a token: Vercel Dashboard > click avatar > Settings > Tokens. Collect the token through a secure input path — do not ask the user to paste it into normal chat.
3. `postagent auth github` — GitHub personal access token is saved. To obtain a token: GitHub > click avatar > Settings > Developer settings > Personal access tokens > Tokens (classic) > Generate new token (classic). At minimum, select the `repo` scope.

If auth is missing, guide the user through `postagent auth` first.

## Rules

- Use `postagent` for all Vercel and GitHub operations — do not add Python, Node.js, or shell wrapper scripts to send API requests when postagent can do it directly
- Do not leave both Vercel Git auto deploy and GitHub Actions deploys enabled on the same project — there must be exactly one active deploy trigger path
- Do not choose the Vercel cutover mode (vercel.json vs dashboard disconnect) on the user's behalf — always let the user decide
- If the repo already has a deploy workflow, update it instead of adding a second one
- The workflow should follow the repository's existing CI conventions
- If the team already has a standard action or bootstrap script for `postagent`, reuse it instead of inventing a new installer

---

## Workflow

### Step 1: Collect Vercel project info locally

Use postagent to discover and call Vercel's teams and projects API.

Goal: collect all **static parameters** — values that do not change between deployments:
- `teamId`
- `projectId`
- `projectName`
- GitHub org / repo name

These values will be injected into GitHub secrets or workflow environment variables in a later step, so the CI workflow can reference them directly at runtime.

If the user doesn't have a Vercel project yet, use postagent to create one.

### Step 2: Check existing Git integration

Before adding GitHub Actions, check whether the project already has Vercel's built-in GitHub connector. If it does, both systems will trigger duplicate deployments.

Use postagent to inspect the project detail. Key fields:

- `link` — if populated, the project is Git-connected
- `connectBuildsEnabled` — if `true`, Git pushes trigger Vercel builds

**If Git-connected:** warn the user that duplicate deployments will occur, and let them choose how to handle it:

- **Option A (recommended): Disconnect Git integration entirely** — use postagent to remove the project's Git link via the Vercel projects API, or do it manually in the Vercel dashboard: Project > Settings > Git > Disconnect.
- **Option B: Keep Git connected, disable Git-triggered deployments** — add `"git": { "deploymentEnabled": false }` to the project's `vercel.json`. This preserves the Git link but stops Vercel from auto-deploying on push.

Present both options and let the user decide. Do not make the choice for them.

### Step 3: Inject collected info into GitHub

Store the static parameters from Step 1 in the GitHub repo so the workflow can reference them at runtime:

- **Sensitive values** (e.g. `VERCEL_TOKEN`) — store as GitHub Actions **secrets**
- **Non-sensitive values** (e.g. `teamId`, `projectId`, `projectName`) — store as secrets or hardcode in the workflow `env` block, depending on user preference

Use postagent to discover GitHub's secrets API and set the secrets programmatically. Note that GitHub's secret API requires encrypting values with the repo's public key (libsodium sealed box) — if this is too complex for the current environment, fall back to telling the user to add secrets manually in the GitHub UI.

### Step 4: Create the GitHub Actions workflow file

Write a `.github/workflows/vercel-deploy.yml` in the user's repo. If a deploy workflow already exists, update it rather than creating a second one.

The workflow has two phases: **set up postagent** and **deploy**.

**Phase 1 — Install and authenticate postagent in CI:**

- Install postagent in the runner (reuse the team's existing bootstrap method if one exists)
- Authenticate postagent using the injected secret via the `--token` flag

After this step, all `postagent send` commands in the workflow can use `$POSTAGENT.VERCEL.API_KEY` just like locally.

**Phase 2 — Deploy:**

Use postagent to create the deployment. Vercel handles the build asynchronously — a simple deployment creation call is sufficient. Status polling is optional and can be added later if the team wants the workflow to wait for the build result.

The deployment request requires two categories of parameters:
- **Static parameters** (collected in Step 1, injected in Step 3) — `teamId`, `projectId`, `projectName`, etc. Reference these from secrets or env vars.
- **Dynamic parameters** (change with each push/PR) — git ref, sha, commit message, actor. Read these from GitHub Actions context variables (e.g. `${{ github.sha }}`).

Use postagent to consult the Vercel deployment API documentation, understand which parameters are required, and assemble the request from static and dynamic values.

**Pitfall — `gitSource` format:** The Vercel API docs mark `gitSource.type` as `[const: vercel]`, but this is misleading. For GitHub repositories, the correct minimal `gitSource` is:
- `type` — must be `"github"` (not `"vercel"`)
- `repoId` — the numeric GitHub repository ID (not the repo name)
- `ref` — the commit SHA to deploy

Using `type: "vercel"` will result in a 500 error. The GitHub repo ID can be obtained via postagent through the GitHub repos API.

### Step 5: Push the workflow to GitHub

Commit and push the workflow file using standard git commands (`git add`, `git commit`, `git push`). Do not use postagent for git operations.

### Step 6: Verify

After pushing, the workflow triggers automatically. Verify both sides using postagent:

- Confirm that GitHub Actions triggered the intended Vercel deployment
- Confirm that the deployment reached `READY` state and produced the expected URL
- If there are multiple deployments, identify which one belongs to the GitHub Actions run
- If the deployment fails, inspect the failure details and check project env vars and team scope

---

## Acceptance Criteria

- The repo has exactly one deployment workflow in `.github/workflows/`
- The Vercel token was collected through a secure input path, not pasted in normal chat
- The GitHub Actions secret was created via postagent (or documented fallback to manual UI)
- The workflow authenticates postagent via `--token` flag using the injected secret
- The workflow uses postagent for deployment creation and status checks — no custom request scripts
- Vercel Git auto deploy is disabled (via `vercel.json`) or the Git integration is disconnected
- There is no double-deploy path

## Security rules

- Never hardcode tokens in workflow files or committed code — always use GitHub Actions secrets
- Use `postagent auth` for local operations — tokens stay in postagent's credential store
- In CI, authenticate postagent via `--token` flag with secrets injection — the token never appears in logs
- Prefer `preview` target for initial testing before deploying to `production`
