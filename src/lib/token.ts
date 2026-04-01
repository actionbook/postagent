import { mkdir, writeFile, readFile } from "node:fs/promises";
import { join } from "node:path";
import { homedir } from "node:os";

function getTokenDir(project: string): string {
  return join(homedir(), ".postagent", project);
}

export async function saveToken(project: string, token: string): Promise<void> {
  const dir = getTokenDir(project.toLowerCase());
  await mkdir(dir, { recursive: true });
  const file = join(dir, "auth");
  await writeFile(file, token, { encoding: "utf-8", mode: 0o600 });
}

export async function loadToken(project: string): Promise<string | undefined> {
  const file = join(getTokenDir(project.toLowerCase()), "auth");
  try {
    return (await readFile(file, "utf-8")).trim();
  } catch {
    return undefined;
  }
}

const TEMPLATE_RE = /\$POSTAGENT\.([A-Za-z0-9_]+)\.API_KEY/g;

export async function resolveTemplateVariables(input: string): Promise<string> {
  const matches = [...input.matchAll(TEMPLATE_RE)];
  if (matches.length === 0) return input;

  let result = input;
  for (const match of matches) {
    const project = match[1].toLowerCase();
    const token = await loadToken(project);
    if (!token) {
      throw new Error(
        `Auth not found for "${project}". Run: postagent auth ${project}`
      );
    }
    result = result.replace(match[0], token);
  }
  return result;
}
