use crate::request_preview::{render_dry_run, HeaderEntry, PreparedRequest};
use crate::token::{referenced_sites, resolve_template_variables};
use reqwest::blocking::Client;
use std::net::IpAddr;
use std::time::Duration;

fn contains_token_template(s: &str) -> bool {
    // Site slot includes `-` so hyphenated slugs (google-drive, share-point) match.
    regex::Regex::new(r"\$POSTAGENT\.[A-Za-z0-9_-]+\.[A-Z_]+")
        .unwrap()
        .is_match(s)
}

fn validated_send_url(raw_url: &str) -> Result<reqwest::Url, String> {
    let url = reqwest::Url::parse(raw_url)
        .map_err(|_| "Invalid URL after template resolution.".to_string())?;

    let is_loopback_http = url.scheme() == "http"
        && url.host_str().is_some_and(|host| {
            let normalized_host = host.trim_start_matches('[').trim_end_matches(']');
            normalized_host.eq_ignore_ascii_case("localhost")
                || normalized_host
                    .parse::<IpAddr>()
                    .map(|ip| ip.is_loopback())
                    .unwrap_or(false)
        });

    if url.scheme() == "https" || is_loopback_http {
        Ok(url)
    } else {
        Err(
            "Refusing to send $POSTAGENT credentials to a non-HTTPS URL. Use https:// or an http://localhost/127.0.0.1/[::1] URL for local testing."
                .to_string(),
        )
    }
}

struct PreSubstitutionInputs {
    url: String,
    headers: Vec<String>,
    body: String,
}

fn prepare(
    raw_url: &str,
    method: Option<&str>,
    headers: &[String],
    data: Option<&str>,
) -> Result<PreparedRequest, String> {
    let url_resolved = resolve_template_variables(raw_url)?;
    let parsed_url = validated_send_url(&url_resolved)?;

    let mut merged: Vec<HeaderEntry> = Vec::new();
    for raw in headers {
        let resolved = resolve_template_variables(raw)?;
        let mut parsed = parse_header(&resolved);
        // Sort JSON-mode multi-header payloads for deterministic ordering.
        parsed.sort_by(|a, b| a.0.to_ascii_lowercase().cmp(&b.0.to_ascii_lowercase()));
        for (k, v) in parsed {
            upsert_header(&mut merged, k, v, false);
        }
    }

    let body = match data {
        Some(d) => Some(resolve_template_variables(d)?),
        None => None,
    };

    let http_method = if let Some(m) = method {
        m.to_uppercase()
    } else if body.is_some() {
        "POST".to_string()
    } else {
        "GET".to_string()
    };

    if !merged
        .iter()
        .any(|h| h.name.eq_ignore_ascii_case("User-Agent"))
    {
        merged.push(HeaderEntry {
            name: "User-Agent".to_string(),
            value: format!("postagent/{}", env!("CARGO_PKG_VERSION")),
            auto_injected: true,
        });
    }

    Ok(PreparedRequest {
        method: http_method,
        url: parsed_url,
        headers: merged,
        body,
    })
}

fn upsert_header(list: &mut Vec<HeaderEntry>, name: String, value: String, auto_injected: bool) {
    if let Some(existing) = list.iter_mut().find(|h| h.name.eq_ignore_ascii_case(&name)) {
        existing.value = value;
        existing.auto_injected = auto_injected;
        return;
    }
    list.push(HeaderEntry {
        name,
        value,
        auto_injected,
    });
}

