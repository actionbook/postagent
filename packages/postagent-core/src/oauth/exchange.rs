use crate::descriptor::{OAuth2AuthMethod, ResponseMap};
use base64::engine::general_purpose::STANDARD as B64_STD;
use base64::Engine;
use reqwest::blocking::Client;
use serde_json::Value;
use std::collections::BTreeMap;
use std::net::IpAddr;
use std::time::Duration;

#[derive(Debug, Clone, Default)]
pub struct TokenResponse {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_in: Option<i64>,
    pub scope: Option<String>,
    pub token_type: Option<String>,
    pub extras: BTreeMap<String, String>,
}

pub struct ExchangeInputs<'a> {
    pub method: &'a OAuth2AuthMethod,
    pub client_id: &'a str,
    pub client_secret: Option<&'a str>,
    pub code: &'a str,
    pub code_verifier: &'a str,
    pub redirect_uri: &'a str,
}

/// Performs the authorization_code → token exchange per descriptor. Returns
/// a populated `TokenResponse` with fields extracted via RFC 6901 pointers
/// from `response_map`.
pub fn exchange(inputs: ExchangeInputs<'_>) -> Result<TokenResponse, String> {
    let grant_params: Vec<(String, String)> = vec![
        ("grant_type".into(), "authorization_code".into()),
        ("code".into(), inputs.code.into()),
        ("redirect_uri".into(), inputs.redirect_uri.into()),
        ("code_verifier".into(), inputs.code_verifier.into()),
    ];
    post_token_request(
        inputs.method,
        inputs.client_id,
        inputs.client_secret,
        grant_params,
    )
}

/// Shared OAuth token-endpoint POST. Both authorization_code exchange and
/// refresh_token refresh share the same descriptor mechanics (body_encoding,
/// client_auth, response_map); callers supply only the grant-specific
/// parameters and this helper attaches client credentials per `client_auth`.
pub(crate) fn post_token_request(
    method: &OAuth2AuthMethod,
    client_id: &str,
    client_secret: Option<&str>,
    grant_specific_params: Vec<(String, String)>,
) -> Result<TokenResponse, String> {
    // Refuse to leak refresh_token / client_secret to a cleartext endpoint.
    // Validate the URL BEFORE we compose any sensitive payload so a misrouted
    // descriptor fails closed. Loopback http is allowed for the unit tests'
    // mock servers.
    let safe_url = validated_token_url(&method.token.url)?;

    let body_encoding = method.token.body_encoding.as_str();
    let client_auth = method.token.client_auth.as_str();

    let use_basic = match client_auth {
        "basic" => true,
        "body" | "either" => false,
        other => return Err(format!("unsupported client_auth: {}", other)),
    };

    let mut params = grant_specific_params;
    if !use_basic {
        params.push(("client_id".into(), client_id.into()));
        if let Some(s) = client_secret {
            params.push(("client_secret".into(), s.into()));
        }
    }

    let client = Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| format!("failed to build HTTP client: {}", e))?;

    let mut req = client.post(safe_url);

    if use_basic {
        let secret = client_secret.unwrap_or("");
        let encoded = B64_STD.encode(format!("{}:{}", client_id, secret));
        req = req.header("Authorization", format!("Basic {}", encoded));
    }

    req = req.header("Accept", "application/json");
    if let Some(extra) = &method.token.extra_headers {
        for (k, v) in extra {
            req = req.header(k.as_str(), v.as_str());
        }
    }

    let req = match body_encoding {
        "form" => req.form(&params),
        "json" => {
            let map: serde_json::Map<String, Value> = params
                .into_iter()
                .map(|(k, v)| (k, Value::String(v)))
                .collect();
            req.json(&Value::Object(map))
        }
        other => return Err(format!("unsupported body_encoding: {}", other)),
    };

    let resp = req
        .send()
        .map_err(|e| format!("token request failed: {}", e))?;
    let status = resp.status();
    let text = resp.text().unwrap_or_default();

    if !status.is_success() {
        return Err(format!(
            "token endpoint returned HTTP {}: {}",
            status.as_u16(),
            text
        ));
    }

    parse_token_response(&text, &method.token.response_map)
}

