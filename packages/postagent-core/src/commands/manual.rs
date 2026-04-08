use reqwest::blocking::Client;
use serde::Deserialize;

use crate::config;
use crate::formatter;

#[derive(Deserialize)]
struct Authentication {
    #[serde(rename = "in")]
    #[allow(dead_code)]
    location: String,
    name: String,
    #[serde(rename = "type")]
    auth_type: String,
    #[allow(dead_code)]
    description: String,
}

#[derive(Deserialize)]
struct GroupSummary {
    name: String,
    base_url: Option<String>,
    actions: Vec<String>,
}

#[derive(Deserialize)]
struct ProjectData {
    name: String,
    description: String,
    authentication: Option<Authentication>,
    groups: Vec<GroupSummary>,
}

// Search API action (has method/path/summary)
#[derive(Deserialize)]
struct SearchAction {
    name: String,
    method: String,
    path: String,
    summary: String,
}

#[derive(Deserialize)]
struct SearchGroup {
    name: String,
    actions: Vec<SearchAction>,
}

#[derive(Deserialize)]
struct SearchProject {
    name: String,
    #[allow(dead_code)]
    description: String,
    groups: Vec<SearchGroup>,
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
        params.push(("resource", g.to_string()));
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

    let data: ProjectData = serde_json::from_str(&body_text)?;

    if group.is_none() {
        // L1: Project overview
        println!("{}", format_l1(&data));
    } else if action.is_none() {
        // L2: Group actions — need search API for method/path/summary
        let group_name = group.unwrap();
        let search_data = fetch_search_data(project, &client)?;
        println!("{}", format_l2(&data, group_name, search_data.as_deref()));
    } else {
        // L3: Action detail — need search API for method/path/summary
        let group_name = group.unwrap();
        let action_name = action.unwrap();
        let search_data = fetch_search_data(project, &client)?;
        println!(
            "{}",
            format_l3(&data, group_name, action_name, search_data.as_deref())
        );
    }

    Ok(())
}

/// Fetch search data for a project to get method/path/summary info.
/// Uses `search?q=<project>` as a heuristic to get action details.
fn fetch_search_data(
    project: &str,
    client: &Client,
) -> Result<Option<Vec<SearchProject>>, Box<dyn std::error::Error>> {
    let url = format!(
        "{}/api/search?q={}",
        config::api_base(),
        urlencoding(project)
    );
    let response = client.get(&url).send()?;
    if !response.status().is_success() {
        return Ok(None);
    }
    let body: Vec<SearchProject> = response.json()?;
    Ok(Some(body))
}

/// Find action detail from search data
fn find_action_detail<'a>(
    search_data: Option<&'a [SearchProject]>,
    project: &str,
    group: &str,
    action: &str,
) -> Option<&'a SearchAction> {
    search_data?
        .iter()
        .find(|p| p.name == project)?
        .groups
        .iter()
        .find(|g| g.name == group)?
        .actions
        .iter()
        .find(|a| a.name == action)
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

