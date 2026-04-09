use crate::token::resolve_template_variables;
use reqwest::blocking::Client;
use std::collections::HashMap;
use std::time::Duration;

fn contains_token_template(s: &str) -> bool {
    regex::Regex::new(r"\$POSTAGENT\.[A-Za-z0-9_]+\.API_KEY")
        .unwrap()
        .is_match(s)
}

pub fn run(
    raw_url: &str,
    method: Option<&str>,
    headers: &[String],
    data: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    // 0. Check for token template
    let has_token = contains_token_template(raw_url)
        || headers.iter().any(|h| contains_token_template(h))
        || data.map_or(false, |d| contains_token_template(d));
    if !has_token {
        eprintln!("Missing $POSTAGENT.<SITE>.API_KEY in headers or body.\n");
        eprintln!("Example: -H 'Authorization: Bearer $POSTAGENT.GITHUB.API_KEY'");
        std::process::exit(1);
    }

    // 1. Template variable substitution
    let url = match resolve_template_variables(raw_url) {
        Ok(u) => u,
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

    // 2. Determine method
    let http_method = if let Some(m) = method {
        m.to_uppercase()
    } else if body.is_some() {
        "POST".to_string()
    } else {
        "GET".to_string()
    };

    // 3. Default User-Agent (user-supplied header takes precedence)
    let ua_key = "User-Agent";
    if !merged_headers.keys().any(|k| k.eq_ignore_ascii_case(ua_key)) {
        merged_headers.insert(
            ua_key.to_string(),
            format!("postagent/{}", env!("CARGO_PKG_VERSION")),
        );
    }

    // 3.5. Inject x-api-key: env POSTAGENT_API_KEY > config apiKey
    if let Ok(api_key) = std::env::var("POSTAGENT_API_KEY") {
        merged_headers.insert("x-api-key".to_string(), api_key);
    } else if let Some(api_key) = super::config::get_value("apiKey") {
        merged_headers.insert("x-api-key".to_string(), api_key);
    }

    // 4. Send request
    let client = Client::builder()
        .timeout(Duration::from_secs(30))
        .build()?;

    let mut request = match http_method.as_str() {
        "GET" => client.get(&url),
        "POST" => client.post(&url),
        "PUT" => client.put(&url),
        "PATCH" => client.patch(&url),
        "DELETE" => client.delete(&url),
        "HEAD" => client.head(&url),
        _ => client.request(reqwest::Method::from_bytes(http_method.as_bytes())?, &url),
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
                eprintln!("Invalid URL: {}", url);
            } else {
                eprintln!("{}", e);
            }
            std::process::exit(1);
        }
    };

    // 5. Handle response
    let status = response.status();
    let status_text = status.canonical_reason().unwrap_or("").to_string();
    let response_body = response.text()?;

    if status.is_success() || status.is_informational() || status.is_redirection() {
        print!("{}", response_body);
    } else {
        eprint!("HTTP {} {}\n", status.as_u16(), status_text);
        eprint!("{}", response_body);
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
        // Value contains colon (e.g., "Bearer abc:def"), only the first colon is the delimiter
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
        // Starts with '{' but is not valid JSON
        let input = "{broken json";
        let result = parse_header(input);
        // Falls through JSON parsing, then tries Key:Value — no colon after key, so empty
        assert!(result.is_empty());
    }

    #[test]
    fn parse_header_invalid_json_with_colon_fallback() {
        // Starts with '{' but invalid JSON, but has a colon so Key:Value fallback works
        let input = "{broken: json}";
        let result = parse_header(input);
        assert_eq!(result.len(), 1);
        assert_eq!(result.get("{broken"), Some(&"json}".to_string()));
    }

    #[test]
    fn method_inference_defaults_to_get_without_body() {
        // Test the method inference logic directly
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
}
