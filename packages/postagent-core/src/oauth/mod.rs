pub mod browser;
pub mod exchange;
pub mod loopback;
pub mod pkce;
pub mod state;

pub const REDIRECT_URI: &str = "http://127.0.0.1:9876/callback";

use crate::descriptor::OAuth2AuthMethod;
use exchange::{ExchangeInputs, TokenResponse};
use std::collections::BTreeMap;
use std::time::Duration;

pub enum AuthorizationCodeFlowOutcome {
    Authorized(TokenResponse),
    DryRun,
}

pub struct AuthParams<'a> {
    pub client_id: &'a str,
    pub client_secret: Option<&'a str>,
    /// Overrides scopes.default. If None, the descriptor default is used.
    pub scopes_override: Option<Vec<String>>,
    /// Values for params_required placeholders referenced in extra_params.
    pub placeholder_values: BTreeMap<String, String>,
    pub dry_run: bool,
    pub timeout: Duration,
}

enum AuthorizeUrlNextStep {
    AwaitCallback,
    DryRun,
}

/// Runs the full authorization_code + PKCE flow against a single descriptor.
/// On success returns the parsed token response.
pub fn run_authorization_code_flow(
    method: &OAuth2AuthMethod,
    params: &AuthParams<'_>,
) -> Result<AuthorizationCodeFlowOutcome, Box<dyn std::error::Error>> {
    let pkce = pkce::generate();
    let state_tok = state::generate();

    let scopes: Vec<String> = params
        .scopes_override
        .clone()
        .unwrap_or_else(|| method.scopes.default.clone());
    let sep = method.scopes.separator.as_str();
    let scope_str = scopes.join(sep);

    let authorize_url = build_authorize_url(
        method,
        params.client_id,
        &state_tok,
        &pkce.challenge,
        &scope_str,
        &params.placeholder_values,
    )?;

    // Bind the port BEFORE nudging the user / opening the browser so "port in
    // use" fails fast without confusing the user about what happened.
    // `listen_for_callback` binds inside, but we'd prefer to pre-bind for the
    // better error. However, pre-binding + rebinding in the helper would
    // conflict — so just let `listen_for_callback` own binding and surface
    // PortInUse as a clear error with exit 1 in the caller.

    match present_authorize_url(&authorize_url, params)? {
        AuthorizeUrlNextStep::DryRun => return Ok(AuthorizationCodeFlowOutcome::DryRun),
        AuthorizeUrlNextStep::AwaitCallback => {}
    }

    let cb = loopback::listen_for_callback(params.timeout)?;

    if let Some(err) = cb.error {
        let desc = cb.error_description.unwrap_or_default();
        return Err(format!("OAuth authorization failed: {} {}", err, desc).into());
    }

    let code = cb
        .code
        .ok_or("OAuth callback did not include a ?code parameter")?;
    let returned_state = cb.state.unwrap_or_default();
    if !state::equals(&returned_state, &state_tok) {
        return Err("OAuth state mismatch — possible CSRF; aborting.".into());
    }

    let tokens = exchange::exchange(ExchangeInputs {
        method,
        client_id: params.client_id,
        client_secret: params.client_secret,
        code: &code,
        code_verifier: &pkce.verifier,
        redirect_uri: REDIRECT_URI,
    })?;

    Ok(AuthorizationCodeFlowOutcome::Authorized(tokens))
}

fn present_authorize_url(
    authorize_url: &str,
    params: &AuthParams<'_>,
) -> Result<AuthorizeUrlNextStep, Box<dyn std::error::Error>> {
    eprintln!("Authorize URL prepared.");
    if params.dry_run {
        let path = browser::write_manual_url(authorize_url)?;
        eprintln!(
            "(dry run — authorize URL written to {}. Re-run without --dry-run to wait for the callback.)",
            path.display()
        );
        return Ok(AuthorizeUrlNextStep::DryRun);
    }

    eprintln!(
        "Listening for callback ({}s timeout):",
        params.timeout.as_secs()
    );
    eprintln!("{}", REDIRECT_URI);
    eprintln!("Opening browser ...");
    if !browser::open(authorize_url) {
        let path = browser::write_manual_url(authorize_url)?;
        eprintln!(
            "Could not open a browser. Authorize URL written to {}.",
            path.display()
        );
    }
    Ok(AuthorizeUrlNextStep::AwaitCallback)
}

fn build_authorize_url(
    method: &OAuth2AuthMethod,
    client_id: &str,
    state: &str,
    code_challenge: &str,
    scope_str: &str,
    placeholder_values: &BTreeMap<String, String>,
) -> Result<String, String> {
    let base = &method.authorize.url;
    let mut pairs: Vec<(String, String)> = vec![
        ("response_type".into(), "code".into()),
        ("client_id".into(), client_id.into()),
        ("redirect_uri".into(), REDIRECT_URI.into()),
        ("state".into(), state.into()),
        ("code_challenge".into(), code_challenge.into()),
        ("code_challenge_method".into(), "S256".into()),
    ];
    if !scope_str.is_empty() {
        pairs.push(("scope".into(), scope_str.into()));
    }

    if let Some(extra) = &method.authorize.extra_params {
        for (k, raw_v) in extra {
            if is_reserved_authorize_param(k) {
                return Err(format!(
                    "authorize.extra_params cannot override reserved OAuth parameter `{}`",
                    k
                ));
            }
            let v = apply_placeholders(raw_v, placeholder_values);
            pairs.push((k.clone(), v));
        }
    }

    let mut url = String::from(base);
    url.push(if base.contains('?') { '&' } else { '?' });
    url.push_str(
        &pairs
            .iter()
            .map(|(k, v)| format!("{}={}", pct_encode(k), pct_encode(v)))
            .collect::<Vec<_>>()
            .join("&"),
    );
    Ok(url)
}

