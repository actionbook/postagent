use reqwest::blocking::Client;
use serde::Deserialize;

use crate::api_response;
use crate::config;
use crate::formatter;

// === Site overview data structures ===

#[derive(Deserialize, Clone)]
struct Authentication {
    #[serde(rename = "in")]
    #[allow(dead_code)]
    location: Option<String>,
    name: Option<String>,
    #[serde(rename = "type")]
    auth_type: String,
    description: Option<String>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum SiteAction {
    Simple(String),
    Detailed { name: String, summary: Option<String> },
}

impl SiteAction {
    fn name(&self) -> &str {
        match self {
            SiteAction::Simple(s) => s,
            SiteAction::Detailed { name, .. } => name,
        }
    }

    fn summary(&self) -> Option<&str> {
        match self {
            SiteAction::Simple(_) => None,
            SiteAction::Detailed { summary, .. } => summary.as_deref(),
        }
    }
}

#[derive(Deserialize)]
struct SiteGroup {
    name: String,
    #[allow(dead_code)]
    base_url: Option<String>,
    actions: Vec<SiteAction>,
}

#[derive(Deserialize)]
struct SiteOverview {
    name: String,
    description: String,
    authentication: Option<Authentication>,
    groups: Vec<SiteGroup>,
}

// === Group overview data structures ===

#[derive(Deserialize)]
struct GroupAction {
    name: String,
    method: String,
    path: String,
    #[allow(dead_code)]
    base_url: Option<String>,
    summary: String,
}

#[derive(Deserialize)]
struct GroupOverview {
    group: String,
    #[allow(dead_code)]
    base_url: Option<String>,
    actions: Vec<GroupAction>,
}

// === Action detail data structures ===

#[derive(Deserialize)]
struct Parameter {
    name: String,
    #[serde(rename = "in")]
    #[allow(dead_code)]
    location: String,
    #[serde(rename = "type")]
    param_type: String,
    required: bool,
    description: String,
}

#[derive(Deserialize)]
struct RequestBody {
    #[serde(rename = "contentType")]
    #[allow(dead_code)]
    content_type: Option<String>,
    schema: Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct ResponseInfo {
    status: String,
    description: String,
    schema: Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct ActionDetail {
    site: String,
    group: String,
    action: String,
    method: String,
    path: String,
    base_url: Option<String>,
    description: String,
    parameters: Vec<Parameter>,
    #[serde(rename = "requestBody")]
    request_body: Option<RequestBody>,
    responses: Vec<ResponseInfo>,
    authentication: Option<Authentication>,
}

pub fn run(
    site: Option<&str>,
    group: Option<&str>,
    action: Option<&str>,
    json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let site = match site {
        Some(p) => p,
        None => {
            return Err("show_help".into());
        }
    };

    // Build the API URL
    let mut params = vec![("site", site.to_string())];
    if let Some(g) = group {
        params.push(("group", g.to_string()));
    }
    if let Some(a) = action {
        params.push(("action", a.to_string()));
    }

    let query_string: String = params
        .iter()
        .map(|(k, v)| format!("{}={}", k, urlencoding(v)))
        .collect::<Vec<_>>()
        .join("&");

    let client = Client::new();
    let url = format!("{}/api/manual?{}", config::api_base(), query_string);

    let response = match client.get(&url).send() {
        Ok(resp) => resp,
        Err(_) => {
            eprintln!("Failed to connect to postagent server.");
            std::process::exit(1);
        }
    };

    if !response.status().is_success() {
        let body: serde_json::Value = response.json()?;
        api_response::print_api_error(&body);
        std::process::exit(1);
    }

    let body_text = response.text()?;
    let data = api_response::unwrap_data(serde_json::from_str(&body_text)?);

    if json {
        println!("{}", serde_json::to_string_pretty(&data)?);
        return Ok(());
    }

    if group.is_none() {
        let l1: SiteOverview = serde_json::from_value(data)?;
        println!("{}", format_site_overview(&l1));
    } else if action.is_none() {
        let l2: GroupOverview = serde_json::from_value(data)?;
        println!("{}", format_group_overview(&l2, site));
    } else {
        let l3: ActionDetail = serde_json::from_value(data)?;
        println!("{}", format_action_detail(&l3));
    }

    Ok(())
}

// === L1: Site Overview ===

struct SiteMeta {
    base_url: Option<String>,
    auth: Option<String>,
    header: Option<String>,
    source: Option<String>,
    endpoint: Option<String>,
    api_type: Option<String>,
}

fn extract_site_meta(data: &SiteOverview) -> SiteMeta {
    let description = &data.description;
    let is_graphql = description.to_lowercase().contains("graphql");

    // Base URL from description or first group
    let base_url_re = regex::Regex::new(r"`(https?://[^`]+)`").ok();
    let base_url = base_url_re
        .and_then(|re| re.captures(description).map(|c| c[1].to_string()))
        .or_else(|| {
            data.groups
                .iter()
                .find_map(|group| group.base_url.as_ref().cloned())
        });

    // Auth from authentication struct
    let auth = data.authentication.as_ref().map(|a| format_auth_from_struct(a, &data.name));

    // Version header from description
    let header = {
        let re = regex::Regex::new(r"`([A-Z][a-zA-Z]+-Version)` header \(latest: `([^`]+)`\)").ok();
        re.and_then(|re| {
            re.captures(description).map(|caps| {
                format!("{}: {}", &caps[1], &caps[2])
            })
        })
    };

    // Source from description
    let source = {
        let re = regex::Regex::new(r"(?:developers?\.)[a-z0-9.-]+\.[a-z]+").ok();
        re.and_then(|re| re.find(description).map(|m| m.as_str().to_string()))
            .or_else(|| {
                let re = regex::Regex::new(r"([a-z]+\.dev(?:/[^\s`\)]*[a-z])?)").ok()?;
                re.find(description).map(|m| m.as_str().trim_end_matches('.').to_string())
            })
    };

    if is_graphql {
        let endpoint = base_url.as_ref().map(|u| format!("POST {}", u));
        let gql_auth = data.authentication.as_ref().map(|a| format_auth_from_struct(a, &data.name));

        SiteMeta {
            base_url: None,
            auth: gql_auth.or(auth),
            header: None,
            source,
            endpoint,
            api_type: Some("GraphQL".into()),
        }
    } else {
        SiteMeta {
            base_url,
            auth,
            header,
            source,
            endpoint: None,
            api_type: None,
        }
    }
}

fn format_auth_from_struct(auth: &Authentication, site: &str) -> String {
    let name = auth.name.as_deref().unwrap_or("Authorization");
    let key_var = format!("$POSTAGENT.{}.API_KEY", site.to_uppercase());
    if auth.auth_type == "bearer" {
        format!("{}: Bearer {}", name, key_var)
    } else {
        format!("{}: {}", name, key_var)
    }
}

fn format_site_overview(data: &SiteOverview) -> String {
    let meta = extract_site_meta(data);

    let mut output = String::new();
    output.push_str(&format!("  === {}\n\n", data.name));

    // Render metadata block
    if meta.api_type.as_deref() == Some("GraphQL") {
        if let Some(ref endpoint) = meta.endpoint {
            output.push_str(&format!("  Endpoint:  {}\n", endpoint));
        }
        if let Some(ref auth) = meta.auth {
            output.push_str(&format!("  Auth:      {}\n", auth));
        }
        output.push_str("  Type:      GraphQL\n");
        if let Some(ref source) = meta.source {
            output.push_str(&format!("  Source:    {}\n", source));
        }
    } else {
        if let Some(ref base_url) = meta.base_url {
            output.push_str(&format!("  Base URL:  {}\n", base_url));
        }
        if let Some(ref auth) = meta.auth {
            output.push_str(&format!("  Auth:      {}\n", auth));
        }
        if let Some(ref header) = meta.header {
            output.push_str(&format!("  Header:    {}\n", header));
        }
        if let Some(ref source) = meta.source {
            output.push_str(&format!("  Source:    {}\n", source));
        }
    }

    let total_actions: usize = data.groups.iter().map(|g| g.actions.len()).sum();
    output.push_str(&format!(
        "\n  {} groups, {} actions total\n",
        data.groups.len(),
        total_actions
    ));

    output.push_str("\n  ---\n");

    // Render groups
    for group in &data.groups {
        let count = group.actions.len();
        output.push_str(&format!("\n  {}\n", group.name));

        let render_actions = if count <= 10 {
            &group.actions[..]
        } else {
            &group.actions[..5]
        };

        let has_summary = render_actions.iter().any(|a| a.summary().is_some());
        if has_summary {
            let mut table_rows: Vec<Vec<String>> = Vec::new();
            for action in render_actions {
                table_rows.push(vec![
                    action.name().to_string(),
                    action.summary().unwrap_or("").to_string(),
                ]);
            }
            let aligned = formatter::align_columns(&table_rows, 2);
            for line in &aligned {
                output.push_str(&format!("    {}\n", line));
            }
        } else {
            let max_name_width = render_actions.iter().map(|a| a.name().len()).max().unwrap_or(0);
            for action in render_actions {
                output.push_str(&format!("    {:<width$}\n", action.name(), width = max_name_width));
            }
        }

        if count > 10 {
            output.push_str(&format!("    ... {} more actions\n", count - 5));
        }
    }

    output.push_str(
        "\n  Run postagent manual <SITE> [GROUP] [ACTION] for full details.\n",
    );
    if let Some(first_group) = data.groups.first() {
        output.push_str(&format!(
            "  Example: postagent manual {} {}  # List actions in group\n",
            data.name, first_group.name
        ));
        if let Some(first_action) = first_group.actions.first() {
            output.push_str(&format!(
                "           postagent manual {} {} {}  # Get full details of action",
                data.name, first_group.name, first_action.name()
            ));
        }
    }

    output
}

// === L2: Group Actions ===

fn format_group_overview(data: &GroupOverview, site: &str) -> String {
    let total = data.actions.len();

    let mut output = String::new();

    output.push_str(&format!("  site:      {}\n", site));
    output.push_str(&format!("  group:     {}\n", data.group));
    output.push_str(&format!("  actions:   {}\n", total));

    output.push_str("\n  ---\n");

    let mut table_rows: Vec<Vec<String>> = Vec::new();
    for a in &data.actions {
        table_rows.push(vec![
            a.name.clone(),
            a.method.clone(),
            a.path.clone(),
            a.summary.clone(),
        ]);
    }

    let aligned = formatter::align_columns(&table_rows, 3);
    for line in &aligned {
        output.push_str(&format!("    {}\n", line));
    }

    output.push_str(
        "\n  Run postagent manual <SITE> [GROUP] [ACTION] for full details.\n",
    );
    if let Some(first_action) = data.actions.first() {
        output.push_str(&format!(
            "  Example: postagent manual {} {} {}  # Get full details of action",
            site, data.group, first_action.name
        ));
    }

    output
}

// === L3: Action Detail ===

fn action_to_title(action: &str) -> String {
    action
        .split('_')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(c) => {
                    let upper: String = c.to_uppercase().collect();
                    format!("{}{}", upper, chars.collect::<String>())
                }
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

// Full recursion budget for request bodies — LLMs need the complete shape to
// compose the JSON. Bounded to guard against pathological schemas.
const REQUEST_SCHEMA_DEPTH: usize = 5;
// Responses are consumed, not composed — top-level fields sketch the shape
// cheaply; deep structure belongs in --format json for anyone who needs it.
const RESPONSE_SCHEMA_DEPTH: usize = 0;
// Hard cap on total recursion frames (independent of `depth_budget`, which
// counts only object-nesting levels). Guards against pathological schemas
// such as `oneOf -> oneOf -> oneOf -> …` chains where each step doesn't
// consume the nesting budget but still grows the stack.
const MAX_WALK_DEPTH: usize = 32;

fn walk_schema(
    rows: &mut Vec<Vec<String>>,
    prefix: &str,
    schema: &serde_json::Value,
    depth_budget: usize,
    walk_depth: usize,
) {
    if walk_depth >= MAX_WALK_DEPTH {
        return;
    }

    // Step 1: emit sibling `properties` first so base fields (often required
    // and shared across all variants) come before any variant-specific ones.
    if let Some(props_obj) = schema.get("properties").and_then(|v| v.as_object()) {
        let required_fields: std::collections::HashSet<String> = schema
            .get("required")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default();

        for (field_name, field_schema) in props_obj {
            let path = if prefix.is_empty() {
                field_name.clone()
            } else {
                format!("{}.{}", prefix, field_name)
            };
            let type_str = extract_type(field_schema);
            let is_req = required_fields.contains(field_name);
            let desc = compose_description(field_schema);

            rows.push(vec![
                path.clone(),
                type_str.clone(),
                if is_req { "yes".into() } else { "no".into() },
                desc,
            ]);

            if depth_budget == 0 {
                continue;
            }

            // Recurse: object properties, oneOf/anyOf variants, or array item schemas.
            if field_schema.get("properties").is_some()
                || field_schema.get("oneOf").is_some()
                || field_schema.get("anyOf").is_some()
            {
                walk_schema(rows, &path, field_schema, depth_budget - 1, walk_depth + 1);
            } else if type_str == "array" {
                if let Some(items) = field_schema.get("items") {
                    let items_prefix = format!("{}[]", path);
                    walk_schema(rows, &items_prefix, items, depth_budget - 1, walk_depth + 1);
                }
            }
        }
    }

    // Step 2: walk the first variant of any oneOf/anyOf at this node. Variants
    // compose with sibling `properties` in JSON Schema (base + variant pattern),
    // so we must visit both — not return early after the union branch. Union
    // descent does not consume `depth_budget` (variants are alternatives at
    // the same logical level), but `walk_depth` always increments so chained
    // union wrappers cannot bypass MAX_WALK_DEPTH.
    for key in ["oneOf", "anyOf"] {
        if let Some(variants) = schema.get(key).and_then(|v| v.as_array()) {
            if let Some(first) = variants.first() {
                walk_schema(rows, prefix, first, depth_budget, walk_depth + 1);
            }
            break;
        }
    }
}

// Returns `None` when the schema produces no field rows (e.g. a oneOf whose
// first variant is a scalar, or a bare `{"type": "string"}` body) — the caller
// should fall back to type/description text instead of printing an empty table.
fn format_schema_table(schema: &serde_json::Value, depth_budget: usize) -> Option<String> {
    let header: Vec<String> = vec![
        "FIELD".into(),
        "TYPE".into(),
        "REQUIRED".into(),
        "DESCRIPTION".into(),
    ];
    let mut table_rows: Vec<Vec<String>> = vec![header];

    walk_schema(&mut table_rows, "", schema, depth_budget, 0);

    if table_rows.len() == 1 {
        return None;
    }

    let aligned = formatter::align_columns(&table_rows, 2);
    let mut out = String::new();
    for line in &aligned {
        out.push_str(&format!("  {}\n", line));
    }
    Some(out)
}

// Renders a top-level schema as a single-line type label. Used when
// `format_schema_table` produces no rows (e.g. a response that's a bare $ref,
// or `array<$ref>`) so we still surface *something* about the shape.
fn describe_top_type(schema: &serde_json::Value) -> String {
    describe_top_type_with_depth(schema, 0)
}

fn describe_top_type_with_depth(schema: &serde_json::Value, walk_depth: usize) -> String {
    if walk_depth >= MAX_WALK_DEPTH {
        return "any".to_string();
    }

    let t = extract_type(schema);
    if t == "array" {
        if let Some(items) = schema.get("items") {
            return format!("array<{}>", describe_top_type_with_depth(items, walk_depth + 1));
        }
    }
    t
}

fn extract_type(schema: &serde_json::Value) -> String {
    if let Some(t) = schema.get("type").and_then(|v| v.as_str()) {
        return t.to_string();
    }
    if let Some(r) = schema.get("$ref").and_then(|v| v.as_str()) {
        return r.rsplit('/').next().unwrap_or("object").to_string();
    }
    // Infer type from enum values when `type` is missing — skip nulls since
    // JSON Schema encodes nullability as `[null, "actual"]`, and fall back to
    // `any` if enum members span multiple scalar types.
    if let Some(arr) = schema.get("enum").and_then(|v| v.as_array()) {
        if let Some(enum_type) = infer_enum_type(arr) {
            return enum_type;
        }
    }
    if let Some(c) = schema.get("const") {
        return infer_scalar_type(c);
    }
    "any".to_string()
}

fn infer_enum_type(values: &[serde_json::Value]) -> Option<String> {
    let mut inferred: Option<String> = None;

    for value in values {
        if value.is_null() {
            continue;
        }

        let value_type = infer_scalar_type(value);
        if value_type == "any" {
            return Some("any".into());
        }

        match &inferred {
            None => inferred = Some(value_type),
            Some(existing) if existing == &value_type => {}
            Some(_) => return Some("any".into()),
        }
    }

    inferred
}

fn infer_scalar_type(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(_) => "string".into(),
        serde_json::Value::Number(_) => "number".into(),
        serde_json::Value::Bool(_) => "boolean".into(),
        _ => "any".into(),
    }
}

fn json_scalar_display(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::Null => "null".into(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::String(s) => s.clone(),
        _ => v.to_string(),
    }
}

// Extracts value-level constraints (const / enum) as a bracketed prefix to
// splice into the DESCRIPTION column. Keeping constraints in the description
// leaves TYPE as pure JSON-type info and avoids blowing the table layout when
// enums contain many values.
fn schema_constraints(schema: &serde_json::Value) -> Option<String> {
    if let Some(c) = schema.get("const") {
        return Some(format!("[const: {}]", json_scalar_display(c)));
    }
    if let Some(arr) = schema.get("enum").and_then(|v| v.as_array()) {
        if arr.is_empty() {
            return None;
        }
        if arr.len() == 1 {
            return Some(format!("[const: {}]", json_scalar_display(&arr[0])));
        }
        let values: Vec<String> = arr.iter().map(json_scalar_display).collect();
        return Some(format!("[enum: {}]", values.join("|")));
    }
    None
}

fn compose_description(schema: &serde_json::Value) -> String {
    let desc = schema.get("description").and_then(|v| v.as_str()).unwrap_or("");
    match (schema_constraints(schema), desc.is_empty()) {
        (Some(c), true) => c,
        (Some(c), false) => format!("{} {}", c, desc),
        (None, _) => desc.to_string(),
    }
}

fn format_action_detail(data: &ActionDetail) -> String {
    let is_graphql = data.method == "QUERY" || data.method == "MUTATION";

    let mut output = String::new();

    // Header
    output.push_str(&format!("  === {}\n\n", data.action));

    // Metadata block
    output.push_str(&format!("  site:      {}\n", data.site));
    if is_graphql {
        output.push_str(&format!("  type:      {}\n", data.method));
        output.push_str(&format!("  field:     {}\n", data.path));
        if let Some(ref base_url) = data.base_url {
            output.push_str(&format!("  endpoint:  POST {}\n", base_url));
        }
    } else {
        output.push_str(&format!("  method:    {}\n", data.method));
        output.push_str(&format!("  path:      {}\n", data.path));
        if let Some(ref base_url) = data.base_url {
            output.push_str(&format!("  base_url:  {}\n", base_url));
        }
    }
    if let Some(ref auth) = data.authentication {
        output.push_str(&format!("  auth:      {}\n", format_auth_from_struct(auth, &data.site)));
    }

    // Separator
    output.push_str("\n  ---\n\n");

    // Title
    output.push_str(&format!("  {}\n", action_to_title(&data.action)));

    // Description
    if !data.description.is_empty() {
        output.push_str(&format!("\n  {}\n", data.description));
    }

    // Parameters
    if !data.parameters.is_empty() {
        let header_label = if is_graphql { "## Arguments" } else { "## Parameters" };
        output.push_str(&format!("\n  {}\n\n", header_label));

        let field_label = if is_graphql { "ARGUMENT" } else { "FIELD" };
        let mut table_rows: Vec<Vec<String>> = vec![vec![
            field_label.into(),
            "TYPE".into(),
            "REQUIRED".into(),
            "DESCRIPTION".into(),
        ]];

        for p in &data.parameters {
            table_rows.push(vec![
                p.name.clone(),
                p.param_type.clone(),
                if p.required { "yes".into() } else { "no".into() },
                p.description.clone(),
            ]);
        }

        let aligned = formatter::align_columns(&table_rows, 2);
        for line in &aligned {
            output.push_str(&format!("  {}\n", line));
        }
    }

    // Request Body
    if let Some(ref body) = data.request_body {
        if let Some(ref schema) = body.schema {
            output.push_str("\n  ## Request Body\n\n");

            if let Some(table) = format_schema_table(schema, REQUEST_SCHEMA_DEPTH) {
                output.push_str(&table);
            } else if let Some(desc) = schema.get("description").and_then(|v| v.as_str()) {
                let type_str = schema.get("type").and_then(|v| v.as_str()).unwrap_or("object");
                output.push_str(&format!("  Type: {}\n", type_str));
                output.push_str(&format!("  {}\n", desc));
            }
        }
    }

    // Responses
    if !data.responses.is_empty() {
        output.push_str("\n  ## Response\n");
        for r in &data.responses {
            output.push_str(&format!("\n  **{}** — {}\n", r.status, r.description));

            // Render response schema fields if available; fall back to a
            // one-line type label when the schema produces no rows (e.g.
            // a bare $ref or `array<$ref>` with nothing to walk into).
            if let Some(ref schema) = r.schema {
                if let Some(table) = format_schema_table(schema, RESPONSE_SCHEMA_DEPTH) {
                    output.push('\n');
                    output.push_str(&table);
                } else {
                    let type_label = describe_top_type(schema);
                    if type_label != "any" {
                        output.push_str(&format!("\n  Type: {}\n", type_label));
                    }
                }
            }
        }
    }

    // Send hint
    output.push_str("\n  ---\n\n");
    output.push_str("  Next step: use `postagent send <CURL_QUERY>` to send an HTTP request with a cURL syntax.\n");
    if data.authentication.is_some() {
        let key_var = format!("$POSTAGENT.{}.API_KEY", data.site.to_uppercase());
        output.push_str(&format!(
            "  `postagent send` will replace {} with your saved credentials.\n",
            key_var
        ));
    }

    output.trim_end().to_string()
}

fn urlencoding(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => c.to_string(),
            ' ' => "%20".to_string(),
            _ => format!("%{:02X}", c as u8),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn urlencoding_basic() {
        assert_eq!(urlencoding("hello world"), "hello%20world");
        assert_eq!(urlencoding("abc123"), "abc123");
        assert_eq!(urlencoding("-_.~"), "-_.~");
    }

    #[test]
    fn urlencoding_special_chars() {
        assert_eq!(urlencoding("a+b"), "a%2Bb");
        assert_eq!(urlencoding("foo@bar"), "foo%40bar");
    }

    #[test]
    fn run_without_site_returns_show_help() {
        let result = run(None, None, None, false);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().to_string(), "show_help");
    }

    #[test]
    fn action_to_title_basic() {
        assert_eq!(action_to_title("create_page"), "Create Page");
        assert_eq!(
            action_to_title("retrieve_block_children"),
            "Retrieve Block Children"
        );
        assert_eq!(action_to_title("search_by_title"), "Search By Title");
    }

    #[test]
    fn extract_type_basic() {
        assert_eq!(extract_type(&json!({"type": "string"})), "string");
        assert_eq!(extract_type(&json!({"type": "object"})), "object");
        assert_eq!(
            extract_type(&json!({"$ref": "#/components/schemas/Parent"})),
            "Parent"
        );
        assert_eq!(extract_type(&json!({})), "any");
        assert_eq!(
            extract_type(&json!({"enum": ["queued", "running", null]})),
            "string"
        );
        assert_eq!(extract_type(&json!({"enum": [1, "auto"]})), "any");
    }

    #[test]
    fn format_site_overview_notion() {
        let data: SiteOverview = serde_json::from_value(json!({
            "name": "notion",
            "description": "All requests are sent to `https://api.notion.com`. Authentication uses tokens via `Authorization: Bearer <token>` header. Every request must include the `Notion-Version` header (latest: `2026-03-11`). Docs at `developers.notion.com`.",
            "authentication": {
                "in": "header",
                "name": "Authorization",
                "type": "bearer",
                "description": "Notion integration token"
            },
            "groups": [
                { "name": "blocks", "base_url": "https://api.notion.com", "actions": ["retrieve_block", "update_block", "delete_block"] },
                { "name": "pages", "base_url": "https://api.notion.com", "actions": ["create_page", "retrieve_page"] }
            ]
        }))
        .unwrap();

        let output = format_site_overview(&data);
        assert!(output.contains("=== notion"));
        assert!(output.contains("Base URL:"));
        assert!(output.contains("https://api.notion.com"));
        assert!(output.contains("Auth:"));
        assert!(output.contains("blocks"));
        assert!(output.contains("pages"));
        assert!(output.contains("retrieve_block"));
        assert!(output.contains("create_page"));
        assert!(output.contains("2 groups, 5 actions total"));
    }

    #[test]
    fn format_site_overview_truncation() {
        let actions: Vec<SiteAction> = (0..15).map(|i| SiteAction::Simple(format!("action_{}", i))).collect();
        let data = SiteOverview {
            name: "test".into(),
            description: "".into(),
            authentication: None,
            groups: vec![SiteGroup {
                name: "big_group".into(),
                base_url: None,
                actions,
            }],
        };

        let output = format_site_overview(&data);
        assert!(output.contains("... 10 more actions"));
        assert!(output.contains("action_0"));
        assert!(output.contains("action_4"));
    }

    #[test]
    fn format_site_overview_uses_group_base_url_fallback() {
        let data = SiteOverview {
            name: "notion".into(),
            description: "Docs at `developers.notion.com`.".into(),
            authentication: None,
            groups: vec![SiteGroup {
                name: "pages".into(),
                base_url: Some("https://api.notion.com".into()),
                actions: vec![SiteAction::Simple("create_page".into())],
            }],
        };

        let output = format_site_overview(&data);
        assert!(output.contains("Base URL:  https://api.notion.com"));
    }

    #[test]
    fn format_group_overview_basic() {
        let data: GroupOverview = serde_json::from_value(json!({
            "group": "pages",
            "base_url": "https://api.notion.com",
            "actions": [
                { "name": "create_page", "method": "POST", "path": "/v1/pages", "base_url": "https://api.notion.com", "summary": "Create a page" },
                { "name": "retrieve_page", "method": "GET", "path": "/v1/pages/{page_id}", "base_url": "https://api.notion.com", "summary": "Retrieve a page" }
            ]
        }))
        .unwrap();

        let output = format_group_overview(&data, "notion");
        assert!(output.contains("site:      notion"));
        assert!(output.contains("group:     pages"));
        assert!(output.contains("actions:   2"));
        assert!(output.contains("create_page"));
        assert!(output.contains("POST"));
        assert!(output.contains("/v1/pages"));
        assert!(output.contains("Create a page"));
        assert!(output.contains("Run postagent manual <SITE> [GROUP] [ACTION] for full details."));
    }

    #[test]
    fn format_group_overview_shows_all_actions() {
        let actions: Vec<serde_json::Value> = (0..25)
            .map(|i| {
                json!({
                    "name": format!("action_{}", i),
                    "method": "GET",
                    "path": format!("/v1/action_{}", i),
                    "base_url": "https://api.example.com",
                    "summary": format!("Action {}", i),
                })
            })
            .collect();
        let data: GroupOverview = serde_json::from_value(json!({
            "group": "pages",
            "base_url": "https://api.example.com",
            "actions": actions
        }))
        .unwrap();

        let output = format_group_overview(&data, "example");
        assert!(output.contains("site:      example"));
        assert!(output.contains("group:     pages"));
        assert!(output.contains("actions:   25"));
        assert!(output.contains("action_0"));
        assert!(output.contains("action_24"));
        assert!(!output.contains("showing first 20"));
        assert!(!output.contains("Use --all"));
    }

    #[test]
    fn format_action_detail_restful() {
        let data: ActionDetail = serde_json::from_value(json!({
            "site": "notion",
            "group": "pages",
            "action": "create_page",
            "method": "POST",
            "path": "/v1/pages",
            "base_url": "https://api.notion.com",
            "description": "Creates a new page as a child of an existing page.",
            "parameters": [],
            "requestBody": {
                "contentType": "application/json",
                "schema": {
                    "type": "object",
                    "required": ["properties"],
                    "properties": {
                        "parent": { "$ref": "#/components/schemas/Parent" },
                        "properties": { "type": "object", "description": "Page properties." },
                        "children": { "type": "array", "description": "Block objects." }
                    }
                }
            },
            "responses": [
                { "status": "200", "description": "The newly created page object.", "schema": {} }
            ],
            "authentication": {
                "in": "header",
                "name": "Authorization",
                "type": "bearer",
                "description": "Notion integration token"
            }
        }))
        .unwrap();

        let output = format_action_detail(&data);
        assert!(output.contains("=== create_page"));
        assert!(output.contains("site:      notion"));
        assert!(output.contains("method:    POST"));
        assert!(output.contains("path:      /v1/pages"));
        assert!(output.contains("base_url:  https://api.notion.com"));
        assert!(output.contains("auth:"));
        assert!(output.contains("---"));
        assert!(output.contains("Create Page"));
        assert!(output.contains("## Request Body"));
        assert!(output.contains("FIELD"));
        assert!(output.contains("parent"));
        assert!(output.contains("properties"));
        assert!(output.contains("## Response"));
        assert!(output.contains("**200**"));
    }

    #[test]
    fn format_action_detail_graphql() {
        let data: ActionDetail = serde_json::from_value(json!({
            "site": "shopify",
            "group": "queries",
            "action": "customer",
            "method": "QUERY",
            "path": "customer",
            "base_url": "https://{store}.myshopify.com/admin/api/2026-04/graphql.json",
            "description": "Returns a Customer resource by ID.",
            "parameters": [
                { "name": "id", "in": "argument", "type": "ID!", "required": true, "description": "The Shopify global ID" }
            ],
            "requestBody": null,
            "responses": [{ "status": "success", "description": "JSON", "schema": {} }],
            "authentication": {
                "in": "header",
                "name": "X-Shopify-Access-Token",
                "type": "apiKey",
                "description": "admin-api-access-token"
            }
        }))
        .unwrap();

        let output = format_action_detail(&data);
        assert!(output.contains("=== customer"));
        assert!(output.contains("site:      shopify"));
        assert!(output.contains("type:      QUERY"));
        assert!(output.contains("field:     customer"));
        assert!(output.contains("## Arguments"));
        assert!(output.contains("ARGUMENT"));
        assert!(output.contains("id"));
        assert!(output.contains("ID!"));
    }

    #[test]
    fn format_action_detail_with_parameters() {
        let data: ActionDetail = serde_json::from_value(json!({
            "site": "notion",
            "group": "pages",
            "action": "retrieve_page",
            "method": "GET",
            "path": "/v1/pages/{page_id}",
            "base_url": "https://api.notion.com",
            "description": "Retrieves a Page object using the ID specified.",
            "parameters": [
                { "name": "page_id", "in": "path", "type": "string", "required": true, "description": "The ID of the page to retrieve." }
            ],
            "requestBody": null,
            "responses": [
                { "status": "200", "description": "The requested page object.", "schema": {} }
            ],
            "authentication": {
                "in": "header",
                "name": "Authorization",
                "type": "bearer",
                "description": "Notion integration token"
            }
        }))
        .unwrap();

        let output = format_action_detail(&data);
        assert!(output.contains("## Parameters"));
        assert!(output.contains("FIELD"));
        assert!(output.contains("page_id"));
        assert!(output.contains("string"));
        assert!(output.contains("yes"));
    }

    #[test]
    fn walk_schema_preserves_base_properties_with_oneof() {
        // Regression for codex-bot review on PR #5: a schema with sibling
        // `properties` next to a `oneOf` used to skip the base properties
        // entirely after recursing into the first variant.
        let schema = json!({
            "type": "object",
            "required": ["common"],
            "properties": {
                "common": { "type": "string", "description": "Always present" },
                "id":     { "type": "string", "description": "Stable id" }
            },
            "oneOf": [
                {
                    "properties": {
                        "variantField": { "type": "number", "description": "Variant-specific" }
                    }
                }
            ]
        });
        let table = format_schema_table(&schema, REQUEST_SCHEMA_DEPTH).expect("table");
        assert!(table.contains("common"), "base property `common` must appear");
        assert!(table.contains("Always present"));
        assert!(table.contains("id"), "base property `id` must appear");
        assert!(table.contains("variantField"), "variant field must appear");
        assert!(table.contains("Variant-specific"));

        // Base properties should be rendered before variant fields so the
        // shared/required parts read first.
        let common_pos = table.find("common").unwrap();
        let variant_pos = table.find("variantField").unwrap();
        assert!(common_pos < variant_pos, "base props must precede variant fields");
    }

    #[test]
    fn walk_schema_caps_pathological_union_chain() {
        // Regression for codex-bot review on PR #5: a long oneOf chain bypassed
        // depth_budget because union descent doesn't consume it. MAX_WALK_DEPTH
        // is the absolute guard that prevents stack/runtime blowups on
        // pathological specs. Wrap a leaf object in 200 oneOf layers — must
        // return without panicking or stack-overflowing.
        let mut schema = json!({
            "properties": { "leaf": { "type": "string", "description": "innermost" } }
        });
        for _ in 0..200 {
            schema = json!({ "oneOf": [schema] });
        }
        // The outer layers exceed MAX_WALK_DEPTH so the leaf is unreachable —
        // the test's purpose is just to confirm the call terminates safely.
        let result = format_schema_table(&schema, REQUEST_SCHEMA_DEPTH);
        // Either None (no rows) or Some(table); both are acceptable. The
        // important thing is we got here without panicking.
        let _ = result;
    }

    #[test]
    fn format_schema_table_returns_none_for_scalar_oneof() {
        // oneOf whose first variant is a scalar — walk_schema produces no rows,
        // so we must return None to let the caller fall back to type/description text.
        let schema = json!({
            "oneOf": [
                { "type": "string", "description": "Raw JSON string body" }
            ]
        });
        assert!(format_schema_table(&schema, REQUEST_SCHEMA_DEPTH).is_none());
    }

    #[test]
    fn format_action_detail_falls_back_for_scalar_oneof_body() {
        // Regression for codex-bot review on PR #5: a oneOf body whose first
        // variant is non-object used to render an empty table and drop the
        // type/description fallback entirely.
        let data: ActionDetail = serde_json::from_value(json!({
            "site": "demo",
            "group": "raw",
            "action": "send_text",
            "method": "POST",
            "path": "/v1/text",
            "base_url": "https://example.com",
            "description": "Send a raw text payload.",
            "parameters": [],
            "requestBody": {
                "contentType": "application/json",
                "schema": {
                    "type": "string",
                    "description": "The raw body content to send.",
                    "oneOf": [
                        { "type": "string", "description": "A plain string." }
                    ]
                }
            },
            "responses": [
                { "status": "200", "description": "OK", "schema": {} }
            ],
            "authentication": null
        }))
        .unwrap();

        let output = format_action_detail(&data);
        assert!(output.contains("## Request Body"));
        assert!(output.contains("Type: string"));
        assert!(output.contains("The raw body content to send."));
        assert!(!output.contains("FIELD"));
    }

    #[test]
    fn format_action_detail_falls_back_for_array_of_ref_response() {
        // Regression: a response schema like `array<$ref>` produces no table
        // rows (no top-level `properties`/`oneOf`, and walk_schema only
        // recurses into array items from inside a property). Without a
        // fallback the user saw just the status line and no shape info.
        let data: ActionDetail = serde_json::from_value(json!({
            "site": "github",
            "group": "projects",
            "action": "list",
            "method": "GET",
            "path": "/orgs/{org}/projectsV2",
            "base_url": "https://api.github.com",
            "description": "List org projects.",
            "parameters": [],
            "responses": [
                {
                    "status": "200",
                    "description": "Response",
                    "schema": {
                        "type": "array",
                        "items": { "$ref": "#/components/schemas/projects-v2" }
                    }
                },
                {
                    "status": "204",
                    "description": "No content",
                    "schema": { "$ref": "#/components/schemas/projects-v2" }
                }
            ],
            "authentication": null
        }))
        .unwrap();

        let output = format_action_detail(&data);
        assert!(output.contains("**200**"));
        assert!(output.contains("Type: array<projects-v2>"));
        assert!(output.contains("**204**"));
        assert!(output.contains("Type: projects-v2"));
    }

    #[test]
    fn describe_top_type_caps_deeply_nested_arrays() {
        let mut schema = json!({ "$ref": "#/components/schemas/projects-v2" });
        for _ in 0..=MAX_WALK_DEPTH {
            schema = json!({
                "type": "array",
                "items": schema
            });
        }

        let rendered = describe_top_type(&schema);
        assert!(rendered.contains("array<any>"));
        assert!(!rendered.contains("projects-v2"));
        assert_eq!(rendered.matches("array<").count(), MAX_WALK_DEPTH);
    }
}
