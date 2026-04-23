use crate::descriptor::{AuthMethod, OAuth2AuthMethod};
use crate::oauth::exchange::{post_token_request, TokenResponse};
use crate::token::{self, AuthFile, AuthKind};
use std::path::Path;

/// Try to refresh the OAuth access token for `site`. On success, the updated
/// auth.yaml is written to disk so subsequent `load_auth(site)` calls see the
/// new access_token.
///
/// The descriptor is re-fetched from the postagent server each time; we never
/// cache token URL / response_map locally, so descriptor edits propagate
/// without a manual re-auth.
pub fn refresh_access_token(site: &str) -> Result<(), Box<dyn std::error::Error>> {
    let base = dirs::home_dir().ok_or("Cannot determine home directory")?;
    refresh_access_token_at(&base, site, fetch_site_descriptor)
}

fn refresh_access_token_at(
    base: &Path,
    site: &str,
    fetch_methods: impl FnOnce(&str) -> Result<Vec<AuthMethod>, Box<dyn std::error::Error>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let auth = token::load_auth_from(base, site)
        .ok_or_else(|| format!("no saved auth for \"{}\"", site))?;

    if auth.effective_kind() != AuthKind::Oauth2 {
        return Err(format!("{}: not an OAuth credential; cannot refresh", site).into());
    }

    let refresh_token = auth.refresh_token.clone().ok_or_else(|| {
        format!(
            "{}: no refresh_token saved; re-run `postagent auth {}`",
            site, site
        )
    })?;

    let app = token::load_app_from(base, site).ok_or_else(|| {
        format!(
            "{}: missing app credentials; re-run `postagent auth {}`",
            site, site
        )
    })?;

    let methods = fetch_methods(site)?;
    let saved_method_id = auth.effective_method_id().to_string();
    let method = pick_oauth_method(&methods, &saved_method_id).ok_or_else(|| {
        format!(
            "{}: descriptor has no OAuth method matching saved id \"{}\"",
            site, saved_method_id
        )
    })?;

    let grant_params = vec![
        ("grant_type".to_string(), "refresh_token".to_string()),
        ("refresh_token".to_string(), refresh_token.clone()),
    ];

    let new_tokens = post_token_request(
        method,
        &app.client_id,
        app.client_secret.as_deref(),
        grant_params,
    )?;

    let merged = merge_refresh_into_auth(&auth, method, new_tokens, &refresh_token);
    token::save_auth_to(base, site, &merged)?;
    Ok(())
}

fn fetch_site_descriptor(site: &str) -> Result<Vec<AuthMethod>, Box<dyn std::error::Error>> {
    Ok(crate::commands::manual::fetch_site_auth_methods(site)?.unwrap_or_default())
}

/// Pick the OAuth method that matches the saved `method_id`, falling back to
/// the first OAuth method if none match. (Saved id can drift if the server
/// renames a method; falling back keeps refresh working when there's only one
/// option anyway.)
fn pick_oauth_method<'a>(
    methods: &'a [AuthMethod],
    saved_method_id: &str,
) -> Option<&'a OAuth2AuthMethod> {
    let mut by_id: Option<&OAuth2AuthMethod> = None;
    let mut first_oauth: Option<&OAuth2AuthMethod> = None;
    for m in methods {
        if let AuthMethod::Oauth2(o) = m {
            if first_oauth.is_none() {
                first_oauth = Some(o);
            }
            if o.id == saved_method_id {
                by_id = Some(o);
                break;
            }
        }
    }
    by_id.or(first_oauth)
}

