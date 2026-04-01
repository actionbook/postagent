import { Command } from "commander";
import { createInterface } from "node:readline/promises";
import { saveToken } from "../lib/token.js";

export const authCommand = new Command("auth")
  .description("Save API key for a project")
  .argument("<project>", "Project name")
  .action(async (project: string) => {
    const rl = createInterface({
      input: process.stdin,
      output: process.stdout,
    });

    try {
      const token = (await rl.question(`Enter API key for "${project}": `)).trim();
      rl.close();

      if (!token) {
        console.error("Error: API key cannot be empty.");
        process.exitCode = 1;
        return;
      }

      await saveToken(project, token);
      console.log(`Auth saved for "${project.toLowerCase()}".`);
    } catch (err) {
      rl.close();
      if ((err as NodeJS.ErrnoException).code === "EACCES") {
        console.error("Error: Permission denied. Check directory permissions.");
      } else {
        console.error(`Error: ${(err as Error).message}`);
      }
      process.exitCode = 1;
    }
  });
