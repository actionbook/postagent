import { Command } from "commander";
import { resolveTemplateVariables } from "../lib/token.js";

function collect(val: string, acc: string[]): string[] {
  acc.push(val);
  return acc;
}

function parseHeader(raw: string): Record<string, string> {
  const trimmed = raw.trim();
  if (trimmed.startsWith("{")) {
    try {
      return JSON.parse(trimmed) as Record<string, string>;
    } catch {
      // fallback to Key: Value format
    }
  }
  const colonIdx = trimmed.indexOf(":");
  if (colonIdx === -1) return {};
  const key = trimmed.slice(0, colonIdx).trim();
  const value = trimmed.slice(colonIdx + 1).trim();
  return { [key]: value };
}

export const requestCommand = new Command("request")
  .description("Send an HTTP request")
  .argument("<url>", "Request URL")
  .option("-X, --method <method>", "HTTP method")
  .option("-H, --header <header>", "Request header (repeatable)", collect, [])
  .option("-d, --data <body>", "Request body")
  .action(async (rawUrl: string, options: { method?: string; header: string[]; data?: string }) => {
    let url: string;
    let body: string | undefined;
    const mergedHeaders: Record<string, string> = {};

    // 1. Template variable substitution
    try {
      url = await resolveTemplateVariables(rawUrl);
      for (const raw of options.header) {
        const resolved = await resolveTemplateVariables(raw);
        Object.assign(mergedHeaders, parseHeader(resolved));
      }
      if (options.data) {
        body = await resolveTemplateVariables(options.data);
      }
    } catch (err) {
      console.error((err as Error).message);
      process.exitCode = 1;
      return;
    }

    // 2. Determine method
    const method = options.method?.toUpperCase() ?? (body ? "POST" : "GET");

    // 3. Send request
    let response: Response;
    try {
      response = await fetch(url, {
        method,
        headers: mergedHeaders,
        body,
        signal: AbortSignal.timeout(30_000),
      });
    } catch (err) {
      if (err instanceof TypeError) {
        console.error(`Invalid URL: ${url}`);
      } else {
        console.error((err as Error).message);
      }
      process.exitCode = 1;
      return;
    }

    // 4. Handle response
    const responseBody = await response.text();
    if (response.ok) {
      process.stdout.write(responseBody);
    } else {
      process.stderr.write(`HTTP ${response.status} ${response.statusText}\n`);
      process.stderr.write(responseBody);
      process.exitCode = 1;
    }
  });
