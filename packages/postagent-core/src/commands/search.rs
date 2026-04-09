use std::collections::HashSet;

use reqwest::blocking::Client;
use serde::Deserialize;

use crate::api_response;
use crate::config;
use crate::formatter;

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
struct SearchSite {
    name: String,
    #[allow(dead_code)]
    description: String,
    groups: Vec<SearchGroup>,
}

struct FlatRow {
    site: String,
    group: String,
    action: String,
    method: String,
    path: String,
    summary: String,
    score: f64,
}

pub fn run(query: &str, json: bool) -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::new();
    let url = format!("{}/api/search?q={}", config::api_base(), urlencoding(query));

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

    let sites: Vec<SearchSite> = serde_json::from_value(data)?;
    let output = format_search_results(&sites, query);
    println!("{}", output);
    Ok(())
}

fn score_action(query_words: &[String], site: &str, group: &str, action: &str, summary: &str) -> f64 {
    let mut score = 0.0;
    let site_lower = site.to_lowercase();
    let group_lower = group.to_lowercase();
    let action_lower = action.to_lowercase();
    let summary_lower = summary.to_lowercase();
    // Action name parts (split by underscore)
    let action_parts: Vec<&str> = action_lower.split('_').collect();

    for word in query_words {
        // Exact site name match is a strong signal
        if site_lower == *word {
            score += 3.0;
        } else if site_lower.contains(word.as_str()) {
            score += 1.5;
        }
        // Action name match
        if action_parts.iter().any(|p| p == word) {
            score += 2.0;
        } else if action_lower.contains(word.as_str()) {
            score += 1.0;
        }
        // Group name match
        if group_lower.contains(word.as_str()) {
            score += 1.0;
        }
        // Summary match
        if summary_lower.contains(word.as_str()) {
            score += 0.5;
        }
    }

    score
}

