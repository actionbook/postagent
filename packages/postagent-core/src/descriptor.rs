use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

/// Auth method descriptor sent by the server in /api/manual responses.
/// Mirrors the TypeScript `AuthMethod` discriminated union in
/// `packages/db/src/schema/types.ts`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
#[allow(clippy::large_enum_variant)]
pub enum AuthMethod {
    Static(StaticAuthMethod),
    Oauth2(OAuth2AuthMethod),
}

impl AuthMethod {
    pub fn id(&self) -> &str {
        match self {
            AuthMethod::Static(m) => &m.id,
            AuthMethod::Oauth2(m) => &m.id,
        }
    }

    pub fn label(&self) -> &str {
        match self {
            AuthMethod::Static(m) => &m.label,
            AuthMethod::Oauth2(m) => &m.label,
        }
    }

    #[allow(dead_code)]
    pub fn setup_instructions(&self) -> Option<&str> {
        match self {
            AuthMethod::Static(m) => m.setup_instructions.as_deref(),
            AuthMethod::Oauth2(m) => m.setup_instructions.as_deref(),
        }
    }

    #[allow(dead_code)]
    pub fn setup_url(&self) -> Option<&str> {
        match self {
            AuthMethod::Static(m) => m.setup_url.as_deref(),
            AuthMethod::Oauth2(m) => m.setup_url.as_deref(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StaticAuthMethod {
    pub id: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub setup_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub setup_instructions: Option<String>,
    pub scheme: String,
    #[serde(rename = "in")]
    pub location: String,
    pub name: String,
    /// Optional value template. Supports `{{token}}` placeholder. Defaults to
    /// `{{token}}` when absent, which bearer schemes render as `Bearer {{token}}`.
    /// Use for providers with non-standard prefixes, e.g. Discord: `Bot {{token}}`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value_template: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuth2AuthMethod {
    pub id: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub setup_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub setup_instructions: Option<String>,
    pub grants: Vec<String>,
    pub client: ClientSpec,
    pub authorize: AuthorizeSpec,
    pub token: TokenSpec,
    pub scopes: ScopesSpec,
    pub refresh: RefreshSpec,
    /// Non-empty list of injection points. The CLI applies every entry on
    /// each outbound request. See InjectSpec for placeholder semantics.
    pub injects: Vec<InjectSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientSpec {
    #[serde(rename = "type")]
    pub client_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthorizeSpec {
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extra_params: Option<BTreeMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params_required: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenSpec {
    pub url: String,
    pub body_encoding: String,
    pub client_auth: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extra_headers: Option<BTreeMap<String, String>>,
    pub response_map: ResponseMap,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseMap {
    pub access_token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_in: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extras: Option<BTreeMap<String, String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScopesSpec {
    pub default: Vec<String>,
    pub separator: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub buckets: Option<BTreeMap<String, ScopeBucket>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_magic_scope: Option<String>,
    /// Full catalog of scopes the provider publishes. Listed by
    /// `postagent auth <site> scopes` so users can escalate without
    /// leaving the CLI to re-read provider docs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub catalog: Option<Vec<ScopeCatalogEntry>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScopeCatalogEntry {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScopeBucket {
    pub default: Vec<String>,
    pub param: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefreshSpec {
    pub behavior: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expiry_instructions: Option<String>,
}

/// Where and how to attach the access token on outbound requests. See
/// `docs/design/oauth.md §3.2a` for placeholder contract.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InjectSpec {
    #[serde(rename = "in")]
    pub location: String,
    pub name: String,
    /// Supports `{{access_token}}` (runtime token) and `{{<extras_key>}}`
    /// (values from `response_map.extras`).
    pub value_template: String,
}

/// SHA256 of the canonical JSON serialization of the method, first 16 hex chars.
/// Used to detect drift between the descriptor in the server response and the
/// client_id/client_secret the user saved locally.
pub fn descriptor_hash(method: &AuthMethod) -> String {
    // serde_json::to_vec emits canonical key order because structs serialize
    // fields in declaration order. For the hash to be stable across server
    // round-trips we serialize the Rust type, not the original JSON bytes.
    let bytes = serde_json::to_vec(method).unwrap_or_default();
    let digest = Sha256::digest(&bytes);
    hex::encode(&digest[..8])
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_static_method() {
        let raw = json!({
            "kind": "static",
            "id": "pat",
            "label": "Personal Access Token",
            "scheme": "bearer",
            "in": "header",
            "name": "Authorization"
        });
        let method: AuthMethod = serde_json::from_value(raw).unwrap();
        match method {
            AuthMethod::Static(m) => {
                assert_eq!(m.id, "pat");
                assert_eq!(m.scheme, "bearer");
                assert_eq!(m.location, "header");
                assert_eq!(m.name, "Authorization");
            }
            _ => panic!("expected Static"),
        }
    }

    #[test]
    fn parse_oauth2_method() {
        let raw = json!({
            "kind": "oauth2",
            "id": "oauth",
            "label": "OAuth",
            "grants": ["authorization_code"],
            "client": { "type": "confidential" },
            "authorize": { "url": "https://example.com/auth" },
            "token": {
                "url": "https://example.com/token",
                "body_encoding": "json",
                "client_auth": "basic",
                "response_map": { "access_token": "/access_token" }
            },
            "scopes": { "default": [], "separator": " " },
            "refresh": { "behavior": "none" },
            "injects": [{
                "in": "header",
                "name": "Authorization",
                "value_template": "Bearer {{access_token}}"
            }]
        });
        let method: AuthMethod = serde_json::from_value(raw).unwrap();
        match method {
            AuthMethod::Oauth2(m) => {
                assert_eq!(m.id, "oauth");
                assert_eq!(m.client.client_type, "confidential");
                assert_eq!(m.token.body_encoding, "json");
                assert_eq!(m.injects.len(), 1);
                assert_eq!(m.injects[0].value_template, "Bearer {{access_token}}");
            }
            _ => panic!("expected Oauth2"),
        }
    }

    #[test]
    fn parse_oauth2_multi_inject() {
        let raw = json!({
            "kind": "oauth2",
            "id": "oauth",
            "label": "OAuth",
            "grants": ["authorization_code"],
            "client": { "type": "public" },
            "authorize": { "url": "https://example.com/auth" },
            "token": {
                "url": "https://example.com/token",
                "body_encoding": "form",
                "client_auth": "either",
                "response_map": { "access_token": "/access_token" }
            },
            "scopes": { "default": [], "separator": " " },
            "refresh": { "behavior": "reusable" },
            "injects": [
                { "in": "header", "name": "Authorization", "value_template": "Bearer {{access_token}}" },
                { "in": "header", "name": "X-Workspace-Id", "value_template": "{{workspace_id}}" }
            ]
        });
        let method: AuthMethod = serde_json::from_value(raw).unwrap();
        match method {
            AuthMethod::Oauth2(m) => {
                assert_eq!(m.injects.len(), 2);
                assert_eq!(m.injects[1].name, "X-Workspace-Id");
            }
            _ => panic!("expected Oauth2"),
        }
    }

    #[test]
    fn descriptor_hash_is_stable_and_16_hex() {
        let raw = json!({
            "kind": "static",
            "id": "pat",
            "label": "PAT",
            "scheme": "bearer",
            "in": "header",
            "name": "Authorization"
        });
        let m: AuthMethod = serde_json::from_value(raw).unwrap();
        let h1 = descriptor_hash(&m);
        let h2 = descriptor_hash(&m);
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 16);
        assert!(h1.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn descriptor_hash_differs_for_different_methods() {
        let a: AuthMethod = serde_json::from_value(json!({
            "kind": "static", "id": "a", "label": "A",
            "scheme": "bearer", "in": "header", "name": "Authorization"
        })).unwrap();
        let b: AuthMethod = serde_json::from_value(json!({
            "kind": "static", "id": "b", "label": "B",
            "scheme": "bearer", "in": "header", "name": "Authorization"
        })).unwrap();
        assert_ne!(descriptor_hash(&a), descriptor_hash(&b));
    }
}
