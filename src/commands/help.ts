import { Command } from "commander";
import { getProject, getResource, getAction } from "../loader/index.js";
import { getFormatter, type Format } from "../formatter/index.js";

export const helpCommand = new Command("help")
  .description("Get project/resource/action details (progressive discovery)")
  .argument("[project]", "Project name")
  .argument("[resource]", "Resource name")
  .argument("[action]", "Action name")
  .action((projectName?: string, resourceName?: string, actionName?: string, _opts?: unknown, cmd?: Command) => {
    // No arguments: show top-level help (equivalent to postagent -h)
    if (!projectName) {
      cmd?.parent?.help();
      return;
    }

    const format = (cmd ?? helpCommand).optsWithGlobals().format as Format;
    const formatter = getFormatter(format);

    const project = getProject(projectName);
    if (!project) {
      console.error(`Project "${projectName}" not found.`);
      process.exitCode = 1;
      return;
    }

    // Level 1: list resources
    if (!resourceName) {
      console.log(formatter.formatProject(project));
      return;
    }

    const resource = getResource(projectName, resourceName);
    if (!resource) {
      console.error(`Resource "${resourceName}" not found in project "${projectName}".`);
      console.error(`Available resources: ${project.resources.map((r) => r.name).join(", ")}`);
      process.exitCode = 1;
      return;
    }

    // Level 2: list actions
    if (!actionName) {
      console.log(formatter.formatResource(project, resource));
      return;
    }

    const action = getAction(projectName, resourceName, actionName);
    if (!action) {
      console.error(`Action "${actionName}" not found in "${projectName} > ${resourceName}".`);
      console.error(`Available actions: ${resource.actions.map((a) => a.name).join(", ")}`);
      process.exitCode = 1;
      return;
    }

    // Level 3: show action detail
    console.log(formatter.formatAction(project, resource, action));
  });