fn merge_refresh_into_auth(
    prev: &AuthFile,
    method: &OAuth2AuthMethod,
    new_tokens: TokenResponse,
    used_refresh_token: &str,
) -> AuthFile {
    let now = chrono::Utc::now();
    AuthFile {
        kind: Some(AuthKind::Oauth2),
        method_id: Some(method.id.clone()),
        api_key: prev.api_key.clone(),
        // RFC 6749 §6: refresh_token rotation is optional. If the server
        // omits a new one, keep using the one we just spent.
        refresh_token: new_tokens
            .refresh_token
            .or_else(|| Some(used_refresh_token.to_string())),
        access_token: Some(new_tokens.access_token),
        expires_at: new_tokens
            .expires_in
            .map(|s| now + chrono::Duration::seconds(s)),
        token_type: new_tokens.token_type.or_else(|| prev.token_type.clone()),
        scope: new_tokens.scope.or_else(|| prev.scope.clone()),
        obtained_at: Some(now),
        // Refresh responses don't always echo the same extras as the initial
        // exchange — preserve previously-stored extras and overlay any newly
        // returned ones so the union wins.
        extras: {
            let mut merged = prev.extras.clone();
            for (k, v) in new_tokens.extras {
                merged.insert(k, v);
            }
            merged
        },
        extra_fields: prev.extra_fields.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::descriptor::{
        AuthorizeSpec, ClientSpec, InjectSpec, OAuth2AuthMethod, RefreshSpec, ResponseMap,
        ScopesSpec, TokenSpec,
    };
    use crate::token::AppConfig;
    use std::collections::BTreeMap;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::time::Duration;
    use tempfile::TempDir;

    fn make_method(token_url: &str, body_encoding: &str, client_auth: &str) -> OAuth2AuthMethod {
        OAuth2AuthMethod {
            id: "oauth".into(),
            label: "OAuth".into(),
            setup_url: None,
            setup_instructions: None,
            provider: None,
            grants: vec!["authorization_code".into(), "refresh_token".into()],
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
                    extras: None,
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
                behavior: "rotating".into(),
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
        body: String,
    }

    /// One-shot HTTP server that captures the next POST and returns the canned
    /// JSON body. Mirrors the helper in `oauth/exchange.rs` tests.
    fn spawn_mock_server(
        response_body: &'static str,
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
                        if let Some(idx) = all.windows(4).position(|w| w == b"\r\n\r\n") {
                            headers_done = true;
                            headers_end = idx + 4;
                            let headers_text = String::from_utf8_lossy(&all[..idx]).into_owned();
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
                let body = String::from_utf8_lossy(&all[headers_end..headers_end + content_length])
                    .to_string();
                let _ = tx.send(CapturedRequest { body });
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                    response_body.len(),
                    response_body
                );
                let _ = stream.write_all(resp.as_bytes());
            }
        });
        (url, rx)
    }

    fn spawn_failing_server(status: u16, body: &'static str) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/token", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 1024];
                let _ = stream.read(&mut buf);
                let resp = format!(
                    "HTTP/1.1 {} ERR\r\nContent-Length: {}\r\n\r\n{}",
                    status,
                    body.len(),
                    body
                );
                let _ = stream.write_all(resp.as_bytes());
            }
        });
        url
    }

    fn seed_oauth_state(
        base: &Path,
        site: &str,
        method: &OAuth2AuthMethod,
        access_token: &str,
        refresh_token: Option<&str>,
    ) {
        let auth = AuthFile {
            kind: Some(AuthKind::Oauth2),
            method_id: Some(method.id.clone()),
            access_token: Some(access_token.into()),
            refresh_token: refresh_token.map(String::from),
            ..Default::default()
        };
        token::save_auth_to(base, site, &auth).unwrap();

        let app = AppConfig {
            method_id: method.id.clone(),
            client_id: "cid".into(),
            client_secret: Some("csec".into()),
            descriptor_hash: "hhhhhhhhhhhhhhhh".into(),
        };
        token::save_app_to(base, site, &app).unwrap();
    }

    #[test]
    fn refresh_writes_new_access_token_and_keeps_old_refresh_when_not_rotated() {
        let tmp = TempDir::new().unwrap();
        let (url, rx) = spawn_mock_server(
            r#"{"access_token":"new_at","expires_in":3600,"token_type":"bearer"}"#,
        );
        let method = make_method(&url, "form", "body");
        seed_oauth_state(tmp.path(), "foo", &method, "old_at", Some("old_rt"));

        let methods = vec![AuthMethod::Oauth2(method.clone())];
        refresh_access_token_at(tmp.path(), "foo", |_| Ok(methods.clone())).unwrap();

        let cap = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(cap.body.contains("grant_type=refresh_token"));
        assert!(cap.body.contains("refresh_token=old_rt"));
        assert!(cap.body.contains("client_id=cid"));
        assert!(cap.body.contains("client_secret=csec"));

        let after = token::load_auth_from(tmp.path(), "foo").unwrap();
        assert_eq!(after.access_token.as_deref(), Some("new_at"));
        // Server didn't issue a new refresh_token → keep the one we sent.
        assert_eq!(after.refresh_token.as_deref(), Some("old_rt"));
        assert!(after.expires_at.is_some());
        assert!(after.obtained_at.is_some());
    }

    #[test]
    fn refresh_rotates_refresh_token_when_server_returns_one() {
        let tmp = TempDir::new().unwrap();
        let (url, _rx) = spawn_mock_server(
            r#"{"access_token":"new_at","refresh_token":"new_rt","expires_in":3600}"#,
        );
        let method = make_method(&url, "form", "body");
        seed_oauth_state(tmp.path(), "foo", &method, "old_at", Some("old_rt"));

        let methods = vec![AuthMethod::Oauth2(method.clone())];
        refresh_access_token_at(tmp.path(), "foo", |_| Ok(methods.clone())).unwrap();

        let after = token::load_auth_from(tmp.path(), "foo").unwrap();
        assert_eq!(after.refresh_token.as_deref(), Some("new_rt"));
        assert_eq!(after.access_token.as_deref(), Some("new_at"));
    }

    #[test]
    fn refresh_with_basic_auth_omits_creds_from_body() {
        let tmp = TempDir::new().unwrap();
        let (url, rx) = spawn_mock_server(r#"{"access_token":"basic_at","expires_in":3600}"#);
        let method = make_method(&url, "form", "basic");
        seed_oauth_state(tmp.path(), "foo", &method, "old_at", Some("old_rt"));

        let methods = vec![AuthMethod::Oauth2(method.clone())];
        refresh_access_token_at(tmp.path(), "foo", |_| Ok(methods.clone())).unwrap();

        let cap = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(cap.body.contains("grant_type=refresh_token"));
        assert!(cap.body.contains("refresh_token=old_rt"));
        assert!(!cap.body.contains("client_id="));
        assert!(!cap.body.contains("client_secret="));
    }

    #[test]
    fn refresh_errors_when_no_refresh_token_saved() {
        let tmp = TempDir::new().unwrap();
        let method = make_method("https://unused/", "form", "body");
        seed_oauth_state(tmp.path(), "foo", &method, "at", None);

        let err = refresh_access_token_at(tmp.path(), "foo", |_| {
            Ok(vec![AuthMethod::Oauth2(method.clone())])
        })
        .unwrap_err();
        assert!(err.to_string().contains("no refresh_token saved"));
    }

    #[test]
    fn refresh_errors_for_static_credentials() {
        let tmp = TempDir::new().unwrap();
        let auth = AuthFile {
            kind: Some(AuthKind::Static),
            api_key: Some("ghp_x".into()),
            ..Default::default()
        };
        token::save_auth_to(tmp.path(), "github", &auth).unwrap();

        let err = refresh_access_token_at(tmp.path(), "github", |_| Ok(vec![])).unwrap_err();
        assert!(err.to_string().contains("not an OAuth credential"));
    }

    #[test]
    fn refresh_surfaces_token_endpoint_failure() {
        let tmp = TempDir::new().unwrap();
        let url = spawn_failing_server(400, r#"{"error":"invalid_grant"}"#);
        let method = make_method(&url, "form", "body");
        seed_oauth_state(tmp.path(), "foo", &method, "old_at", Some("expired_rt"));

        let err = refresh_access_token_at(tmp.path(), "foo", |_| {
            Ok(vec![AuthMethod::Oauth2(method.clone())])
        })
        .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("400"));
        assert!(msg.contains("invalid_grant"));

        // Auth file unchanged on failure (still old token).
        let after = token::load_auth_from(tmp.path(), "foo").unwrap();
        assert_eq!(after.access_token.as_deref(), Some("old_at"));
        assert_eq!(after.refresh_token.as_deref(), Some("expired_rt"));
    }

    #[test]
    fn refresh_errors_when_descriptor_has_no_oauth_method() {
        let tmp = TempDir::new().unwrap();
        let method = make_method("https://unused/", "form", "body");
        seed_oauth_state(tmp.path(), "foo", &method, "at", Some("rt"));

        let err = refresh_access_token_at(tmp.path(), "foo", |_| Ok(vec![])).unwrap_err();
        assert!(err.to_string().contains("no OAuth method"));
    }

    #[test]
    fn pick_oauth_method_prefers_id_match_over_first() {
        let m1 = make_method("https://a/", "form", "body");
        let mut m2 = make_method("https://b/", "form", "body");
        m2.id = "alt".into();

        let methods = vec![
            AuthMethod::Oauth2(m1.clone()),
            AuthMethod::Oauth2(m2.clone()),
        ];
        let picked = pick_oauth_method(&methods, "alt").unwrap();
        assert_eq!(picked.id, "alt");
    }

    #[test]
    fn pick_oauth_method_falls_back_to_first_when_id_absent() {
        let m1 = make_method("https://a/", "form", "body");
        let methods = vec![AuthMethod::Oauth2(m1.clone())];
        let picked = pick_oauth_method(&methods, "does-not-exist").unwrap();
        assert_eq!(picked.id, "oauth");
    }

    #[test]
    fn merge_overlays_extras_and_preserves_unrelated_prev_fields() {
        let mut prev = AuthFile {
            kind: Some(AuthKind::Oauth2),
            method_id: Some("oauth".into()),
            access_token: Some("old_at".into()),
            refresh_token: Some("old_rt".into()),
            token_type: Some("Bearer".into()),
            scope: Some("read".into()),
            api_key: Some("legacy".into()),
            ..Default::default()
        };
        prev.extras.insert("workspace_id".into(), "ws_123".into());
        prev.extras.insert("bot_id".into(), "bot_old".into());

        let method = make_method("https://x/", "form", "body");

        let new = TokenResponse {
            access_token: "new_at".into(),
            refresh_token: None,
            expires_in: Some(7200),
            scope: None,
            token_type: None,
            extras: {
                let mut m = BTreeMap::new();
                m.insert("bot_id".into(), "bot_new".into());
                m
            },
        };

        let merged = merge_refresh_into_auth(&prev, &method, new, "old_rt");
        assert_eq!(merged.access_token.as_deref(), Some("new_at"));
        assert_eq!(merged.refresh_token.as_deref(), Some("old_rt"));
        assert_eq!(merged.token_type.as_deref(), Some("Bearer"));
        assert_eq!(merged.scope.as_deref(), Some("read"));
        assert_eq!(merged.api_key.as_deref(), Some("legacy"));
        assert_eq!(
            merged.extras.get("workspace_id").map(String::as_str),
            Some("ws_123")
        );
        assert_eq!(
            merged.extras.get("bot_id").map(String::as_str),
            Some("bot_new")
        );
    }
}
