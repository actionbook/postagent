use crate::oauth::refresh::refresh_access_token;
use crate::request_preview::{render_dry_run, HeaderEntry, PreparedRequest};
use crate::token::{
    self, provider_for_site, referenced_sites, resolve_template_variables, AuthKind,
};
use reqwest::blocking::{Client, Response};
use reqwest::header::{HeaderName, HeaderValue};
use std::net::IpAddr;
use std::time::Duration;

fn contains_token_template(s: &str) -> bool {
    // Site slot includes `-` so hyphenated slugs (google-drive, share-point) match.
    regex::Regex::new(r"\$POSTAGENT\.[A-Za-z0-9_-]+\.[A-Z_]+")
        .unwrap()
        .is_match(s)
}

fn is_loopback_host(url: &reqwest::Url) -> bool {
    url.host_str().is_some_and(|host| {
        let normalized = host.trim_start_matches('[').trim_end_matches(']');
        normalized.eq_ignore_ascii_case("localhost")
            || normalized
                .parse::<IpAddr>()
                .map(|ip| ip.is_loopback())
                .unwrap_or(false)
    })
}

fn is_allowed_transport(url: &reqwest::Url) -> bool {
    url.scheme() == "https" || (url.scheme() == "http" && is_loopback_host(url))
}

fn validated_send_url(raw_url: &str) -> Result<reqwest::Url, String> {
    let url = reqwest::Url::parse(raw_url)
        .map_err(|_| "Invalid URL after template resolution.".to_string())?;

    if is_allowed_transport(&url) {
        Ok(url)
    } else {
        Err(
            "Refusing to send $POSTAGENT credentials to a non-HTTPS URL. Use https:// or an http://localhost/127.0.0.1/[::1] URL for local testing."
                .to_string(),
        )
    }
}

/// Captures the original CLI args before template substitution. Two reasons
/// we need them after `prepare()`: (1) to surface site names in the expired-
/// token hint without leaking resolved secrets, and (2) to re-prepare the
/// request after an OAuth auto-refresh so it picks up the new access_token.
struct PreSubstitutionInputs {
    url: String,
    method: Option<String>,
    headers: Vec<String>,
    data: Option<String>,
}

