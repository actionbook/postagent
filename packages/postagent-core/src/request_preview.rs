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

fn is_sensitive_query_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    if matches!(
        lower.as_str(),
        "key"
            | "token"
            | "access_token"
            | "refresh_token"
            | "id_token"
            | "password"
            | "passwd"
            | "pwd"
            | "secret"
            | "client_secret"
            | "auth"
            | "authorization"
            | "signature"
            | "sig"
            | "credentials"
            | "session"
            | "sessionid"
            | "session_id"
    ) {
        return true;
    }
    lower.contains("token")
        || lower.contains("secret")
        || lower.contains("password")
        || lower.contains("apikey")
        || lower.contains("api_key")
        || lower.contains("api-key")
        || lower.ends_with("_key")
        || lower.ends_with("-key")
}

fn looks_like_secret_segment(seg: &str) -> bool {
    // Path segments that resolved from $POSTAGENT.<SITE>.<FIELD> templates
    // are opaque high-entropy strings (PATs, JWTs, signed keys). Catch them
    // by shape: long, URL-safe, with mixed letter+digit content. Readable
    // path components like "repos", "v1", "actionbook", "19" stay intact.
    if seg.len() < 20 {
        return false;
    }
    let mut has_letter = false;
    let mut has_digit = false;
    for b in seg.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' => has_letter = true,
            b'0'..=b'9' => has_digit = true,
            b'_' | b'-' | b'.' | b'~' => {}
            _ => return false,
        }
    }
    has_letter && has_digit
}

pub fn redact_path(path: &str) -> String {
    // Preserve leading/trailing slashes and empty segments (e.g. "//").
    path.split('/')
        .map(|seg| {
            if looks_like_secret_segment(seg) {
                "***"
            } else {
                seg
            }
        })
        .collect::<Vec<_>>()
        .join("/")
}

pub fn redact_url(url: &reqwest::Url) -> String {
    // Build a preview form; prioritize readability over exact URL roundtrip.
    let mut out = String::new();
    out.push_str(url.scheme());
    out.push_str("://");
    if !url.username().is_empty() {
        out.push_str(url.username());
        if url.password().is_some() {
            out.push_str(":***");
        }
        out.push('@');
    }
    if let Some(host) = url.host_str() {
        if host.contains(':') {
            out.push('[');
            out.push_str(host);
            out.push(']');
        } else {
            out.push_str(host);
        }
    }
    if let Some(port) = url.port() {
        out.push(':');
        out.push_str(&port.to_string());
    }
    out.push_str(&redact_path(url.path()));
    if url.query().is_some() {
        out.push('?');
        let pairs: Vec<String> = url
            .query_pairs()
            .map(|(k, v)| {
                if is_sensitive_query_name(&k) {
                    format!("{}=***", k)
                } else {
                    format!("{}={}", k, v)
                }
            })
            .collect();
        out.push_str(&pairs.join("&"));
    }
    if let Some(frag) = url.fragment() {
        out.push('#');
        out.push_str(frag);
    }
    out
}

