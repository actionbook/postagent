use reqwest::blocking::Client;
use serde::Deserialize;

use crate::config;
use crate::formatter;

// === L1 data structures ===

#[derive(Deserialize, Clone)]
struct Authentication {
    #[serde(rename = "in")]
    #[allow(dead_code)]
    location: String,
    name: String,
    #[serde(rename = "type")]
    auth_type: String,
    description: String,
}

#[derive(Deserialize)]
struct L1Group {
    name: String,
    #[allow(dead_code)]
    base_url: Option<String>,
    actions: Vec<String>,
}

#[derive(Deserialize)]
struct L1Response {
    name: String,
    description: String,
    authentication: Option<Authentication>,
    groups: Vec<L1Group>,
}

// === L2 data structures ===

#[derive(Deserialize)]
struct L2Action {
    name: String,
    method: String,
    path: String,
    #[allow(dead_code)]
    base_url: Option<String>,
    summary: String,
}

#[derive(Deserialize)]
struct L2Response {
    group: String,
    #[allow(dead_code)]
    base_url: Option<String>,
    actions: Vec<L2Action>,
}

// === L3 data structures ===

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
    schema: serde_json::Value,
}

#[derive(Deserialize)]
struct ResponseInfo {
    status: String,
    description: String,
    schema: Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct L3Response {
    project: String,
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
    project: Option<&str>,
    group: Option<&str>,
    action: Option<&str>,
    json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let project = match project {
        Some(p) => p,
        None => {
            return Err("show_help".into());
        }
    };

    // Build the API URL
    let mut params = vec![("project", project.to_string())];
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
        if let Some(error) = body.get("error").and_then(|v| v.as_str()) {
            eprintln!("{}", error);
        }
        if let Some(available) = body.get("available").and_then(|v| v.as_array()) {
            let items: Vec<&str> = available.iter().filter_map(|v| v.as_str()).collect();
            eprintln!("Available: {}", items.join(", "));
        }
        std::process::exit(1);
    }

    let body_text = response.text()?;

    if json {
        let value: serde_json::Value = serde_json::from_str(&body_text)?;
        println!("{}", serde_json::to_string_pretty(&value)?);
        return Ok(());
    }

    if group.is_none() {
        let data: L1Response = serde_json::from_str(&body_text)?;
        println!("{}", format_l1(&data));
    } else if action.is_none() {
        let data: L2Response = serde_json::from_str(&body_text)?;
        println!("{}", format_l2(&data, project));
    } else {
        let data: L3Response = serde_json::from_str(&body_text)?;
        println!("{}", format_l3(&data));
    }

    Ok(())
}

// === L1: Project Overview ===

struct ProjectMeta {
    base_url: Option<String>,
    auth: Option<String>,
    header: Option<String>,
    source: Option<String>,
    endpoint: Option<String>,
    api_type: Option<String>,
}

fn extract_meta_from_l1(data: &L1Response) -> ProjectMeta {
    let description = &data.description;
    let is_graphql = description.to_lowercase().contains("graphql");

    // Base URL from description or first group
    let base_url_re = regex::Regex::new(r"`(https?://[^`]+)`").ok();
    let base_url = base_url_re
        .and_then(|re| re.captures(description).map(|c| c[1].to_string()));

    // Auth from authentication struct
    let auth = data.authentication.as_ref().map(|a| {
        if a.auth_type == "bearer" {
            format!("{}: Bearer <token>", a.name)
        } else {
            format!("{}: <{}>", a.name, a.description)
        }
    });

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
        let gql_auth = data.authentication.as_ref().map(|a| {
            format!("{}: <{}>", a.name, a.description)
        });

        ProjectMeta {
            base_url: None,
            auth: gql_auth.or(auth),
            header: None,
            source,
            endpoint,
            api_type: Some("GraphQL".into()),
        }
    } else {
        ProjectMeta {
            base_url,
            auth,
            header,
            source,
            endpoint: None,
            api_type: None,
        }
    }
}

fn format_auth_from_struct(auth: &Authentication) -> String {
    if auth.auth_type == "bearer" {
        format!("{}: Bearer <token>", auth.name)
    } else {
        format!("{}: <{}>", auth.name, auth.description)
    }
}