fn format_search_results(sites: &[SearchSite], query: &str) -> String {
    let query_words: Vec<String> = query
        .to_lowercase()
        .split_whitespace()
        .filter(|w| !["a", "an", "the", "in", "from", "for", "to", "of", "all", "new"].contains(w))
        .map(String::from)
        .collect();

    // Flatten all actions across sites and groups, with scoring
    let mut rows: Vec<FlatRow> = Vec::new();
    for p in sites {
        for g in &p.groups {
            for a in &g.actions {
                let score = score_action(&query_words, &p.name, &g.name, &a.name, &a.summary);
                rows.push(FlatRow {
                    site: p.name.clone(),
                    group: g.name.clone(),
                    action: a.name.clone(),
                    method: a.method.clone(),
                    path: a.path.clone(),
                    summary: a.summary.clone(),
                    score,
                });
            }
        }
    }

    // Filter out zero-score results and sort by score descending
    rows.retain(|r| r.score > 0.0);
    rows.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.site.cmp(&b.site))
            .then_with(|| a.action.cmp(&b.action))
    });

    // Dedup by (site, group, action)
    let mut seen = HashSet::new();
    rows.retain(|r| seen.insert((r.site.clone(), r.group.clone(), r.action.clone())));

    // Limit to top 20 results
    rows.truncate(20);

    if rows.is_empty() {
        return "No results found.".to_string();
    }

    // Count distinct sites
    let site_count = rows
        .iter()
        .map(|r| r.site.as_str())
        .collect::<HashSet<_>>()
        .len();
    let result_count = rows.len();

    // Group by site -> group, preserving score order
    let mut sites: Vec<(String, Vec<(String, Vec<&FlatRow>)>)> = Vec::new();
    for r in &rows {
        let site_entry = sites.iter_mut().find(|(s, _)| s == &r.site);
        if let Some((_, groups)) = site_entry {
            let group_entry = groups.iter_mut().find(|(g, _)| g == &r.group);
            if let Some((_, items)) = group_entry {
                items.push(r);
            } else {
                groups.push((r.group.clone(), vec![r]));
            }
        } else {
            sites.push((r.site.clone(), vec![(r.group.clone(), vec![r])]));
        }
    }

    let mut output = String::new();

    output.push_str(&format!(
        "  {} actions from {} sites\n\n",
        result_count, site_count
    ));
    output.push_str("  Results are listed as:\n\n");
    output.push_str("  site                        # API provider\n");
    output.push_str("    group                     # Action group\n");
    output.push_str("      action  summary         # Action name and description\n");
    output.push_str("\n  ---\n\n");

    for (site, groups) in &sites {
        output.push_str(&format!("  {}\n", site));
        for (group, items) in groups {
            output.push_str(&format!("    {}\n", group));

            let mut table_rows: Vec<Vec<String>> = Vec::new();
            for r in items {
                table_rows.push(vec![r.action.clone(), r.summary.clone()]);
            }

            let aligned = formatter::align_columns(&table_rows, 2);
            for line in &aligned {
                output.push_str(&format!("      {}\n", line));
            }
        }
        output.push('\n');
    }

    // Hint with example using best match (first row)
    output.push_str("  Run postagent manual <SITE> [GROUP] [ACTION] for full details.\n");
    output.push_str(&format!(
        "  Example: postagent manual {}  # List all groups and actions\n",
        rows[0].site
    ));
    output.push_str(&format!(
        "           postagent manual {} {} {}  # Get full details of action",
        rows[0].site, rows[0].group, rows[0].action
    ));

    output
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

    #[test]
    fn urlencoding_preserves_alphanumeric() {
        assert_eq!(urlencoding("abc123"), "abc123");
        assert_eq!(urlencoding("ABC"), "ABC");
    }

    #[test]
    fn urlencoding_preserves_unreserved_chars() {
        assert_eq!(urlencoding("-_.~"), "-_.~");
    }

    #[test]
    fn urlencoding_encodes_spaces_as_percent20() {
        assert_eq!(urlencoding("hello world"), "hello%20world");
    }

    #[test]
    fn urlencoding_encodes_special_characters() {
        assert_eq!(urlencoding("a+b"), "a%2Bb");
        assert_eq!(urlencoding("a&b=c"), "a%26b%3Dc");
    }

    #[test]
    fn urlencoding_encodes_slash() {
        assert_eq!(urlencoding("path/to/resource"), "path%2Fto%2Fresource");
    }

    #[test]
    fn urlencoding_empty_string() {
        assert_eq!(urlencoding(""), "");
    }

    #[test]
    fn urlencoding_all_spaces() {
        assert_eq!(urlencoding("   "), "%20%20%20");
    }

    #[test]
    fn urlencoding_mixed_content() {
        assert_eq!(urlencoding("search query!"), "search%20query%21");
    }

    #[test]
    fn format_search_empty() {
        let sites: Vec<SearchSite> = vec![];
        assert_eq!(
            format_search_results(&sites, "create page"),
            "No results found."
        );
    }

    #[test]
    fn format_search_basic() {
        let sites = vec![SearchSite {
            name: "notion".into(),
            description: "test".into(),
            groups: vec![
                SearchGroup {
                    name: "pages".into(),
                    actions: vec![SearchAction {
                        name: "create_page".into(),
                        method: "POST".into(),
                        path: "/v1/pages".into(),
                        summary: "Create a page".into(),
                    }],
                },
                SearchGroup {
                    name: "databases".into(),
                    actions: vec![SearchAction {
                        name: "create_database".into(),
                        method: "POST".into(),
                        path: "/v1/databases".into(),
                        summary: "Create a database".into(),
                    }],
                },
            ],
        }];

        let output = format_search_results(&sites, "create page notion");
        assert!(output.contains("  notion\n"));
        assert!(output.contains("    pages\n"));
        assert!(output.contains("    databases\n"));
        assert!(output.contains("create_page"));
        assert!(output.contains("create_database"));
        assert!(output.contains("Run postagent manual <SITE> [GROUP] [ACTION] for full details."));
    }

    #[test]
    fn format_search_multi_site() {
        let sites = vec![
            SearchSite {
                name: "notion".into(),
                description: "".into(),
                groups: vec![SearchGroup {
                    name: "pages".into(),
                    actions: vec![SearchAction {
                        name: "create_page".into(),
                        method: "POST".into(),
                        path: "/v1/pages".into(),
                        summary: "Create a page".into(),
                    }],
                }],
            },
            SearchSite {
                name: "coda".into(),
                description: "".into(),
                groups: vec![SearchGroup {
                    name: "pages".into(),
                    actions: vec![SearchAction {
                        name: "create_page".into(),
                        method: "POST".into(),
                        path: "/docs/{docId}/pages".into(),
                        summary: "Create a page".into(),
                    }],
                }],
            },
        ];

        let output = format_search_results(&sites, "create page");
        assert!(output.contains("  notion\n"));
        assert!(output.contains("  coda\n"));
        assert!(output.contains("actions from 2 sites"));
    }

    #[test]
    fn format_search_dedup() {
        let sites = vec![SearchSite {
            name: "notion".into(),
            description: "".into(),
            groups: vec![SearchGroup {
                name: "pages".into(),
                actions: vec![
                    SearchAction {
                        name: "create_page".into(),
                        method: "POST".into(),
                        path: "/v1/pages".into(),
                        summary: "Create a page".into(),
                    },
                    SearchAction {
                        name: "create_page".into(),
                        method: "POST".into(),
                        path: "/v1/pages".into(),
                        summary: "Create a page".into(),
                    },
                ],
            }],
        }];

        let output = format_search_results(&sites, "create page");
        assert!(output.contains("1 actions from 1 sites"));
    }

    #[test]
    fn format_search_preserves_full_summary() {
        let long_summary = format!("Create a page {}", "with details ".repeat(5));
        let sites = vec![SearchSite {
            name: "test".into(),
            description: "".into(),
            groups: vec![SearchGroup {
                name: "group".into(),
                actions: vec![SearchAction {
                    name: "create_action".into(),
                    method: "GET".into(),
                    path: "/test".into(),
                    summary: long_summary.clone(),
                }],
            }],
        }];

        let output = format_search_results(&sites, "create");
        assert!(output.contains(long_summary.trim()));
    }

    #[test]
    fn score_action_basic() {
        let words = vec!["create".into(), "page".into(), "notion".into()];
        let score = score_action(&words, "notion", "pages", "create_page", "Create a page");
        assert!(score > 0.0);

        // Unrelated action should score lower
        let low_score = score_action(&words, "stripe", "charges", "list_charges", "List charges");
        assert!(score > low_score);
    }
}
