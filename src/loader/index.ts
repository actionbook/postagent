import { readFileSync, readdirSync } from "node:fs";
import { join, basename, extname } from "node:path";
import { parse as parseYaml } from "yaml";
import { parseGraphQL } from "./graphql.js";

export interface ProjectInfo {
  name: string;
  description: string;
  version: string;
  resources: ResourceInfo[];
}

export interface ResourceInfo {
  name: string;
  actions: ActionInfo[];
}

export interface ActionInfo {
  name: string;
  method: string;
  path: string;
  summary: string;
  description: string;
  parameters: ParameterInfo[];
  requestBody?: RequestBodyInfo;
  responses: ResponseInfo[];
}

export interface ParameterInfo {
  name: string;
  in: string;
  type: string;
  required: boolean;
  description: string;
}

export interface RequestBodyInfo {
  contentType: string;
  schema: unknown;
}

export interface ResponseInfo {
  status: string;
  description: string;
  schema?: unknown;
}

function resolveRef(spec: Record<string, unknown>, ref: string): unknown {
  const parts = ref.replace("#/", "").split("/");
  let current: unknown = spec;
  for (const part of parts) {
    if (current && typeof current === "object" && part in current) {
      current = (current as Record<string, unknown>)[part];
    } else {
      return undefined;
    }
  }
  return current;
}

function resolveSchema(spec: Record<string, unknown>, schema: unknown): unknown {
  if (!schema || typeof schema !== "object") return schema;
  const obj = schema as Record<string, unknown>;
  if ("$ref" in obj && typeof obj["$ref"] === "string") {
    return resolveSchema(spec, resolveRef(spec, obj["$ref"]));
  }
  return schema;
}

function extractType(spec: Record<string, unknown>, schema: unknown): string {
  if (!schema || typeof schema !== "object") return "unknown";
  const resolved = resolveSchema(spec, schema) as Record<string, unknown>;
  if (!resolved) return "unknown";
  if (resolved.type) return resolved.type as string;
  if (resolved.oneOf) return "oneOf";
  return "object";
}

function operationIdToActionName(operationId: string): string {
  return operationId
    .replace(/([a-z])([A-Z])/g, "$1_$2")
    .toLowerCase();
}

function parseOperation(
  spec: Record<string, unknown>,
  method: string,
  path: string,
  operation: Record<string, unknown>,
): ActionInfo {
  const opId = (operation.operationId as string) || `${method}_${path}`;
  const params: ParameterInfo[] = [];

  const rawParams = operation.parameters as Array<Record<string, unknown>> | undefined;
  if (rawParams) {
    for (const p of rawParams) {
      const resolved = ("$ref" in p
        ? resolveRef(spec, p["$ref"] as string)
        : p) as Record<string, unknown> | undefined;
      if (!resolved || resolved.name === "Notion-Version") continue;
      params.push({
        name: resolved.name as string,
        in: resolved.in as string,
        type: extractType(spec, resolved.schema),
        required: (resolved.required as boolean) ?? false,
        description: (resolved.description as string) ?? "",
      });
    }
  }

  let requestBody: RequestBodyInfo | undefined;
  const body = operation.requestBody as Record<string, unknown> | undefined;
  if (body) {
    const content = body.content as Record<string, Record<string, unknown>> | undefined;
    if (content) {
      const [contentType, mediaObj] = Object.entries(content)[0];
      requestBody = {
        contentType,
        schema: resolveSchema(spec, mediaObj.schema),
      };
    }
  }

  const responses: ResponseInfo[] = [];
  const rawResponses = operation.responses as Record<string, Record<string, unknown>> | undefined;
  if (rawResponses) {
    for (const [status, resp] of Object.entries(rawResponses)) {
      const respContent = resp.content as Record<string, Record<string, unknown>> | undefined;
      let schema: unknown;
      if (respContent) {
        const [, mediaObj] = Object.entries(respContent)[0];
        schema = resolveSchema(spec, mediaObj.schema);
      }
      responses.push({
        status,
        description: (resp.description as string) ?? "",
        schema,
      });
    }
  }

  return {
    name: operationIdToActionName(opId),
    method: method.toUpperCase(),
    path,
    summary: (operation.summary as string) ?? "",
    description: (operation.description as string) ?? "",
    parameters: params,
    requestBody,
    responses,
  };
}

function parseSpec(spec: Record<string, unknown>): ProjectInfo {
  const info = spec.info as Record<string, unknown>;
  const paths = spec.paths as Record<string, Record<string, unknown>>;

  const tagMap = new Map<string, ActionInfo[]>();

  for (const [path, methods] of Object.entries(paths)) {
    for (const [method, operation] of Object.entries(methods)) {
      if (typeof operation !== "object" || !operation) continue;
      const op = operation as Record<string, unknown>;
      const tags = (op.tags as string[]) ?? ["default"];
      const action = parseOperation(spec, method, path, op);
      for (const tag of tags) {
        const normalized = tag.toLowerCase().replace(/\s+/g, "_");
        if (!tagMap.has(normalized)) tagMap.set(normalized, []);
        tagMap.get(normalized)!.push(action);
      }
    }
  }

  const resources: ResourceInfo[] = [];
  for (const [name, actions] of tagMap) {
    resources.push({ name, actions });
  }

  return {
    name: (info.title as string) ?? "unknown",
    description: (info.description as string) ?? "",
    version: (info.version as string) ?? "",
    resources,
  };
}

const projectCache = new Map<string, ProjectInfo>();

function getExamplesDir(): string {
  // Both dev (src/loader/) and build (dist/) need to resolve to <root>/examples
  // import.meta.dirname is src/loader in dev, dist in build
  const dir = import.meta.dirname;
  const candidate = join(dir, "../examples");
  try {
    readdirSync(candidate);
    return candidate;
  } catch {
    return join(dir, "../../examples");
  }
}

export function loadAllProjects(): Map<string, ProjectInfo> {
  if (projectCache.size > 0) return projectCache;

  const dir = getExamplesDir();
  let files: string[];
  try {
    files = readdirSync(dir).filter(
      (f) => f.endsWith(".yaml") || f.endsWith(".yml") || f.endsWith(".graphql"),
    );
  } catch {
    return projectCache;
  }

  for (const file of files) {
    const raw = readFileSync(join(dir, file), "utf-8");
    const ext = extname(file);
    const key = basename(file, ext);
    let project: ProjectInfo;

    if (ext === ".graphql") {
      project = parseGraphQL(raw, key);
    } else {
      const spec = parseYaml(raw) as Record<string, unknown>;
      project = parseSpec(spec);
      project.name = key;
    }

    projectCache.set(key, project);
  }

  return projectCache;
}

export function getProject(name: string): ProjectInfo | undefined {
  const projects = loadAllProjects();
  return projects.get(name);
}

export function getResource(projectName: string, resourceName: string): ResourceInfo | undefined {
  const project = getProject(projectName);
  if (!project) return undefined;
  return project.resources.find((r) => r.name === resourceName);
}

export function getAction(
  projectName: string,
  resourceName: string,
  actionName: string,
): ActionInfo | undefined {
  const resource = getResource(projectName, resourceName);
  if (!resource) return undefined;
  return resource.actions.find((a) => a.name === actionName);
}