fn format_l1(data: &L1Response) -> String {
    let meta = extract_meta_from_l1(data);

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

    // Render groups
    let mut total_actions = 0;
    for group in &data.groups {
        let count = group.actions.len();
        total_actions += count;
        output.push_str(&format!("\n  {}:\n", group.name));

        if count <= 10 {
            let max_name_width = group.actions.iter().map(|a| a.len()).max().unwrap_or(0);
            for action in &group.actions {
                output.push_str(&format!("    {:<width$}\n", action, width = max_name_width));
            }
        } else {
            let display_actions = &group.actions[..5];
            let max_name_width = display_actions.iter().map(|a| a.len()).max().unwrap_or(0);
            for action in display_actions {
                output.push_str(&format!("    {:<width$}\n", action, width = max_name_width));
            }
            output.push_str(&format!("    ... {} more actions\n", count - 5));
        }
    }

    output.push_str(&format!(
        "\n  {} groups, {} actions total\n",
        data.groups.len(),
        total_actions
    ));

    output.push_str(
        "\n  Run postagent manual <project> <group> <action> for full details.\n",
    );
    if let Some(first_group) = data.groups.first() {
        if let Some(first_action) = first_group.actions.first() {
            output.push_str(&format!(
                "  Example: postagent manual {} {} {}",
                data.name, first_group.name, first_action
            ));
        }
    }

    output
}

// === L2: Group Actions ===

