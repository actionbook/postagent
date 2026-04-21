use crate::token::{referenced_sites, resolve_template_variables};
use reqwest::blocking::Client;
use std::collections::HashMap;
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

pub fn run(
    raw_url: &str,
    method: Option<&str>,
    headers: &[String],
    data: Option<&str>,
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

    // Capture the pre-resolution inputs so we can still report which sites
    // were referenced after substitution replaces the templates with tokens.
    let headers_joined: Vec<String> = headers.to_vec();
    let body_snap = data.unwrap_or("").to_string();
    let url_snap = raw_url.to_string();

    let url = match resolve_template_variables(raw_url) {
        Ok(u) => u,
        Err(e) => {
            eprintln!("{}", e);
            std::process::exit(1);
        }
    };
    let parsed_url = match validated_send_url(&url) {
        Ok(url) => url,
        Err(e) => {
            eprintln!("{}", e);
            std::process::exit(1);
        }
    };

    let mut merged_headers: HashMap<String, String> = HashMap::new();
    for raw in headers {
        let resolved = match resolve_template_variables(raw) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("{}", e);
                std::process::exit(1);
            }
        };
        for (k, v) in parse_header(&resolved) {
            merged_headers.insert(k, v);
        }
    }

    let body = match data {
        Some(d) => match resolve_template_variables(d) {
            Ok(r) => Some(r),
            Err(e) => {
                eprintln!("{}", e);
                std::process::exit(1);
            }
        },
        None => None,
    };

    let http_method = if let Some(m) = method {
        m.to_uppercase()
    } else if body.is_some() {
        "POST".to_string()
    } else {
        "GET".to_string()
    };

    let ua_key = "User-Agent";
    if !merged_headers
        .keys()
        .any(|k| k.eq_ignore_ascii_case(ua_key))
    {
        merged_headers.insert(
            ua_key.to_string(),
            format!("postagent/{}", env!("CARGO_PKG_VERSION")),
        );
    }

    let client = Client::builder().timeout(Duration::from_secs(30)).build()?;

    let mut request = match http_method.as_str() {
        "GET" => client.get(parsed_url.clone()),
        "POST" => client.post(parsed_url.clone()),
        "PUT" => client.put(parsed_url.clone()),
        "PATCH" => client.patch(parsed_url.clone()),
        "DELETE" => client.delete(parsed_url.clone()),
        "HEAD" => client.head(parsed_url.clone()),
        _ => client.request(
            reqwest::Method::from_bytes(http_method.as_bytes())?,
            parsed_url.clone(),
        ),
    };

    for (key, value) in &merged_headers {
        request = request.header(key.as_str(), value.as_str());
    }

    if let Some(b) = &body {
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
            let header_refs: Vec<&str> = headers_joined.iter().map(|s| s.as_str()).collect();
            let mut inputs: Vec<&str> = vec![url_snap.as_str(), body_snap.as_str()];
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

fn parse_header(raw: &str) -> HashMap<String, String> {
    let trimmed = raw.trim();
    if trimmed.starts_with('{') {
        if let Ok(map) = serde_json::from_str::<HashMap<String, String>>(trimmed) {
            return map;
        }
    }
    let mut result = HashMap::new();
    if let Some(colon_idx) = trimmed.find(':') {
        let key = trimmed[..colon_idx].trim().to_string();
        let value = trimmed[colon_idx + 1..].trim().to_string();
        result.insert(key, value);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_header_json_format() {
        let input = r#"{"content-type": "application/json"}"#;
        let result = parse_header(input);
        assert_eq!(result.len(), 1);
        assert_eq!(
            result.get("content-type"),
            Some(&"application/json".to_string())
        );
    }

    #[test]
    fn parse_header_json_multiple_keys() {
        let input = r#"{"Authorization": "Bearer token", "Accept": "text/html"}"#;
        let result = parse_header(input);
        assert_eq!(result.len(), 2);
        assert_eq!(
            result.get("Authorization"),
            Some(&"Bearer token".to_string())
        );
        assert_eq!(result.get("Accept"), Some(&"text/html".to_string()));
    }

    #[test]
    fn parse_header_key_value_format() {
        let input = "Content-Type: application/json";
        let result = parse_header(input);
        assert_eq!(result.len(), 1);
        assert_eq!(
            result.get("Content-Type"),
            Some(&"application/json".to_string())
        );
    }

    #[test]
    fn parse_header_key_value_with_extra_whitespace() {
        let input = "  Authorization :  Bearer my-token  ";
        let result = parse_header(input);
        assert_eq!(result.len(), 1);
        assert_eq!(
            result.get("Authorization"),
            Some(&"Bearer my-token".to_string())
        );
    }

    #[test]
    fn parse_header_key_value_with_colon_in_value() {
        let input = "Authorization: Bearer abc:def";
        let result = parse_header(input);
        assert_eq!(result.len(), 1);
        assert_eq!(
            result.get("Authorization"),
            Some(&"Bearer abc:def".to_string())
        );
    }

    #[test]
    fn parse_header_invalid_input_returns_empty() {
        let result = parse_header("no-colon-here");
        assert!(result.is_empty());
    }

    #[test]
    fn parse_header_empty_string_returns_empty() {
        let result = parse_header("");
        assert!(result.is_empty());
    }

    #[test]
    fn parse_header_invalid_json_falls_back_to_key_value() {
        let input = "{broken json";
        let result = parse_header(input);
        assert!(result.is_empty());
    }

    #[test]
    fn parse_header_invalid_json_with_colon_fallback() {
        let input = "{broken: json}";
        let result = parse_header(input);
        assert_eq!(result.len(), 1);
        assert_eq!(result.get("{broken"), Some(&"json}".to_string()));
    }

    #[test]
    fn method_inference_defaults_to_get_without_body() {
        let method: Option<&str> = None;
        let body: Option<&str> = None;
        let http_method = if let Some(m) = method {
            m.to_uppercase()
        } else if body.is_some() {
            "POST".to_string()
        } else {
            "GET".to_string()
        };
        assert_eq!(http_method, "GET");
    }

    #[test]
    fn method_inference_defaults_to_post_with_body() {
        let method: Option<&str> = None;
        let body: Option<&str> = Some(r#"{"key":"value"}"#);
        let http_method = if let Some(m) = method {
            m.to_uppercase()
        } else if body.is_some() {
            "POST".to_string()
        } else {
            "GET".to_string()
        };
        assert_eq!(http_method, "POST");
    }

    #[test]
    fn method_inference_explicit_method_overrides() {
        let method: Option<&str> = Some("put");
        let body: Option<&str> = Some(r#"{"key":"value"}"#);
        let http_method = if let Some(m) = method {
            m.to_uppercase()
        } else if body.is_some() {
            "POST".to_string()
        } else {
            "GET".to_string()
        };
        assert_eq!(http_method, "PUT");
    }

    #[test]
    fn method_inference_explicit_delete_without_body() {
        let method: Option<&str> = Some("DELETE");
        let body: Option<&str> = None;
        let http_method = if let Some(m) = method {
            m.to_uppercase()
        } else if body.is_some() {
            "POST".to_string()
        } else {
            "GET".to_string()
        };
        assert_eq!(http_method, "DELETE");
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
