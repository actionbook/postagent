import { Command } from "commander";
import { saveToken } from "../lib/token.js";

function readSecret(prompt: string): Promise<string> {
  return new Promise((resolve, reject) => {
    process.stdout.write(prompt);

    if (!process.stdin.isTTY) {
      // Non-TTY: read one line from stdin
      let buf = "";
      process.stdin.setEncoding("utf-8");
      process.stdin.on("data", (chunk: string) => {
        buf += chunk;
        const nl = buf.indexOf("\n");
        if (nl !== -1) {
          process.stdin.pause();
          process.stdin.removeAllListeners("data");
          resolve(buf.slice(0, nl).trim());
        }
      });
      process.stdin.on("error", reject);
      process.stdin.resume();
      return;
    }

    // TTY: read with hidden input
    const stdin = process.stdin;
    stdin.setRawMode(true);
    stdin.setEncoding("utf-8");
    stdin.resume();

    let input = "";
    let stars = 0;
    const finish = (value: string) => {
      stdin.setRawMode(false);
      stdin.pause();
      stdin.removeListener("data", onData);
      process.stdout.write("\n");
      resolve(value);
    };
    const abort = () => {
      stdin.setRawMode(false);
      stdin.pause();
      stdin.removeListener("data", onData);
      process.stdout.write("\n");
      reject(new Error("Aborted."));
    };
    const onData = (data: string) => {
      for (const ch of data) {
        if (ch === "\r" || ch === "\n") {
          return finish(input.trim());
        } else if (ch === "\u0003") {
          return abort();
        } else if (ch === "\u007f" || ch === "\b") {
          if (input.length > 0 && stars > 0) {
            input = input.slice(0, -1);
            stars--;
            process.stdout.write("\b \b");
          }
        } else if (ch.charCodeAt(0) >= 32) {
          input += ch;
          stars++;
          process.stdout.write("*");
        }
      }
    };
    stdin.on("data", onData);
  });
}

export const authCommand = new Command("auth")
  .description("Save API key for a project")
  .argument("<project>", "Project name")
  .action(async (project: string) => {
    try {
      const token = await readSecret(`Enter API key for "${project}": `);

      if (!token) {
        console.error("Error: API key cannot be empty.");
        process.exitCode = 1;
        return;
      }

      await saveToken(project, token);
      console.log(`Auth saved for "${project.toLowerCase()}".`);
    } catch (err) {
      if ((err as NodeJS.ErrnoException).code === "EACCES") {
        console.error("Error: Permission denied. Check directory permissions.");
      } else {
        console.error(`Error: ${(err as Error).message}`);
      }
      process.exitCode = 1;
    }
  });