pub fn render_dry_run(prepared: &PreparedRequest) -> String {
    let mut out = String::new();
    out.push_str("DRY RUN — request not sent\n\n");
    out.push_str("Method:\n");
    out.push_str(&prepared.method);
    out.push_str("\n\n");
    out.push_str("URL:\n");
    out.push_str(&redact_url(&prepared.url));
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
    fn redact_url_masks_sensitive_query_params() {
        let u = reqwest::Url::parse(
            "https://api.example.com/v1?api_key=sk_live_xyz&user=alice&access_token=AT",
        )
        .unwrap();
        let out = redact_url(&u);
        assert!(out.contains("api_key=***"), "api_key not redacted: {}", out);
        assert!(
            out.contains("access_token=***"),
            "access_token not redacted: {}",
            out
        );
        assert!(out.contains("user=alice"), "benign param lost: {}", out);
    }

    #[test]
    fn redact_url_masks_additional_sensitive_names() {
        // The rule favors false-positive redaction over leaking: any *_key /
        // *-key query name is masked, even benign ones like sort_key. The
        // preview's goal is "no secrets on stdout", not diff fidelity.
        for (name, should_redact) in [
            ("token", true),
            ("id_token", true),
            ("password", true),
            ("sig", true),
            ("client_secret", true),
            ("refresh_token", true),
            ("private_key", true),
            ("account-key", true),
            ("sort_key", true),
            ("cache_key", true),
            ("id", false),
            ("page", false),
            ("sort", false),
            ("limit", false),
        ] {
            let u = reqwest::Url::parse(&format!("https://x/?{}={}", name, "value")).unwrap();
            let out = redact_url(&u);
            let redacted = out.contains(&format!("{}=***", name));
            assert_eq!(
                redacted, should_redact,
                "name={} should_redact={} got={}",
                name, should_redact, out
            );
        }
    }

    #[test]
    fn redact_url_keeps_benign_query_intact() {
        let u = reqwest::Url::parse("https://api.example.com/users?id=1&sort=name").unwrap();
        assert_eq!(
            redact_url(&u),
            "https://api.example.com/users?id=1&sort=name"
        );
    }

    #[test]
    fn redact_url_masks_userinfo_password() {
        let u = reqwest::Url::parse("https://alice:verysecret@api.example.com/v1").unwrap();
        let out = redact_url(&u);
        assert!(out.contains("alice:***@"), "password not redacted: {}", out);
        assert!(!out.contains("verysecret"), "plaintext leaked: {}", out);
    }

    #[test]
    fn redact_url_without_query_or_auth_unchanged() {
        let u = reqwest::Url::parse("https://api.example.com/users/1").unwrap();
        assert_eq!(redact_url(&u), "https://api.example.com/users/1");
    }

    #[test]
    fn redact_url_preserves_port_and_fragment() {
        let u = reqwest::Url::parse("https://api.example.com:8443/v1?api_key=k#anchor").unwrap();
        let out = redact_url(&u);
        assert!(out.starts_with("https://api.example.com:8443/v1?"));
        assert!(out.contains("api_key=***"));
        assert!(out.ends_with("#anchor"));
    }

    #[test]
    fn redact_path_keeps_readable_segments() {
        assert_eq!(
            redact_path("/repos/actionbook/postagent/pulls/19"),
            "/repos/actionbook/postagent/pulls/19"
        );
        assert_eq!(redact_path("/v1/users/42"), "/v1/users/42");
        assert_eq!(redact_path("/"), "/");
        assert_eq!(redact_path(""), "");
    }

    #[test]
    fn redact_path_masks_token_like_segments() {
        // GitHub classic PAT shape
        assert_eq!(
            redact_path("/api/ghp_AbCdEf0123456789xyz0123456789AAAA/items"),
            "/api/***/items"
        );
        // JWT-ish high-entropy base64
        let jwt_path =
            "/verify/eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJhZG1pbiIsImlhdCI6MTYwOTQ1OTIwMH0.abc";
        let out = redact_path(jwt_path);
        assert!(
            out.starts_with("/verify/***"),
            "jwt segment not redacted: {}",
            out
        );
        // UUID
        assert_eq!(
            redact_path("/orgs/550e8400-e29b-41d4-a716-446655440000/keys"),
            "/orgs/***/keys"
        );
    }

    #[test]
    fn redact_path_ignores_pure_numeric_ids() {
        // 20-digit order IDs are masked (len >= 20 + digits). Acceptable
        // overmasking — dry-run favors safety. Short numeric IDs stay.
        assert_eq!(redact_path("/orders/12345"), "/orders/12345");
        assert_eq!(
            redact_path("/orders/1234567890123"),
            "/orders/1234567890123"
        );
    }

    #[test]
    fn redact_path_ignores_long_words() {
        // Pure letters (no digit) are kept even if long.
        assert_eq!(
            redact_path("/repos/extraordinarilylongreponame/file"),
            "/repos/extraordinarilylongreponame/file"
        );
    }

    #[test]
    fn redact_url_masks_token_in_path() {
        let u = reqwest::Url::parse(
            "https://api.example.com/v1/ghp_AbCdEf0123456789xyz0123456789AAAA/items",
        )
        .unwrap();
        let out = redact_url(&u);
        assert!(
            out.contains("/v1/***/items"),
            "path token not redacted: {}",
            out
        );
        assert!(!out.contains("ghp_AbCdEf"), "plaintext leaked: {}", out);
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