impl PreSubstitutionInputs {
    fn template_inputs(&self) -> Vec<&str> {
        let mut out: Vec<&str> = Vec::with_capacity(2 + self.headers.len());
        out.push(self.url.as_str());
        if let Some(d) = &self.data {
            out.push(d.as_str());
        }
        for h in &self.headers {
            out.push(h.as_str());
        }
        out
    }
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
        reject_token_templates_in_header_names(raw)?;
        let resolved = resolve_template_variables(raw)?;
        let mut parsed = parse_header(&resolved);
        // Sort JSON-mode multi-header payloads for deterministic ordering.
        parsed.sort_by(|a, b| a.0.to_ascii_lowercase().cmp(&b.0.to_ascii_lowercase()));
        for (k, v) in parsed {
            validate_header(&k, &v)?;
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

    if reqwest::Method::from_bytes(http_method.as_bytes()).is_err() {
        return Err(format!(
            "Invalid HTTP method: {:?}. Method must be a valid HTTP token.",
            http_method
        ));
    }

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

fn validate_header(name: &str, value: &str) -> Result<(), String> {
    HeaderName::from_bytes(name.as_bytes())
        .map_err(|_| format!("Invalid HTTP header name: {:?}.", name))?;
    HeaderValue::from_str(value)
        .map_err(|_| format!("Invalid HTTP header value for {:?}.", name))?;
    Ok(())
}

fn reject_token_templates_in_header_names(raw: &str) -> Result<(), String> {
    for (name, _) in parse_header(raw) {
        if contains_token_template(&name) {
            return Err("Header names must not contain $POSTAGENT templates.".to_string());
        }
    }
    Ok(())
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

fn send_request_once(prepared: &PreparedRequest) -> Result<Response, reqwest::Error> {
    // Re-run validated_send_url in this function so the sanitizer is
    // visible to static-analysis taint tracking in the same scope as the
    // reqwest sinks below. prepare() already validated once.
    let safe_url = validated_send_url(prepared.url.as_str())
        .expect("prepare() already validated url; should not fail here");

    let client = Client::builder().timeout(Duration::from_secs(30)).build()?;

    let mut request = match prepared.method.as_str() {
        "GET" => client.get(safe_url.clone()),
        "POST" => client.post(safe_url.clone()),
        "PUT" => client.put(safe_url.clone()),
        "PATCH" => client.patch(safe_url.clone()),
        "DELETE" => client.delete(safe_url.clone()),
        "HEAD" => client.head(safe_url.clone()),
        other => client.request(
            reqwest::Method::from_bytes(other.as_bytes())
                .expect("prepare() validated method; should not fail here"),
            safe_url.clone(),
        ),
    };

    for h in &prepared.headers {
        request = request.header(h.name.as_str(), h.value.as_str());
    }

    if let Some(b) = &prepared.body {
        request = request.body(b.clone());
    }

    request.send()
}

fn execute(
    prepared: &PreparedRequest,
    pre: &PreSubstitutionInputs,
) -> Result<(), Box<dyn std::error::Error>> {
    let response = send_or_exit(send_request_once(prepared));

    // OAuth auto-refresh on 401/403: try refreshing any OAuth-typed credential
    // referenced by the request, then re-issue the request once. We only
    // attempt refresh if at least one referenced credential is OAuth — static
    // tokens have nothing to refresh.
    let response = if matches!(response.status().as_u16(), 401 | 403)
        && try_refresh_referenced_oauth_credentials(pre) > 0
    {
        let reprepared = match prepare(
            &pre.url,
            pre.method.as_deref(),
            &pre.headers,
            pre.data.as_deref(),
        ) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("{}", e);
                std::process::exit(1);
            }
        };
        send_or_exit(send_request_once(&reprepared))
    } else {
        response
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
            let sites = referenced_sites(&pre.template_inputs());
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

fn send_or_exit(result: Result<Response, reqwest::Error>) -> Response {
    match result {
        Ok(resp) => resp,
        Err(e) => {
            if e.is_builder() {
                eprintln!("Invalid URL after template resolution.");
            } else {
                eprintln!("{}", e);
            }
            std::process::exit(1);
        }
    }
}

/// Refresh OAuth access tokens for every referenced site that uses OAuth.
/// Returns the number of credentials successfully refreshed; 0 means caller
/// should not retry. Provider-shared sites are deduped so a rotating
/// refresh_token is spent at most once per provider.
fn try_refresh_referenced_oauth_credentials(pre: &PreSubstitutionInputs) -> usize {
    let sites = referenced_sites(&pre.template_inputs());
    let mut seen_keys: Vec<String> = Vec::new();
    let mut refreshed = 0usize;
    for site in &sites {
        let auth = match token::load_auth(site) {
            Some(a) => a,
            None => continue,
        };
        if auth.effective_kind() != AuthKind::Oauth2 {
            continue;
        }
        // Sites that point at a shared provider all back the same auth.yaml,
        // so dedupe by provider name (or by site when no pointer exists) to
        // avoid burning a rotating refresh_token twice. Only record the key
        // on success — if the first sibling's refresh fails (e.g. descriptor
        // lookup blip or saved method renamed), a later sibling under the
        // same provider still gets a chance to succeed.
        let key = provider_for_site(site).unwrap_or_else(|| site.clone());
        if seen_keys.contains(&key) {
            continue;
        }

        match refresh_access_token(site) {
            Ok(()) => {
                eprintln!("postagent: refreshed OAuth token for {}; retrying", site);
                refreshed += 1;
                seen_keys.push(key);
            }
            Err(e) => {
                eprintln!("postagent: auto-refresh failed for {}: {}", site, e);
            }
        }
    }
    refreshed
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
        method: method.map(String::from),
        headers: headers.to_vec(),
        data: data.map(String::from),
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

    #[test]
    fn prepare_rejects_invalid_http_method() {
        // Dry-run must reject bogus methods at prepare time so users don't
        // see a "successful" preview for a request that cannot actually be
        // sent. reqwest::Method::from_bytes enforces the RFC 7230 token
        // grammar (no spaces, no control chars, etc.).
        for bad in ["BAD METHOD", "", "GE T", "not\tok"] {
            let err = prepare("https://example.com/", Some(bad), &[], None).unwrap_err();
            assert!(
                err.contains("Invalid HTTP method"),
                "expected invalid-method error for {:?}, got: {}",
                bad,
                err
            );
        }
    }

    #[test]
    fn prepare_rejects_invalid_header_name() {
        let headers = vec!["Bad Header: x".to_string()];
        let err = prepare("https://example.com/", None, &headers, None).unwrap_err();
        assert!(
            err.contains("Invalid HTTP header name"),
            "expected invalid-header-name error, got: {}",
            err
        );
    }

    #[test]
    fn prepare_rejects_invalid_header_value() {
        let headers = vec!["X-Test: ok\nbad".to_string()];
        let err = prepare("https://example.com/", None, &headers, None).unwrap_err();
        assert!(
            err.contains("Invalid HTTP header value"),
            "expected invalid-header-value error, got: {}",
            err
        );
    }

    #[test]
    fn prepare_rejects_token_template_in_header_name() {
        let headers = vec!["$POSTAGENT.GITHUB.TOKEN: x".to_string()];
        let err = prepare("https://example.com/", None, &headers, None).unwrap_err();
        assert_eq!(err, "Header names must not contain $POSTAGENT templates.");
    }

    #[test]
    fn prepare_rejects_token_template_in_json_header_name() {
        let headers = vec![r#"{"$POSTAGENT.GITHUB.TOKEN":"x"}"#.to_string()];
        let err = prepare("https://example.com/", None, &headers, None).unwrap_err();
        assert_eq!(err, "Header names must not contain $POSTAGENT templates.");
    }

    #[test]
    fn prepare_accepts_standard_methods() {
        for m in ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] {
            prepare("https://example.com/", Some(m), &[], None)
                .unwrap_or_else(|e| panic!("method {} should be valid: {}", m, e));
        }
    }
}
