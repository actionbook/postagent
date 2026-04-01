import type { ProjectInfo, ResourceInfo, ActionInfo, ParameterInfo, RequestBodyInfo, ResponseInfo } from "./index.js";

// ── Internal types ──────────────────────────────────────────

interface ParsedField {
  name: string;
  description: string;
  args: { name: string; type: string; required: boolean; defaultValue?: string }[];
  returnType: string;
}

interface ParsedType {
  kind: "type" | "input" | "enum" | "scalar";
  name: string;
  description: string;
  body: string;
}

// ── Header description (# comments at top of file) ─────────

function extractHeaderDescription(source: string): string {
  const lines: string[] = [];
  for (const line of source.split("\n")) {
    const t = line.trim();
    if (t.startsWith("#")) {
      lines.push(t.replace(/^#\s?/, ""));
    } else if (t === "" && lines.length > 0) {
      lines.push("");
    } else if (t !== "") {
      break;
    }
  }
  return lines.join("\n").trim();
}

// ── Extract top-level type/input/enum/scalar definitions ────

function extractTopLevelDefs(source: string): ParsedType[] {
  const defs: ParsedType[] = [];
  const len = source.length;
  let i = 0;

  while (i < len) {
    // skip whitespace
    while (i < len && /\s/.test(source[i])) i++;
    if (i >= len) break;

    // capture optional docstring before a definition
    let description = "";
    if (source.startsWith('"""', i)) {
      const end = source.indexOf('"""', i + 3);
      if (end !== -1) {
        description = source.substring(i + 3, end).trim();
        i = end + 3;
        while (i < len && /\s/.test(source[i])) i++;
      }
    }

    // skip line comments
    if (i < len && source[i] === "#") {
      while (i < len && source[i] !== "\n") i++;
      i++;
      continue;
    }

    // match type|input|enum|scalar keyword
    const rest = source.substring(i);
    const kwMatch = rest.match(/^(type|input|enum|scalar)\s+(\w+)/);
    if (!kwMatch) {
      // skip to next line
      while (i < len && source[i] !== "\n") i++;
      i++;
      continue;
    }

    const kind = kwMatch[1] as ParsedType["kind"];
    const name = kwMatch[2];
    i += kwMatch[0].length;

    if (kind === "scalar") {
      defs.push({ kind, name, description, body: "" });
      continue;
    }

    // find opening brace
    while (i < len && source[i] !== "{") i++;
    if (i >= len) break;

    // find matching closing brace
    let depth = 1;
    const bodyStart = i + 1;
    i++;
    while (i < len && depth > 0) {
      if (source[i] === "{") depth++;
      if (source[i] === "}") depth--;
      i++;
    }
    const body = source.substring(bodyStart, i - 1);
    defs.push({ kind, name, description, body });
  }

  return defs;
}

// ── Collapse multi-line args into single lines ──────────────

function normalizeFieldBody(body: string): string {
  let result = "";
  let parenDepth = 0;
  for (const ch of body) {
    if (ch === "(") parenDepth++;
    if (ch === ")") parenDepth--;
    result += parenDepth > 0 && ch === "\n" ? " " : ch;
  }
  return result;
}

// ── Parse fields inside a type / input body ─────────────────

function parseFields(body: string): ParsedField[] {
  const normalized = normalizeFieldBody(body);
  const lines = normalized.split("\n");
  const fields: ParsedField[] = [];

  let currentDesc = "";
  let inDocstring = false;
  let descBuf: string[] = [];

  for (const rawLine of lines) {
    const line = rawLine.trim();

    // multi-line docstring handling
    if (inDocstring) {
      if (line.endsWith('"""')) {
        descBuf.push(line.slice(0, -3));
        currentDesc = descBuf.join("\n").trim();
        inDocstring = false;
        descBuf = [];
      } else {
        descBuf.push(line);
      }
      continue;
    }

    if (line.startsWith('"""')) {
      if (line.endsWith('"""') && line.length >= 6) {
        // single-line docstring
        currentDesc = line.slice(3, -3).trim();
      } else {
        // multi-line docstring start
        inDocstring = true;
        descBuf = [line.slice(3)];
      }
      continue;
    }

    // skip empty lines & comments
    if (line === "" || line.startsWith("#")) {
      if (line === "") currentDesc = "";
      continue;
    }

    // field pattern: name(args): ReturnType
    const m = line.match(/^(\w+)\s*(\(.*\))?\s*:\s*(.+)$/);
    if (!m) continue;

    const fieldName = m[1];
    const argsStr = m[2] ?? "";
    const returnType = m[3].trim();

    // parse args
    const args: ParsedField["args"] = [];
    if (argsStr.length > 2) {
      const argsBody = argsStr.slice(1, -1).replace(/"""[\s\S]*?"""/g, "");
      const argRe = /(\w+)\s*:\s*(\[?\w+!?\]?!?)(?:\s*=\s*(\S+))?/g;
      let am;
      while ((am = argRe.exec(argsBody)) !== null) {
        args.push({
          name: am[1],
          type: am[2],
          required: am[2].endsWith("!"),
          defaultValue: am[3],
        });
      }
    }

    fields.push({ name: fieldName, description: currentDesc, args, returnType });
    currentDesc = "";
  }

  return fields;
}

// ── Resolve a GraphQL type into a JSON-schema-like object ───

function resolveTypeAsSchema(typeName: string, typeMap: Map<string, ParsedType>): unknown {
  const base = typeName.replace(/[!\[\]]/g, "").trim();
  const def = typeMap.get(base);
  if (!def) return { type: base };

  if (def.kind === "scalar") {
    const s: Record<string, unknown> = { type: base };
    if (def.description) s.description = def.description;
    return s;
  }

  if (def.kind === "enum") {
    const values = def.body
      .split("\n")
      .map((l) => l.trim())
      .filter((l) => l && !l.startsWith("#") && !l.startsWith('"""'));
    return { type: "enum", enum: values };
  }

  // type / input
  const fields = parseFields(def.body);
  const properties: Record<string, unknown> = {};
  const required: string[] = [];

  for (const f of fields) {
    const prop: Record<string, unknown> = { type: f.returnType };
    if (f.description) prop.description = f.description;
    properties[f.name] = prop;
    if (f.returnType.endsWith("!")) required.push(f.name);
  }

  const schema: Record<string, unknown> = { type: "object", properties };
  if (required.length > 0) schema.required = required;
  return schema;
}

// ── Helpers ─────────────────────────────────────────────────

function camelToSnake(s: string): string {
  return s.replace(/([a-z])([A-Z])/g, "$1_$2").toLowerCase();
}

// ── Public API ──────────────────────────────────────────────

export function parseGraphQL(source: string, fileName: string): ProjectInfo {
  const description = extractHeaderDescription(source);
  const defs = extractTopLevelDefs(source);

  const typeMap = new Map<string, ParsedType>();
  for (const def of defs) typeMap.set(def.name, def);

  const resources: ResourceInfo[] = [];

  for (const [resourceName, typeName, method] of [
    ["queries", "Query", "QUERY"],
    ["mutations", "Mutation", "MUTATION"],
  ] as const) {
    const def = typeMap.get(typeName);
    if (!def || def.kind === "scalar") continue;

    const fields = parseFields(def.body);
    const actions: ActionInfo[] = fields.map((field): ActionInfo => {
      const parameters: ParameterInfo[] = field.args.map((arg) => ({
        name: arg.name,
        in: "argument",
        type: arg.type,
        required: arg.required,
        description: arg.defaultValue ? `default: ${arg.defaultValue}` : "",
      }));

      // resolve input-type args into requestBody
      let requestBody: RequestBodyInfo | undefined;
      const inputArg = field.args.find((a) => {
        const base = a.type.replace(/[!\[\]]/g, "");
        return typeMap.get(base)?.kind === "input";
      });
      if (inputArg) {
        const base = inputArg.type.replace(/[!\[\]]/g, "");
        requestBody = {
          contentType: "application/json",
          schema: resolveTypeAsSchema(base, typeMap),
        };
      }

      // resolve return type
      const responses: ResponseInfo[] = [
        {
          status: "success",
          description: field.returnType,
          schema: resolveTypeAsSchema(field.returnType, typeMap),
        },
      ];

      return {
        name: camelToSnake(field.name),
        method,
        path: field.name,
        summary: field.description,
        description: field.description,
        parameters,
        requestBody,
        responses,
      };
    });

    if (actions.length > 0) {
      resources.push({ name: resourceName, actions });
    }
  }

  return { name: fileName, description, version: "", resources };
}
