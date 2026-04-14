import path from "node:path";
import { existsSync } from "node:fs";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";

const PLATFORM_PACKAGES: Record<string, string> = {
  "darwin-arm64": "postagent-darwin-arm64",
  "darwin-x64": "postagent-darwin-x64",
  "linux-x64": "postagent-linux-x64-gnu",
  "linux-arm64": "postagent-linux-arm64-gnu",
};

const require = createRequire(import.meta.url);

function resolvePackageDir(packageName: string): string | null {
  try {
    const packageJsonPath = require.resolve(`${packageName}/package.json`);
    return path.dirname(packageJsonPath);
  } catch {
    // Fallback for workspace or non-hoisted layouts
    const __dirname = path.dirname(fileURLToPath(import.meta.url));
    const packageDir = path.join(__dirname, "..", "..", packageName);
    const packageJsonPath = path.join(packageDir, "package.json");
    if (existsSync(packageJsonPath)) {
      return packageDir;
    }
    return null;
  }
}

export function resolveBinary(): string {
  // Dev mode: use local cargo build output
  if (process.env.POSTAGENT_DEV) {
    const __dirname = path.dirname(fileURLToPath(import.meta.url));
    const coreDir = path.resolve(__dirname, "..", "..", "postagent-core");
    const devBinary = path.join(coreDir, "target", "debug", "postagent-core");
    if (existsSync(devBinary)) {
      return devBinary;
    }
  }

  const platformKey = `${process.platform}-${process.arch}`;
  const packageName = PLATFORM_PACKAGES[platformKey];

  if (!packageName) {
    console.error(`Error: Unsupported platform: ${platformKey}`);
    console.error(`Supported: ${Object.keys(PLATFORM_PACKAGES).join(", ")}`);
    process.exit(1);
  }

  const binaryName = "postagent-core";
  const packageDir = resolvePackageDir(packageName);

  if (!packageDir) {
    console.error(`Error: Missing native package for ${platformKey}`);
    console.error(`Expected package: ${packageName}`);
    console.error("");
    console.error("Try reinstalling: npm install postagent");
    process.exit(1);
  }

  const binaryPath = path.join(packageDir, "bin", binaryName);

  if (!existsSync(binaryPath)) {
    console.error(`Error: No binary found in ${packageName}`);
    console.error(`Expected: ${binaryPath}`);
    process.exit(1);
  }

  return binaryPath;
}