/// Reject token endpoints that would transmit OAuth credentials (refresh_token,
/// client_secret, authorization_code) over cleartext. Mirrors the loopback
/// allowance in `commands/send.rs::validated_send_url` so unit tests can run a
/// mock server on `http://127.0.0.1:NNNN/token`.
fn validated_token_url(raw_url: &str) -> Result<reqwest::Url, String> {
    let url = reqwest::Url::parse(raw_url)
        .map_err(|_| format!("invalid OAuth token endpoint URL: {}", raw_url))?;

    if url.scheme() == "https" {
        return Ok(url);
    }
    if url.scheme() == "http" {
        if let Some(host) = url.host_str() {
            let normalized = host.trim_start_matches('[').trim_end_matches(']');
            let is_loopback = normalized.eq_ignore_ascii_case("localhost")
                || normalized
                    .parse::<IpAddr>()
                    .map(|ip| ip.is_loopback())
                    .unwrap_or(false);
            if is_loopback {
                return Ok(url);
            }
        }
    }

    Err(format!(
        "Refusing to POST OAuth credentials to a non-HTTPS token endpoint: {}",
        raw_url
    ))
}

fn parse_token_response(text: &str, rm: &ResponseMap) -> Result<TokenResponse, String> {
    let value: Value = serde_json::from_str(text)
        .map_err(|e| format!("token response is not JSON: {} ({})", e, text))?;

    let access_token = pick_string(&value, &rm.access_token).ok_or_else(|| {
        format!(
            "access_token missing at pointer {} in response",
            rm.access_token
        )
    })?;

    let refresh_token = rm
        .refresh_token
        .as_deref()
        .and_then(|p| pick_string(&value, p));
    let expires_in = rm.expires_in.as_deref().and_then(|p| pick_i64(&value, p));
    let scope = rm.scope.as_deref().and_then(|p| pick_string(&value, p));
    let token_type = rm
        .token_type
        .as_deref()
        .and_then(|p| pick_string(&value, p));

    let mut extras: BTreeMap<String, String> = BTreeMap::new();
    if let Some(map) = &rm.extras {
        for (name, pointer) in map {
            if let Some(v) = pick_string(&value, pointer) {
                extras.insert(name.to_lowercase(), v);
            }
        }
    }

    Ok(TokenResponse {
        access_token,
        refresh_token,
        expires_in,
        scope,
        token_type,
        extras,
    })
}