fn extract_meta(data: &ProjectData) -> ProjectMeta {
    let description = &data.description;
    let is_graphql = description.to_lowercase().contains("graphql");

    // Base URL from first group
    let base_url = data
        .groups
        .first()
        .and_then(|g| g.base_url.clone());

    // Auth from authentication struct
    let auth = data.authentication.as_ref().map(|a| {
        if a.auth_type == "bearer" {
            format!("{}: Bearer <token>", a.name)
        } else {
            format!("{}: <{}>", a.name, a.auth_type)
        }
    });

    // Version header from description
    let version_re = regex::Regex::new(r"([A-Z][a-zA-Z]+-Version[^`]*?`(\d{4}-\d{2}-\d{2})`)").ok();
    let header = version_re.and_then(|re| {
        re.captures(description).map(|caps| {
            let version = caps.get(2).map(|m| m.as_str()).unwrap_or("");
            // Extract the header name from the matched text
            let full = caps.get(0).map(|m| m.as_str()).unwrap_or("");
            if full.contains("Notion-Version") {
                format!("Notion-Version: {}", version)
            } else {
                format!("{}", version)
            }
        })
    });

    // Alternative: look for explicit header pattern
    let header = header.or_else(|| {
        let re = regex::Regex::new(r"(`([A-Z][a-zA-Z]+-Version)` header \(latest: `([^`]+)`\))").ok()?;
        re.captures(description).map(|caps| {
            let name = caps.get(2).map(|m| m.as_str()).unwrap_or("");
            let version = caps.get(3).map(|m| m.as_str()).unwrap_or("");
            format!("{}: {}", name, version)
        })
    });

    // Source from description
    let source_re = regex::Regex::new(r"(?:developers?\.)[a-z0-9.-]+\.[a-z]+").ok();
    let source = source_re
        .and_then(|re| re.find(description).map(|m| m.as_str().to_string()))
        .or_else(|| {
            let re = regex::Regex::new(r"([a-z]+\.dev(?:/[^\s`\)]*[a-z])?)").ok()?;
            re.find(description).map(|m| m.as_str().trim_end_matches('.').to_string())
        });

    if is_graphql {
        // For GraphQL, construct endpoint from base_url
        let endpoint = base_url
            .as_ref()
            .map(|u| format!("POST {}", u));

        // Extract auth for GraphQL (may use different header)
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

fn format_l1(data: &ProjectData) -> String {
    let meta = extract_meta(data);

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

    // Hint
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

fn format_l2(
    data: &ProjectData,
    group_name: &str,
    search_data: Option<&[SearchProject]>,
) -> String {
    let group = data.groups.iter().find(|g| g.name == group_name);
    let actions = match group {
        Some(g) => &g.actions,
        None => {
            return format!("  Group '{}' not found in project '{}'.", group_name, data.name);
        }
    };

    let total = actions.len();
    let display_actions = if total > 20 { &actions[..20] } else { &actions[..] };

    let mut output = String::new();

    // Breadcrumb title
    if total > 20 {
        output.push_str(&format!(
            "  {}/{} — {} actions (showing first 20)\n",
            data.name, group_name, total
        ));
    } else {
        output.push_str(&format!(
            "  {}/{} — {} actions\n",
            data.name, group_name, total
        ));
    }

    output.push_str("\n  Actions:\n");

    // Build table rows — try to get method/path/summary from search data
    let mut table_rows: Vec<Vec<String>> = Vec::new();
    for action_name in display_actions {
        if let Some(detail) =
            find_action_detail(search_data, &data.name, group_name, action_name)
        {
            table_rows.push(vec![
                action_name.clone(),
                detail.method.clone(),
                detail.path.clone(),
                detail.summary.clone(),
            ]);
        } else {
            table_rows.push(vec![action_name.clone()]);
        }
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

    // Hint
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

fn format_l3(
    data: &ProjectData,
    group_name: &str,
    action_name: &str,
    search_data: Option<&[SearchProject]>,
) -> String {
    let meta = extract_meta(data);
    let detail = find_action_detail(search_data, &data.name, group_name, action_name);

    let method = detail.map(|d| d.method.as_str()).unwrap_or("");
    let path = detail.map(|d| d.path.as_str()).unwrap_or("");
    let summary = detail.map(|d| d.summary.as_str()).unwrap_or("");

    let is_graphql = method == "QUERY" || method == "MUTATION"
        || meta.api_type.as_deref() == Some("GraphQL");

    let mut output = String::new();

    // Header
    output.push_str(&format!("  === {}\n\n", action_name));

    // Metadata block
    output.push_str(&format!("  project:   {}\n", data.name));
    if is_graphql {
        if !method.is_empty() {
            output.push_str(&format!("  type:      {}\n", method));
        }
        if !path.is_empty() {
            output.push_str(&format!("  field:     {}\n", path));
        }
        if let Some(ref endpoint) = meta.endpoint {
            output.push_str(&format!("  endpoint:  {}\n", endpoint));
        }
    } else {
        if !method.is_empty() {
            output.push_str(&format!("  method:    {}\n", method));
        }
        if !path.is_empty() {
            output.push_str(&format!("  path:      {}\n", path));
        }
        if let Some(ref base_url) = meta.base_url {
            output.push_str(&format!("  base_url:  {}\n", base_url));
        }
    }
    if let Some(ref auth) = meta.auth {
        output.push_str(&format!("  auth:      {}\n", auth));
    }
    if let Some(ref header) = meta.header {
        output.push_str(&format!("  header:    {}\n", header));
    }

    // Separator
    output.push_str("\n  ---\n\n");

    // Title
    output.push_str(&format!("  {}\n", action_to_title(action_name)));

    // Summary/description
    if !summary.is_empty() {
        output.push_str(&format!("\n  {}", summary));
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

    fn sample_project_data() -> ProjectData {
        serde_json::from_value(json!({
            "name": "notion",
            "description": "Curated core operations for the Notion REST API. All requests are sent to `https://api.notion.com`. Authentication uses tokens via `Authorization: Bearer <token>` header. Every request must include the `Notion-Version` header (latest: `2026-03-11`). Docs at `developers.notion.com`.",
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
        })).unwrap()
    }

    fn sample_search_data() -> Vec<SearchProject> {
        serde_json::from_value(json!([{
            "name": "notion",
            "description": "",
            "groups": [{
                "name": "pages",
                "actions": [
                    { "name": "create_page", "method": "POST", "path": "/v1/pages", "summary": "Create a page" },
                    { "name": "retrieve_page", "method": "GET", "path": "/v1/pages/{page_id}", "summary": "Retrieve a page" }
                ]
            }]
        }]))
        .unwrap()
    }

    #[test]
    fn format_l1_notion() {
        let data = sample_project_data();
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
        assert!(output.contains("Run postagent manual <project> <group> <action> for full details."));
    }

    #[test]
    fn format_l1_truncation() {
        let actions: Vec<String> = (0..15).map(|i| format!("action_{}", i)).collect();
        let data = ProjectData {
            name: "test".into(),
            description: "".into(),
            authentication: None,
            groups: vec![GroupSummary {
                name: "big_group".into(),
                base_url: None,
                actions,
            }],
        };

        let output = format_l1(&data);
        assert!(output.contains("... 10 more actions"));
        assert!(output.contains("action_0"));
        assert!(output.contains("action_4"));
        assert!(!output.contains("action_5\n"));
    }

    #[test]
    fn format_l2_with_search_data() {
        let data = sample_project_data();
        let search = sample_search_data();
        let output = format_l2(&data, "pages", Some(&search));
        assert!(output.contains("notion/pages — 2 actions"));
        assert!(output.contains("Actions:"));
        assert!(output.contains("create_page"));
        assert!(output.contains("POST"));
        assert!(output.contains("/v1/pages"));
        assert!(output.contains("Create a page"));
        assert!(output.contains("Run postagent manual <project> <group> <action> for full details."));
    }

    #[test]
    fn format_l2_without_search_data() {
        let data = sample_project_data();
        let output = format_l2(&data, "pages", None);
        assert!(output.contains("notion/pages — 2 actions"));
        assert!(output.contains("create_page"));
        assert!(output.contains("retrieve_page"));
    }

    #[test]
    fn format_l2_group_not_found() {
        let data = sample_project_data();
        let output = format_l2(&data, "nonexistent", None);
        assert!(output.contains("not found"));
    }

    #[test]
    fn format_l3_restful() {
        let data = sample_project_data();
        let search = sample_search_data();
        let output = format_l3(&data, "pages", "create_page", Some(&search));
        assert!(output.contains("=== create_page"));
        assert!(output.contains("project:   notion"));
        assert!(output.contains("method:    POST"));
        assert!(output.contains("path:      /v1/pages"));
        assert!(output.contains("base_url:  https://api.notion.com"));
        assert!(output.contains("auth:"));
        assert!(output.contains("---"));
        assert!(output.contains("Create Page"));
        assert!(output.contains("Create a page"));
    }

    #[test]
    fn format_l3_graphql() {
        let data: ProjectData = serde_json::from_value(json!({
            "name": "shopify",
            "description": "Shopify GraphQL Admin API. Source: shopify.dev/docs/api/admin-graphql/latest",
            "authentication": {
                "in": "header",
                "name": "X-Shopify-Access-Token",
                "type": "apiKey",
                "description": "admin-api-access-token"
            },
            "groups": [{
                "name": "queries",
                "base_url": "https://{store}.myshopify.com/admin/api/2026-04/graphql.json",
                "actions": ["customer"]
            }]
        }))
        .unwrap();

        let search: Vec<SearchProject> = serde_json::from_value(json!([{
            "name": "shopify",
            "description": "",
            "groups": [{
                "name": "queries",
                "actions": [
                    { "name": "customer", "method": "QUERY", "path": "customer", "summary": "Returns a Customer resource by ID" }
                ]
            }]
        }]))
        .unwrap();

        let output = format_l3(&data, "queries", "customer", Some(&search));
        assert!(output.contains("=== customer"));
        assert!(output.contains("project:   shopify"));
        assert!(output.contains("type:      QUERY"));
        assert!(output.contains("field:     customer"));
        assert!(output.contains("Customer"));
    }

    #[test]
    fn extract_meta_restful() {
        let data = sample_project_data();
        let meta = extract_meta(&data);
        assert_eq!(meta.base_url.as_deref(), Some("https://api.notion.com"));
        assert!(meta.auth.is_some());
        assert!(meta.auth.as_ref().unwrap().contains("Bearer"));
        assert!(meta.api_type.is_none());
    }

    #[test]
    fn extract_meta_graphql() {
        let data: ProjectData = serde_json::from_value(json!({
            "name": "shopify",
            "description": "Shopify GraphQL Admin API. Source: shopify.dev/docs/api/admin-graphql/latest",
            "authentication": {
                "in": "header",
                "name": "X-Shopify-Access-Token",
                "type": "apiKey",
                "description": "admin-api-access-token"
            },
            "groups": [{
                "name": "queries",
                "base_url": "https://{store}.myshopify.com/admin/api/2026-04/graphql.json",
                "actions": ["customer"]
            }]
        }))
        .unwrap();

        let meta = extract_meta(&data);
        assert_eq!(meta.api_type.as_deref(), Some("GraphQL"));
        assert!(meta.endpoint.is_some());
    }
}