fn is_reserved_authorize_param(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "response_type"
            | "client_id"
            | "redirect_uri"
            | "state"
            | "code_challenge"
            | "code_challenge_method"
            | "scope"
    )
}

/// Substitutes `{{name}}` placeholders with values from `values`. Single-brace
/// `{name}` is preserved as a literal character sequence (common in URL path
/// templates, OpenAPI refs). See `docs/design/oauth.md §3.2a`.
fn apply_placeholders(raw: &str, values: &BTreeMap<String, String>) -> String {
    let mut out = raw.to_string();
    for (k, v) in values {
        let needle = format!("{{{{{}}}}}", k); // literal `{{k}}`
        out = out.replace(&needle, v);
    }
    out
}

fn pct_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => out.push(c),
            ' ' => out.push_str("%20"),
            _ => {
                let mut buf = [0u8; 4];
                for &byte in c.encode_utf8(&mut buf).as_bytes() {
                    out.push_str(&format!("%{:02X}", byte));
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::descriptor::{
        AuthorizeSpec, ClientSpec, InjectSpec, OAuth2AuthMethod, RefreshSpec, ResponseMap,
        ScopesSpec, TokenSpec,
    };

    fn make_method(extra: Option<BTreeMap<String, String>>) -> OAuth2AuthMethod {
        OAuth2AuthMethod {
            id: "o".into(),
            label: "l".into(),
            setup_url: None,
            setup_instructions: None,
            provider: None,
            grants: vec!["authorization_code".into()],
            client: ClientSpec {
                client_type: "public".into(),
            },
            authorize: AuthorizeSpec {
                url: "https://example.com/auth".into(),
                extra_params: extra,
                params_required: None,
            },
            token: TokenSpec {
                url: "https://example.com/token".into(),
                body_encoding: "form".into(),
                client_auth: "body".into(),
                extra_headers: None,
                response_map: ResponseMap {
                    access_token: "/access_token".into(),
                    refresh_token: None,
                    expires_in: None,
                    scope: None,
                    token_type: None,
                    extras: None,
                },
            },
            scopes: ScopesSpec {
                default: vec!["repo".into(), "read:user".into()],
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

    #[test]
    fn authorize_url_includes_pkce_and_state() {
        let m = make_method(None);
        let url = build_authorize_url(&m, "cid", "st", "chal", "repo read:user", &BTreeMap::new())
            .unwrap();
        assert!(url.starts_with("https://example.com/auth?"));
        assert!(url.contains("response_type=code"));
        assert!(url.contains("client_id=cid"));
        assert!(url.contains("code_challenge=chal"));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains("state=st"));
        assert!(url.contains("scope=repo%20read%3Auser"));
        assert!(url.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A9876%2Fcallback"));
    }

    #[test]
    fn extra_params_merged_and_placeholders_applied() {
        let mut extra = BTreeMap::new();
        extra.insert("owner".into(), "user".into());
        extra.insert("tenant".into(), "{{tenant}}".into());

        let m = make_method(Some(extra));
        let mut ph = BTreeMap::new();
        ph.insert("tenant".into(), "acme".into());

        let url = build_authorize_url(&m, "cid", "st", "chal", "", &ph).unwrap();
        assert!(url.contains("owner=user"));
        assert!(url.contains("tenant=acme"));
    }

    #[test]
    fn reserved_authorize_params_cannot_be_overridden() {
        let mut extra = BTreeMap::new();
        extra.insert("state".into(), "bad".into());

        let method = make_method(Some(extra));
        let err =
            build_authorize_url(&method, "cid", "st", "chal", "", &BTreeMap::new()).unwrap_err();
        assert!(err.contains("reserved OAuth parameter `state`"));
    }

    #[test]
    fn single_brace_is_treated_as_literal() {
        // `{tenant}` (single brace) must NOT be substituted — it commonly
        // appears as a URL path template in docs and must pass through.
        let mut extra = BTreeMap::new();
        extra.insert("target".into(), "/repos/{owner}/{repo}".into());

        let m = make_method(Some(extra));
        let url = build_authorize_url(&m, "cid", "st", "chal", "", &BTreeMap::new()).unwrap();
        // The literal `{owner}` / `{repo}` are percent-encoded (%7B / %7D).
        assert!(url.contains("target=%2Frepos%2F%7Bowner%7D%2F%7Brepo%7D"));
    }

    #[test]
    fn empty_scope_is_omitted() {
        let m = make_method(None);
        let url = build_authorize_url(&m, "cid", "st", "chal", "", &BTreeMap::new()).unwrap();
        assert!(!url.contains("scope="));
    }

    #[test]
    fn dry_run_returns_before_waiting_for_callback() {
        let params = AuthParams {
            client_id: "cid",
            client_secret: None,
            scopes_override: None,
            placeholder_values: BTreeMap::new(),
            dry_run: true,
            timeout: Duration::from_millis(1),
        };

        let outcome = run_authorization_code_flow(&make_method(None), &params).unwrap();
        assert!(matches!(outcome, AuthorizationCodeFlowOutcome::DryRun));
    }
}