fn execute(
    prepared: &PreparedRequest,
    pre: &PreSubstitutionInputs,
) -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::builder().timeout(Duration::from_secs(30)).build()?;

    let mut request = match prepared.method.as_str() {
        "GET" => client.get(prepared.url.clone()),
        "POST" => client.post(prepared.url.clone()),
        "PUT" => client.put(prepared.url.clone()),
        "PATCH" => client.patch(prepared.url.clone()),
        "DELETE" => client.delete(prepared.url.clone()),
        "HEAD" => client.head(prepared.url.clone()),
        other => client.request(
            reqwest::Method::from_bytes(other.as_bytes())?,
            prepared.url.clone(),
        ),
    };

    for h in &prepared.headers {
        request = request.header(h.name.as_str(), h.value.as_str());
    }

    if let Some(b) = &prepared.body {
        request = request.body(b.clone());
    }

    let response = match request.send() {
        Ok(resp) => resp,
        Err(e) => {
            if e.is_builder() {
                eprintln!("Invalid URL after template resolution.");
            } else {
                eprintln!("{}", e);
            }
            std::process::exit(1);
        }
    };

    let status = response.status();
    let status_text = status.canonical_reason().unwrap_or("").to_string();
    let response_body = response.text()?;

    if status.is_success() || status.is_informational() || status.is_redirection() {
        print!("{}", response_body);
    } else {
        eprintln!("HTTP {} {}", status.as_u16(), status_text);
        eprint!("{}", response_body);

        if status.as_u16() == 401 || status.as_u16() == 403 {
            eprintln!();
            let header_refs: Vec<&str> = pre.headers.iter().map(|s| s.as_str()).collect();
            let mut inputs: Vec<&str> = vec![pre.url.as_str(), pre.body.as_str()];
            inputs.extend(header_refs);
            let sites = referenced_sites(&inputs);
            if sites.is_empty() {
                eprintln!("Your access token may be expired. Run: postagent auth <site>");
            } else if sites.len() == 1 {
                eprintln!(
                    "Your access token may be expired. Run: postagent auth {}",
                    sites[0]
                );
            } else {
                eprintln!(
                    "Your access token may be expired. Run: postagent auth <site> for one of: {}",
                    sites.join(", ")
                );
            }
        }
        std::process::exit(1);
    }

    Ok(())
}

pub fn run(
    raw_url: &str,
    method: Option<&str>,
    headers: &[String],
    data: Option<&str>,
    dry_run: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let has_token = contains_token_template(raw_url)
        || headers.iter().any(|h| contains_token_template(h))
        || data.is_some_and(contains_token_template);
    if !has_token {
        eprintln!("Missing $POSTAGENT.<SITE>.TOKEN (or .ACCESS_TOKEN / .API_KEY) in URL, headers, or body.");
        eprintln!("Pass the template as a literal string — do not try to fetch the token value separately.\n");
        eprintln!("Example: -H 'Authorization: Bearer $POSTAGENT.GITHUB.TOKEN'");
        std::process::exit(1);
    }

    let prepared = match prepare(raw_url, method, headers, data) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{}", e);
            std::process::exit(1);
        }
    };

    if dry_run {
        print!("{}", render_dry_run(&prepared));
        return Ok(());
    }

    let pre = PreSubstitutionInputs {
        url: raw_url.to_string(),
        headers: headers.to_vec(),
        body: data.unwrap_or("").to_string(),
    };
    execute(&prepared, &pre)
}

