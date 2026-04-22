use std::fmt::Write;

#[derive(Debug)]
pub struct HeaderEntry {
    pub name: String,
    pub value: String,
    pub auto_injected: bool,
}

#[derive(Debug)]
pub struct PreparedRequest {
    pub method: String,
    pub url: reqwest::Url,
    pub headers: Vec<HeaderEntry>,
    pub body: Option<String>,
}

pub fn is_sensitive_header(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    if matches!(
        lower.as_str(),
        "authorization"
            | "cookie"
            | "set-cookie"
            | "proxy-authorization"
            | "x-api-key"
            | "api-key"
            | "apikey"
    ) {
        return true;
    }
    lower.contains("secret")
        || lower.contains("password")
        || lower.ends_with("-token")
        || lower.ends_with("_token")
        || lower.ends_with("-key")
        || lower.ends_with("_key")
        || lower.ends_with("-auth")
}

pub fn redact_header_value(value: &str) -> String {
    if value.is_empty() {
        return String::new();
    }
    if let Some((scheme, rest)) = value.split_once(' ') {
        let lower = scheme.to_ascii_lowercase();
        if matches!(lower.as_str(), "bearer" | "basic" | "digest" | "token")
            && !rest.trim().is_empty()
        {
            return format!("{} ***", scheme);
        }
    }
    "***".to_string()
}

pub fn redact_body(body: &str) -> String {
    if let Ok(mut v) = serde_json::from_str::<serde_json::Value>(body) {
        if let Some(obj) = v.as_object_mut() {
            let mut touched = false;
            for (k, val) in obj.iter_mut() {
                if is_sensitive_body_key(k) {
                    *val = serde_json::Value::String("***".to_string());
                    touched = true;
                }
            }
            if touched {
                return serde_json::to_string(&v).unwrap_or_else(|_| body.to_string());
            }
        }
    }
    body.to_string()
}

fn is_sensitive_body_key(k: &str) -> bool {
    let lower = k.to_ascii_lowercase();
    lower.contains("password")
        || lower.contains("token")
        || lower.contains("secret")
        || lower.contains("credential")
        || lower == "api_key"
        || lower == "api-key"
        || lower == "apikey"
}

pub fn render_dry_run(prepared: &PreparedRequest) -> String {
    let mut out = String::new();
    out.push_str("DRY RUN — request not sent\n\n");
    out.push_str("Method:\n");
    out.push_str(&prepared.method);
    out.push_str("\n\n");
    out.push_str("URL:\n");
    out.push_str(prepared.url.as_str());
    out.push_str("\n\n");
    out.push_str("Headers:\n");
    for h in &prepared.headers {
        let displayed = if is_sensitive_header(&h.name) {
            redact_header_value(&h.value)
        } else {
            h.value.clone()
        };
        if h.auto_injected {
            let _ = writeln!(out, "{}: {}   [auto-injected]", h.name, displayed);
        } else {
            let _ = writeln!(out, "{}: {}", h.name, displayed);
        }
    }
    out.push_str("\nBody:\n");
    match &prepared.body {
        Some(b) => {
            out.push_str(&redact_body(b));
            out.push('\n');
        }
        None => out.push_str("(none)\n"),
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_sensitive_header_covers_explicit_names() {
        for name in [
            "Authorization",
            "authorization",
            "Cookie",
            "Set-Cookie",
            "Proxy-Authorization",
            "x-api-key",
            "X-API-Key",
            "api-key",
            "apikey",
        ] {
            assert!(is_sensitive_header(name), "expected sensitive: {}", name);
        }
    }

    #[test]
    fn is_sensitive_header_covers_pattern_names() {
        for name in [
            "X-Session-Token",
            "My_Secret",
            "Refresh-Token",
            "x-auth",
            "private-key",
        ] {
            assert!(is_sensitive_header(name), "expected sensitive: {}", name);
        }
    }

    #[test]
    fn is_sensitive_header_ignores_benign_names() {
        for name in ["Content-Type", "Accept", "User-Agent", "X-Trace-Id", "Host"] {
            assert!(!is_sensitive_header(name), "expected benign: {}", name);
        }
    }

    #[test]
    fn redact_bearer_preserves_scheme() {
        assert_eq!(redact_header_value("Bearer ghp_xyz"), "Bearer ***");
        assert_eq!(redact_header_value("bearer abc"), "bearer ***");
        assert_eq!(redact_header_value("Basic dXNlcjpwYXNz"), "Basic ***");
    }

    #[test]
    fn redact_opaque_value_fully_masks() {
        assert_eq!(redact_header_value("ak_live_xxxxxxxxxxxx"), "***");
        assert_eq!(redact_header_value("sid=abc; user=xy"), "***");
    }

    #[test]
    fn redact_empty_stays_empty() {
        assert_eq!(redact_header_value(""), "");
    }

    #[test]
    fn redact_bearer_without_token_is_fully_masked() {
        assert_eq!(redact_header_value("Bearer "), "***");
        assert_eq!(redact_header_value("Bearer   "), "***");
    }

    #[test]
    fn redact_body_masks_sensitive_top_level_keys() {
        let body = r#"{"username":"a","password":"p","api_key":"k","note":"hi"}"#;
        let redacted = redact_body(body);
        assert!(redacted.contains(r#""username":"a""#));
        assert!(redacted.contains(r#""password":"***""#));
        assert!(redacted.contains(r#""api_key":"***""#));
        assert!(redacted.contains(r#""note":"hi""#));
    }

    #[test]
    fn redact_body_passes_through_non_json() {
        let body = "plain text body";
        assert_eq!(redact_body(body), "plain text body");
    }

    #[test]
    fn redact_body_passes_through_when_no_sensitive_keys() {
        let body = r#"{"name":"a","value":1}"#;
        assert_eq!(redact_body(body), body);
    }

    #[test]
    fn render_dry_run_has_expected_structure() {
        let prepared = PreparedRequest {
            method: "POST".to_string(),
            url: reqwest::Url::parse("https://api.example.com/users").unwrap(),
            headers: vec![
                HeaderEntry {
                    name: "Content-Type".to_string(),
                    value: "application/json".to_string(),
                    auto_injected: false,
                },
                HeaderEntry {
                    name: "Authorization".to_string(),
                    value: "Bearer abc123".to_string(),
                    auto_injected: false,
                },
                HeaderEntry {
                    name: "User-Agent".to_string(),
                    value: "postagent/0.0.0".to_string(),
                    auto_injected: true,
                },
            ],
            body: Some(r#"{"name":"a"}"#.to_string()),
        };
        let out = render_dry_run(&prepared);
        assert!(out.starts_with("DRY RUN — request not sent\n"));
        assert!(out.contains("Method:\nPOST\n"));
        assert!(out.contains("URL:\nhttps://api.example.com/users\n"));
        assert!(out.contains("Content-Type: application/json\n"));
        assert!(out.contains("Authorization: Bearer ***\n"));
        assert!(out.contains("User-Agent: postagent/0.0.0   [auto-injected]\n"));
        assert!(out.contains("Body:\n{\"name\":\"a\"}\n"));
    }

    #[test]
    fn render_dry_run_without_body_says_none() {
        let prepared = PreparedRequest {
            method: "GET".to_string(),
            url: reqwest::Url::parse("https://example.com/").unwrap(),
            headers: vec![],
            body: None,
        };
        let out = render_dry_run(&prepared);
        assert!(out.contains("Body:\n(none)\n"));
    }
}
