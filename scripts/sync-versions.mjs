#!/usr/bin/env node
// Sync the version of packages/postagent-core/Cargo.toml to match the
// version carried by the JS packages after `changeset version` runs.
//
// Changesets only understands package.json. The Rust crate is kept in lockstep
// manually here so that `postagent-core`'s crate version never diverges from
// the npm packages it backs.

import { readFileSync, writeFileSync } from "node:fs";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";

const __dirname = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(__dirname, "..");
const coreDir = resolve(repoRoot, "packages/postagent-core");

const sourcePkg = JSON.parse(
  readFileSync(resolve(repoRoot, "packages/postagent/package.json"), "utf8"),
);
const version = sourcePkg.version;
if (!version) {
  console.error("[sync-versions] cannot read postagent version");
  process.exit(1);
}

const cargoPath = resolve(coreDir, "Cargo.toml");
const cargo = readFileSync(cargoPath, "utf8");

// Match the version line scoped to the [package] section only. [^[] stops at
// the next TOML section header so we never clobber a dependency version.
const packageVersionRe = /(\[package\][^[]*?\nversion\s*=\s*)"[^"]+"/;
if (!packageVersionRe.test(cargo)) {
  console.error(
    "[sync-versions] could not find [package] version line in Cargo.toml",
  );
  process.exit(1);
}

const updated = cargo.replace(packageVersionRe, `$1"${version}"`);
const cargoChanged = cargo !== updated;

if (cargoChanged) {
  writeFileSync(cargoPath, updated);
  console.log(`[sync-versions] Cargo.toml -> ${version}`);
} else {
  console.log(`[sync-versions] Cargo.toml already at ${version}`);
}

// Keep Cargo.lock in sync so `cargo build --locked` (and any lockfile-pinned
// CI check) does not fail after a version bump. `cargo update -p <pkg>` works
// for workspace members and touches only the relevant entries.
const lockResult = spawnSync(
  "cargo",
  ["update", "-p", "postagent-core"],
  { cwd: coreDir, stdio: "inherit" },
);
if (lockResult.error) {
  console.error(
    `[sync-versions] failed to run cargo update: ${lockResult.error.message}`,
  );
  process.exit(1);
}
if (lockResult.status !== 0) {
  console.error(
    `[sync-versions] cargo update exited with status ${lockResult.status}`,
  );
  process.exit(lockResult.status ?? 1);
}
console.log(`[sync-versions] Cargo.lock -> ${version}`);
