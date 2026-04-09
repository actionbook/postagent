#!/usr/bin/env node

"use strict";

const fs = require("fs");
const path = require("path");

const PLATFORM_PACKAGES = {
  "darwin-arm64": "postagent-darwin-arm64",
  "darwin-x64": "postagent-darwin-x64",
  "linux-x64": "postagent-linux-x64-gnu",
  "linux-arm64": "postagent-linux-arm64-gnu",
};

function resolvePackageDir(packageName) {
  try {
    const packageJsonPath = require.resolve(`${packageName}/package.json`);
    return path.dirname(packageJsonPath);
  } catch {
    const packageDir = path.join(__dirname, "..", "..", packageName);
    const packageJsonPath = path.join(packageDir, "package.json");
    if (fs.existsSync(packageJsonPath)) {
      return packageDir;
    }
    return null;
  }
}

function main() {
  const platformKey = `${process.platform}-${process.arch}`;
  const packageName = PLATFORM_PACKAGES[platformKey];
  if (!packageName) return;

  const packageDir = resolvePackageDir(packageName);
  if (!packageDir) return;

  const binaryPath = path.join(packageDir, "bin", "postagent-core");
  if (fs.existsSync(binaryPath)) {
    fs.chmodSync(binaryPath, 0o755);
  }
}

if (require.main === module) {
  main();
}