fn pick_string(v: &Value, pointer: &str) -> Option<String> {
    let got = v.pointer(pointer)?;
    match got {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

fn pick_i64(v: &Value, pointer: &str) -> Option<i64> {
    let got = v.pointer(pointer)?;
    match got {
        Value::Number(n) => n.as_i64(),
        Value::String(s) => s.parse().ok(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::descriptor::{
        AuthorizeSpec, ClientSpec, InjectSpec, OAuth2AuthMethod, RefreshSpec, ResponseMap,
        ScopesSpec, TokenSpec,
    };
    use std::io::{Read, Write};
    use std::net::TcpListener;

    #[test]
    fn validated_token_url_accepts_https() {
        assert!(validated_token_url("https://accounts.google.com/o/oauth2/token").is_ok());
    }

    #[test]
    fn validated_token_url_accepts_loopback_http() {
        for url in [
            "http://localhost:9876/token",
            "http://127.0.0.1:9876/token",
            "http://[::1]:9876/token",
        ] {
            assert!(
                validated_token_url(url).is_ok(),
                "{} should be allowed",
                url
            );
        }
    }

    #[test]
    fn validated_token_url_rejects_remote_http() {
        let err = validated_token_url("http://accounts.example.com/token").unwrap_err();
        assert!(err.contains("non-HTTPS token endpoint"));
    }

    #[test]
    fn validated_token_url_rejects_unparseable() {
        let err = validated_token_url("not a url").unwrap_err();
        assert!(err.contains("invalid OAuth token endpoint URL"));
    }

    #[test]
    fn post_token_request_refuses_cleartext_remote_endpoint() {
        let mut method = make_method("https://placeholder/", "form", "body");
        method.token.url = "http://accounts.example.com/token".into();
        let err = post_token_request(
            &method,
            "cid",
            Some("csec"),
            vec![("grant_type".into(), "refresh_token".into())],
        )
        .unwrap_err();
        assert!(err.contains("non-HTTPS token endpoint"));
    }

    fn make_method(token_url: &str, body_encoding: &str, client_auth: &str) -> OAuth2AuthMethod {
        OAuth2AuthMethod {
            id: "oauth".into(),
            label: "OAuth".into(),
            setup_url: None,
            setup_instructions: None,
            provider: None,
            grants: vec!["authorization_code".into()],
            client: ClientSpec {
                client_type: "confidential".into(),
            },
            authorize: AuthorizeSpec {
                url: "https://example.com/auth".into(),
                extra_params: None,
                params_required: None,
            },
            token: TokenSpec {
                url: token_url.into(),
                body_encoding: body_encoding.into(),
                client_auth: client_auth.into(),
                extra_headers: None,
                response_map: ResponseMap {
                    access_token: "/access_token".into(),
                    refresh_token: Some("/refresh_token".into()),
                    expires_in: Some("/expires_in".into()),
                    scope: None,
                    token_type: Some("/token_type".into()),
                    extras: Some({
                        let mut m = BTreeMap::new();
                        m.insert("bot_id".into(), "/bot_id".into());
                        m
                    }),
                },
            },
            scopes: ScopesSpec {
                default: vec![],
                separator: " ".into(),
                buckets: None,
                refresh_magic_scope: None,
                catalog: None,
            },
            refresh: RefreshSpec {
                behavior: "reusable".into(),
                expiry_instructions: None,
            },
            injects: vec![InjectSpec {
                location: "header".into(),
                name: "Authorization".into(),
                value_template: "Bearer {{access_token}}".into(),
            }],
        }
    }

    struct CapturedRequest {
        headers: String,
        body: String,
    }

    /// Spawn a one-shot HTTP server that accepts a single POST, replies with
    /// a canned JSON token response, and returns the captured request.
    fn spawn_mock_server(
        response_json: &'static str,
    ) -> (String, std::sync::mpsc::Receiver<CapturedRequest>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/token", listener.local_addr().unwrap());
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 4096];
                let mut all = Vec::new();
                let mut headers_done = false;
                let mut content_length = 0usize;
                let mut headers_end = 0usize;

                loop {
                    let n = match stream.read(&mut buf) {
                        Ok(n) if n > 0 => n,
                        _ => break,
                    };
                    all.extend_from_slice(&buf[..n]);

                    if !headers_done {
                        if let Some(idx) = find_subseq(&all, b"\r\n\r\n") {
                            headers_done = true;
                            headers_end = idx + 4;
                            let headers_bytes = &all[..idx];
                            let headers_text = String::from_utf8_lossy(headers_bytes).into_owned();
                            for line in headers_text.lines() {
                                if let Some(rest) = line
                                    .strip_prefix("Content-Length:")
                                    .or_else(|| line.strip_prefix("content-length:"))
                                {
                                    content_length = rest.trim().parse().unwrap_or(0);
                                }
                            }
                        }
                    }

                    if headers_done && all.len() >= headers_end + content_length {
                        break;
                    }
                }

                let headers =
                    String::from_utf8_lossy(&all[..headers_end.saturating_sub(4)]).to_string();
                let body = String::from_utf8_lossy(&all[headers_end..headers_end + content_length])
                    .to_string();
                tx.send(CapturedRequest { headers, body }).ok();

                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                    response_json.len(),
                    response_json
                );
                let _ = stream.write_all(resp.as_bytes());
            }
        });
        (url, rx)
    }

    fn find_subseq(hay: &[u8], needle: &[u8]) -> Option<usize> {
        hay.windows(needle.len()).position(|w| w == needle)
    }

    #[test]
    fn form_encoding_with_body_auth() {
        let (url, rx) = spawn_mock_server(
            r#"{"access_token":"at_xyz","refresh_token":"rt","expires_in":3600,"token_type":"bearer","bot_id":"b1"}"#,
        );
        let method = make_method(&url, "form", "body");
        let tokens = exchange(ExchangeInputs {
            method: &method,
            client_id: "cid",
            client_secret: Some("csec"),
            code: "auth_code",
            code_verifier: "v",
            redirect_uri: "http://127.0.0.1:9876/callback",
        })
        .unwrap();

        let cap = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(cap.headers.contains("application/x-www-form-urlencoded"));
        assert!(!cap.headers.to_lowercase().contains("authorization: basic"));
        assert!(cap.body.contains("client_id=cid"));
        assert!(cap.body.contains("client_secret=csec"));
        assert!(cap.body.contains("grant_type=authorization_code"));
        assert!(cap.body.contains("code_verifier=v"));

        assert_eq!(tokens.access_token, "at_xyz");
        assert_eq!(tokens.refresh_token.as_deref(), Some("rt"));
        assert_eq!(tokens.expires_in, Some(3600));
        assert_eq!(tokens.token_type.as_deref(), Some("bearer"));
        assert_eq!(tokens.extras.get("bot_id").map(|s| s.as_str()), Some("b1"));
    }

    #[test]
    fn json_encoding_with_basic_auth() {
        let (url, rx) = spawn_mock_server(r#"{"access_token":"nj_at","token_type":"bearer"}"#);
        let method = make_method(&url, "json", "basic");
        let tokens = exchange(ExchangeInputs {
            method: &method,
            client_id: "cid",
            client_secret: Some("csec"),
            code: "ac",
            code_verifier: "v",
            redirect_uri: "http://127.0.0.1:9876/callback",
        })
        .unwrap();

        let cap = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(cap.headers.to_lowercase().contains("authorization: basic "));
        assert!(cap.headers.contains("application/json"));
        // body must NOT include client_id/client_secret under basic auth.
        assert!(!cap.body.contains("\"client_id\""));
        assert!(!cap.body.contains("\"client_secret\""));
        assert!(cap.body.contains("\"grant_type\":\"authorization_code\""));

        assert_eq!(tokens.access_token, "nj_at");
    }

    #[test]
    fn either_with_secret_puts_creds_in_body() {
        let (url, rx) = spawn_mock_server(r#"{"access_token":"at"}"#);
        let method = make_method(&url, "form", "either");
        let _ = exchange(ExchangeInputs {
            method: &method,
            client_id: "cid",
            client_secret: Some("sec"),
            code: "c",
            code_verifier: "v",
            redirect_uri: "http://127.0.0.1:9876/callback",
        })
        .unwrap();
        let cap = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(!cap.headers.to_lowercase().contains("authorization: basic"));
        assert!(cap.body.contains("client_id=cid"));
        assert!(cap.body.contains("client_secret=sec"));
    }

    #[test]
    fn either_without_secret_sends_public_body() {
        let (url, rx) = spawn_mock_server(r#"{"access_token":"at"}"#);
        let method = make_method(&url, "form", "either");
        let _ = exchange(ExchangeInputs {
            method: &method,
            client_id: "cid",
            client_secret: None,
            code: "c",
            code_verifier: "v",
            redirect_uri: "http://127.0.0.1:9876/callback",
        })
        .unwrap();
        let cap = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(!cap.headers.to_lowercase().contains("authorization: basic"));
        assert!(cap.body.contains("client_id=cid"));
        assert!(!cap.body.contains("client_secret"));
    }

    #[test]
    fn token_error_returns_descriptive_error() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/token", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 1024];
                let _ = stream.read(&mut buf);
                let body = r#"{"error":"invalid_grant"}"#;
                let resp = format!(
                    "HTTP/1.1 400 Bad Request\r\nContent-Length: {}\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(resp.as_bytes());
            }
        });
        let method = make_method(&url, "form", "body");
        let err = exchange(ExchangeInputs {
            method: &method,
            client_id: "c",
            client_secret: Some("s"),
            code: "x",
            code_verifier: "v",
            redirect_uri: "http://127.0.0.1:9876/callback",
        })
        .unwrap_err();
        assert!(err.contains("400"));
        assert!(err.contains("invalid_grant"));
    }
}