fn format_l2(data: &L2Response, project: &str) -> String {
    let total = data.actions.len();
    let display_actions = if total > 20 { &data.actions[..20] } else { &data.actions[..] };

    let mut output = String::new();

    if total > 20 {
        output.push_str(&format!(
            "  {}/{} — {} actions (showing first 20)\n",
            project, data.group, total
        ));
    } else {
        output.push_str(&format!(
            "  {}/{} — {} actions\n",
            project, data.group, total
        ));
    }

    output.push_str("\n  Actions:\n");

    let mut table_rows: Vec<Vec<String>> = Vec::new();
    for a in display_actions {
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

    if total > 20 {
        output.push_str(&format!(
            "\n  Showing 20 of {}. Use --all to see all actions.\n",
            total
        ));
    }

    output.push_str(
        "\n  Run postagent manual <project> <group> <action> for full details.",
    );

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

fn format_schema_table(
    props_obj: &serde_json::Map<String, serde_json::Value>,
    schema: &serde_json::Value,
) -> String {
    let required_fields: std::collections::HashSet<String> = schema
        .get("required")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();

    let mut table_rows: Vec<Vec<String>> = vec![vec![
        "FIELD".into(),
        "TYPE".into(),
        "REQUIRED".into(),
        "DESCRIPTION".into(),
    ]];

    for (field_name, field_schema) in props_obj {
        let type_str = extract_type(field_schema);
        let is_req = required_fields.contains(field_name);
        let desc = field_schema
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        table_rows.push(vec![
            field_name.clone(),
            type_str,
            if is_req { "yes".into() } else { "no".into() },
            desc.to_string(),
        ]);
    }

    let aligned = formatter::align_columns(&table_rows, 2);
    let mut out = String::new();
    for line in &aligned {
        out.push_str(&format!("  {}\n", line));
    }
    out
}

fn extract_type(schema: &serde_json::Value) -> String {
    if let Some(t) = schema.get("type").and_then(|v| v.as_str()) {
        return t.to_string();
    }
    if let Some(r) = schema.get("$ref").and_then(|v| v.as_str()) {
        return r.rsplit('/').next().unwrap_or("object").to_string();
    }
    "any".to_string()
}

fn format_l3(data: &L3Response) -> String {
    let is_graphql = data.method == "QUERY" || data.method == "MUTATION";

    let mut output = String::new();

    // Header
    output.push_str(&format!("  === {}\n\n", data.action));

    // Metadata block
    output.push_str(&format!("  project:   {}\n", data.project));
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
        output.push_str(&format!("  auth:      {}\n", format_auth_from_struct(auth)));
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
        output.push_str("\n  ## Request Body\n\n");

        if let Some(props) = body.schema.get("properties") {
            if let Some(props_obj) = props.as_object() {
                output.push_str(&format_schema_table(props_obj, &body.schema));
            }
        } else if let Some(desc) = body.schema.get("description").and_then(|v| v.as_str()) {
            // Schema without properties — just a description (e.g. "Block type-specific fields")
            let type_str = body.schema.get("type").and_then(|v| v.as_str()).unwrap_or("object");
            output.push_str(&format!("  Type: {}\n", type_str));
            output.push_str(&format!("  {}\n", desc));
        }
    }

    // Responses
    if !data.responses.is_empty() {
        output.push_str("\n  ## Response\n");
        for r in &data.responses {
            output.push_str(&format!("\n  **{}** — {}\n", r.status, r.description));

            // Render response schema fields if available
            if let Some(ref schema) = r.schema {
                if let Some(props) = schema.get("properties") {
                    if let Some(props_obj) = props.as_object() {
                        output.push('\n');
                        output.push_str(&format_schema_table(props_obj, schema));
                    }
                }
            }
        }
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
    fn run_without_project_returns_show_help() {
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
    }

    #[test]
    fn format_l1_notion() {
        let data: L1Response = serde_json::from_value(json!({
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

        let output = format_l1(&data);
        assert!(output.contains("=== notion"));
        assert!(output.contains("Base URL:"));
        assert!(output.contains("https://api.notion.com"));
        assert!(output.contains("Auth:"));
        assert!(output.contains("blocks:"));
        assert!(output.contains("pages:"));
        assert!(output.contains("retrieve_block"));
        assert!(output.contains("create_page"));
        assert!(output.contains("2 groups, 5 actions total"));
    }

    #[test]
    fn format_l1_truncation() {
        let actions: Vec<String> = (0..15).map(|i| format!("action_{}", i)).collect();
        let data = L1Response {
            name: "test".into(),
            description: "".into(),
            authentication: None,
            groups: vec![L1Group {
                name: "big_group".into(),
                base_url: None,
                actions,
            }],
        };

        let output = format_l1(&data);
        assert!(output.contains("... 10 more actions"));
        assert!(output.contains("action_0"));
        assert!(output.contains("action_4"));
    }

    #[test]
    fn format_l2_basic() {
        let data: L2Response = serde_json::from_value(json!({
            "group": "pages",
            "base_url": "https://api.notion.com",
            "actions": [
                { "name": "create_page", "method": "POST", "path": "/v1/pages", "base_url": "https://api.notion.com", "summary": "Create a page" },
                { "name": "retrieve_page", "method": "GET", "path": "/v1/pages/{page_id}", "base_url": "https://api.notion.com", "summary": "Retrieve a page" }
            ]
        }))
        .unwrap();

        let output = format_l2(&data, "notion");
        assert!(output.contains("notion/pages — 2 actions"));
        assert!(output.contains("Actions:"));
        assert!(output.contains("create_page"));
        assert!(output.contains("POST"));
        assert!(output.contains("/v1/pages"));
        assert!(output.contains("Create a page"));
        assert!(output.contains("Run postagent manual <project> <group> <action> for full details."));
    }

    #[test]
    fn format_l3_restful() {
        let data: L3Response = serde_json::from_value(json!({
            "project": "notion",
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

        let output = format_l3(&data);
        assert!(output.contains("=== create_page"));
        assert!(output.contains("project:   notion"));
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
    fn format_l3_graphql() {
        let data: L3Response = serde_json::from_value(json!({
            "project": "shopify",
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

        let output = format_l3(&data);
        assert!(output.contains("=== customer"));
        assert!(output.contains("project:   shopify"));
        assert!(output.contains("type:      QUERY"));
        assert!(output.contains("field:     customer"));
        assert!(output.contains("## Arguments"));
        assert!(output.contains("ARGUMENT"));
        assert!(output.contains("id"));
        assert!(output.contains("ID!"));
    }

    #[test]
    fn format_l3_with_parameters() {
        let data: L3Response = serde_json::from_value(json!({
            "project": "notion",
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

        let output = format_l3(&data);
        assert!(output.contains("## Parameters"));
        assert!(output.contains("FIELD"));
        assert!(output.contains("page_id"));
        assert!(output.contains("string"));
        assert!(output.contains("yes"));
    }
}