fn parse_header(raw: &str) -> Vec<(String, String)> {
    let trimmed = raw.trim();
    if trimmed.starts_with('{') {
        if let Ok(map) = serde_json::from_str::<std::collections::HashMap<String, String>>(trimmed)
        {
            return map.into_iter().collect();
        }
    }
    if let Some(colon_idx) = trimmed.find(':') {
        let key = trimmed[..colon_idx].trim().to_string();
        let value = trimmed[colon_idx + 1..].trim().to_string();
        return vec![(key, value)];
    }
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn find_header<'a>(prepared: &'a PreparedRequest, name: &str) -> Option<&'a HeaderEntry> {
        prepared
            .headers
            .iter()
            .find(|h| h.name.eq_ignore_ascii_case(name))
    }

    #[test]
    fn parse_header_json_format() {
        let input = r#"{"content-type": "application/json"}"#;
        let result = parse_header(input);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, "content-type");
        assert_eq!(result[0].1, "application/json");
    }

    #[test]
    fn parse_header_key_value_format() {
        let input = "Content-Type: application/json";
        let result = parse_header(input);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, "Content-Type");
        assert_eq!(result[0].1, "application/json");
    }

    #[test]
    fn parse_header_key_value_with_extra_whitespace() {
        let input = "  Authorization :  Bearer my-token  ";
        let result = parse_header(input);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, "Authorization");
        assert_eq!(result[0].1, "Bearer my-token");
    }

    #[test]
    fn parse_header_key_value_with_colon_in_value() {
        let input = "Authorization: Bearer abc:def";
        let result = parse_header(input);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].1, "Bearer abc:def");
    }

    #[test]
    fn parse_header_invalid_input_returns_empty() {
        assert!(parse_header("no-colon-here").is_empty());
    }

    #[test]
    fn parse_header_empty_string_returns_empty() {
        assert!(parse_header("").is_empty());
    }

    #[test]
    fn parse_header_invalid_json_falls_back_to_empty() {
        assert!(parse_header("{broken json").is_empty());
    }

    #[test]
    fn prepare_infers_get_without_method_and_body() {
        let prepared = prepare("https://example.com/", None, &[], None).expect("prepare");
        assert_eq!(prepared.method, "GET");
        assert!(prepared.body.is_none());
    }

    #[test]
    fn prepare_infers_post_with_body() {
        let prepared =
            prepare("https://example.com/", None, &[], Some(r#"{"a":1}"#)).expect("prepare");
        assert_eq!(prepared.method, "POST");
        assert_eq!(prepared.body.as_deref(), Some(r#"{"a":1}"#));
    }

    #[test]
    fn prepare_honors_explicit_method() {
        let prepared = prepare("https://example.com/", Some("delete"), &[], None).expect("prepare");
        assert_eq!(prepared.method, "DELETE");
    }

    #[test]
    fn prepare_auto_injects_user_agent() {
        let prepared = prepare("https://example.com/", None, &[], None).expect("prepare");
        let ua = find_header(&prepared, "User-Agent").expect("UA present");
        assert!(ua.auto_injected);
        assert!(ua.value.starts_with("postagent/"));
    }

    #[test]
    fn prepare_respects_user_provided_user_agent() {
        let headers = vec!["User-Agent: my-tool/1.0".to_string()];
        let prepared = prepare("https://example.com/", None, &headers, None).expect("prepare");
        let ua = find_header(&prepared, "User-Agent").expect("UA present");
        assert!(!ua.auto_injected);
        assert_eq!(ua.value, "my-tool/1.0");
    }

    #[test]
    fn prepare_dedupes_duplicate_header_names_case_insensitively() {
        let headers = vec![
            "Content-Type: text/plain".to_string(),
            "content-type: application/json".to_string(),
        ];
        let prepared = prepare("https://example.com/", None, &headers, None).expect("prepare");
        let count = prepared
            .headers
            .iter()
            .filter(|h| h.name.eq_ignore_ascii_case("content-type"))
            .count();
        assert_eq!(count, 1);
        let ct = find_header(&prepared, "Content-Type").expect("CT present");
        assert_eq!(ct.value, "application/json");
    }

    #[test]
    fn prepare_rejects_non_https_remote_url() {
        let err = prepare("http://api.example.com/v1", None, &[], None).unwrap_err();
        assert!(err.contains("non-HTTPS URL"));
    }

    #[test]
    fn prepare_allows_loopback_http() {
        for raw_url in [
            "http://localhost:3000/v1",
            "http://127.0.0.1:3000/v1",
            "http://[::1]:3000/v1",
        ] {
            prepare(raw_url, None, &[], None).expect("loopback should be allowed");
        }
    }

    #[test]
    fn prepare_rejects_invalid_url() {
        let err = prepare("not a url", None, &[], None).unwrap_err();
        assert_eq!(err, "Invalid URL after template resolution.");
    }

    #[test]
    fn contains_token_template_recognizes_new_forms() {
        assert!(contains_token_template("$POSTAGENT.FOO.TOKEN"));
        assert!(contains_token_template("$POSTAGENT.FOO.ACCESS_TOKEN"));
        assert!(contains_token_template("$POSTAGENT.FOO.API_KEY"));
        assert!(contains_token_template("$POSTAGENT.FOO.EXTRAS"));
        assert!(!contains_token_template("no templates here"));
    }

    #[test]
    fn validated_send_url_allows_https() {
        let url = validated_send_url("https://api.example.com/v1").unwrap();
        assert_eq!(url.scheme(), "https");
    }

    #[test]
    fn validated_send_url_allows_loopback_http() {
        for raw_url in [
            "http://localhost:3000/v1",
            "http://127.0.0.1:3000/v1",
            "http://[::1]:3000/v1",
        ] {
            let url = validated_send_url(raw_url).unwrap();
            assert_eq!(url.scheme(), "http");
        }
    }

    #[test]
    fn validated_send_url_rejects_remote_http() {
        let err = validated_send_url("http://api.example.com/v1").unwrap_err();
        assert!(err.contains("non-HTTPS URL"));
        assert!(!err.contains("api.example.com"));
    }

    #[test]
    fn validated_send_url_rejects_invalid_urls() {
        let err = validated_send_url("not a url").unwrap_err();
        assert_eq!(err, "Invalid URL after template resolution.");
    }
}
