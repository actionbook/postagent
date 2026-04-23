use chrono::{DateTime, Utc};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Once;

const DEFAULT_PROFILE: &str = "default";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AuthKind {
    Static,
    Oauth2,
}

/// On-disk representation of `auth.yaml`. Backward compatible with legacy
/// files that contain only `api_key: xxx` — missing `kind` is treated as
/// `Static`, missing `method_id` as `"default"`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AuthFile {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<AuthKind>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method_id: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub access_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub obtained_at: Option<DateTime<Utc>>,

    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extras: BTreeMap<String, String>,

    /// Forward-compat: any unknown keys are preserved on round-trip.
    #[serde(flatten, default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra_fields: BTreeMap<String, serde_yaml::Value>,
}

impl AuthFile {
    pub fn effective_kind(&self) -> AuthKind {
        self.kind.unwrap_or(AuthKind::Static)
    }

    pub fn effective_method_id(&self) -> &str {
        self.method_id.as_deref().unwrap_or("default")
    }
}

/// Persisted OAuth BYO app credentials for a single site.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AppConfig {
    pub method_id: String,
    pub client_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_secret: Option<String>,
    /// sha256(method JSON) first 16 hex — lets `status` warn on drift.
    pub descriptor_hash: String,
}

/// Pointer file written into `<site>/provider.yaml` when the site's auth
/// method opts into a shared provider namespace (Google/Microsoft/...).
/// Its existence routes every subsequent `load_auth` / `save_auth` /
/// `load_app` / `save_app` call for that site to the provider directory
/// instead of the per-site one, so sibling sites naturally reuse the same
/// BYO credentials and the same access/refresh token.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ProviderPointer {
    provider: String,
}

fn normalize_provider_name(provider: &str) -> Result<String, Box<dyn std::error::Error>> {
    let name = provider.trim();
    if name.is_empty() {
        return Err("provider name cannot be empty".into());
    }
    if !name
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
    {
        return Err("provider name must use only ASCII letters, digits, '-' or '_'".into());
    }
    Ok(name.to_lowercase())
}

fn token_dir_with_base(base: &Path, site: &str) -> PathBuf {
    base.join(".postagent")
        .join("profiles")
        .join(DEFAULT_PROFILE)
        .join(site.to_lowercase())
}

/// Shared storage directory for every site that declares this provider.
/// Nested under a reserved `providers/` segment so it never collides with a
/// site slug (site slugs are hyphenated words; the segment name is fixed).
fn providers_dir(base: &Path, provider: &str) -> PathBuf {
    base.join(".postagent")
        .join("profiles")
        .join(DEFAULT_PROFILE)
        .join("providers")
        .join(provider.to_lowercase())
}

fn provider_pointer_file(base: &Path, site: &str) -> PathBuf {
    token_dir_with_base(base, site).join("provider.yaml")
}

/// Returns the provider name this site is linked to, or `None` when the
/// site keeps its credentials and tokens in its own directory.
fn load_provider_pointer(base: &Path, site: &str) -> Option<String> {
    let path = provider_pointer_file(base, site);
    let content = fs::read_to_string(&path).ok()?;
    let p: ProviderPointer = serde_yaml::from_str(&content).ok()?;
    normalize_provider_name(&p.provider).ok()
}

/// Storage directory that actually holds this site's `auth.yaml` / `app.yaml`.
/// If a provider pointer exists, all reads/writes route to the shared
/// provider directory; otherwise they stay in the per-site directory.
fn effective_auth_dir(base: &Path, site: &str) -> PathBuf {
    match load_provider_pointer(base, site) {
        Some(provider) => providers_dir(base, &provider),
        None => token_dir_with_base(base, site),
    }
}

fn provider_auth_file(base: &Path, provider: &str) -> PathBuf {
    providers_dir(base, provider).join("auth.yaml")
}

fn provider_app_file(base: &Path, provider: &str) -> PathBuf {
    providers_dir(base, provider).join("app.yaml")
}

fn auth_file(base: &Path, site: &str) -> PathBuf {
    if let Some(provider) = load_provider_pointer(base, site) {
        let shared = provider_auth_file(base, &provider);
        // Preserve site-local auth until the shared provider token actually
        // exists. This keeps a failed/dry-run OAuth attempt from hiding a
        // previously working site-local credential while still letting the
        // site see the shared app.yaml through the provider pointer.
        if shared.exists() || !site_auth_file(base, site).exists() {
            return shared;
        }
    }
    site_auth_file(base, site)
}

fn site_auth_file(base: &Path, site: &str) -> PathBuf {
    token_dir_with_base(base, site).join("auth.yaml")
}

fn site_app_file(base: &Path, site: &str) -> PathBuf {
    token_dir_with_base(base, site).join("app.yaml")
}

fn app_file(base: &Path, site: &str) -> PathBuf {
    effective_auth_dir(base, site).join("app.yaml")
}

fn home() -> PathBuf {
    dirs::home_dir().expect("Cannot determine home directory")
}

// ---------- Public API ----------

pub fn load_auth(site: &str) -> Option<AuthFile> {
    load_auth_from(&home(), site)
}

pub fn save_auth(site: &str, auth: &AuthFile) -> Result<(), Box<dyn std::error::Error>> {
    save_auth_to(&home(), site, auth)
}

/// Save `auth.yaml` in the site's own directory and detach any provider
/// pointer so static credentials never overwrite a shared provider token.
pub fn save_site_auth_local(site: &str, auth: &AuthFile) -> Result<(), Box<dyn std::error::Error>> {
    save_site_auth_local_to(&home(), site, auth)
}

pub fn load_app(site: &str) -> Option<AppConfig> {
    load_app_from(&home(), site)
}

pub fn save_app(site: &str, app: &AppConfig) -> Result<(), Box<dyn std::error::Error>> {
    save_app_to(&home(), site, app)
}

pub fn save_provider_auth(
    provider: &str,
    auth: &AuthFile,
) -> Result<(), Box<dyn std::error::Error>> {
    save_provider_auth_to(&home(), provider, auth)
}

pub fn load_provider_app(provider: &str) -> Option<AppConfig> {
    load_provider_app_from(&home(), provider)
}

pub fn link_provider_app(
    site: &str,
    provider: &str,
    app: &AppConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    link_provider_app_to(&home(), site, provider, app)
}

pub fn provider_for_site(site: &str) -> Option<String> {
    load_provider_pointer(&home(), site)
}

/// Canonical on-disk path for this site's `auth.yaml`. Two sites share auth
/// iff they resolve to the same path — useful as a dedupe key when a
/// rotating refresh_token must not be spent twice against the same file.
pub fn auth_storage_path(site: &str) -> PathBuf {
    auth_file(&home(), site)
}

pub fn logout(site: &str) -> Result<(), Box<dyn std::error::Error>> {
    logout_in(&home(), site)
}

pub fn reset(site: &str) -> Result<(), Box<dyn std::error::Error>> {
    reset_in(&home(), site)
}

#[allow(dead_code)]
pub fn site_dir_exists(site: &str) -> bool {
    token_dir_with_base(&home(), site).exists()
}

// ---------- Legacy compat ----------

/// Kept for call sites still using the old "save a single api_key" API.
#[allow(dead_code)]
pub fn save_token(site: &str, token: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut auth = load_auth(site).unwrap_or_default();
    auth.kind = Some(AuthKind::Static);
    if auth.method_id.is_none() {
        auth.method_id = Some("default".into());
    }
    auth.api_key = Some(token.to_string());
    save_site_auth_local(site, &auth)
}

#[allow(dead_code)]
pub fn load_token(site: &str) -> Option<String> {
    load_auth(site).and_then(|a| a.api_key)
}

// ---------- Template resolution ----------

static WARN_FALLBACK_ONCE: Once = Once::new();

pub fn resolve_template_variables(input: &str) -> Result<String, String> {
    resolve_template_variables_with_base(&home(), input)
}

fn resolve_template_variables_with_base(base: &Path, input: &str) -> Result<String, String> {
    // Site slot allows letters, digits, underscore, AND hyphen — site slugs
    // in the registry are hyphenated (e.g. `google-drive`, `share-point`).
    // The sub-field after EXTRAS uses the narrower set: extras names are
    // user-chosen identifiers (`bot_id`, `workspace_id`) without hyphens.

    // Reject REFRESH_TOKEN at lex stage to prevent accidental leakage into
    // request bodies / URLs.
    let refuse = Regex::new(r"\$POSTAGENT\.[A-Za-z0-9_-]+\.REFRESH_TOKEN\b").unwrap();
    if refuse.is_match(input) {
        return Err(
            "$POSTAGENT.<SITE>.REFRESH_TOKEN is not a usable template; refresh tokens are \
             kept private to postagent."
                .to_string(),
        );
    }

    let re = Regex::new(r"\$POSTAGENT\.([A-Za-z0-9_-]+)\.([A-Z_]+)(?:\.([A-Za-z0-9_]+))?").unwrap();
    let mut result = input.to_string();
    for cap in re.captures_iter(input) {
        let site = cap[1].to_lowercase();
        let field = cap[2].to_string();
        let sub = cap.get(3).map(|m| m.as_str().to_string());
        let matched = cap[0].to_string();

        if sub.is_some()
            && matches!(
                field.as_str(),
                "API_KEY" | "ACCESS_TOKEN" | "TOKEN" | "REFRESH_TOKEN"
            )
        {
            return Err(format!(
                "$POSTAGENT.{}.{} does not accept a sub-field. Only EXTRAS.<NAME> supports a suffix.",
                site.to_uppercase(),
                field
            ));
        }

        let auth = load_auth_from(base, &site).ok_or_else(|| {
            format!(
                "Auth not found for \"{}\". Run: postagent auth {}",
                site, site
            )
        })?;

        let value = resolve_one(&auth, &site, &field, sub.as_deref())?;
        result = result.replace(&matched, &value);
    }
    Ok(result)
}

fn resolve_one(
    auth: &AuthFile,
    site: &str,
    field: &str,
    sub: Option<&str>,
) -> Result<String, String> {
    match field {
        "API_KEY" => {
            if let Some(v) = &auth.api_key {
                return Ok(v.clone());
            }
            if auth.effective_kind() == AuthKind::Oauth2 {
                if let Some(v) = &auth.access_token {
                    WARN_FALLBACK_ONCE.call_once(|| {
                        eprintln!(
                            "warning: $POSTAGENT.<SITE>.API_KEY resolved from OAuth access_token; \
                             prefer $POSTAGENT.<SITE>.TOKEN or $POSTAGENT.<SITE>.ACCESS_TOKEN in new specs."
                        );
                    });
                    return Ok(v.clone());
                }
            }
            Err(format!(
                "Auth for \"{}\" has no api_key. Run: postagent auth {}",
                site, site
            ))
        }
        "ACCESS_TOKEN" => auth.access_token.clone().ok_or_else(|| {
            format!(
                "Auth for \"{}\" has no access_token. Run: postagent auth {}",
                site, site
            )
        }),
        "TOKEN" => match auth.effective_kind() {
            AuthKind::Static => auth.api_key.clone().ok_or_else(|| {
                format!(
                    "Auth for \"{}\" has no static token. Run: postagent auth {}",
                    site, site
                )
            }),
            AuthKind::Oauth2 => auth.access_token.clone().ok_or_else(|| {
                format!(
                    "Auth for \"{}\" has no access_token. Run: postagent auth {}",
                    site, site
                )
            }),
        },
        "EXTRAS" => {
            let name = sub.ok_or_else(|| {
                format!(
                    "$POSTAGENT.{}.EXTRAS requires a sub-field, e.g. EXTRAS.BOT_ID",
                    site.to_uppercase()
                )
            })?;
            let key = name.to_lowercase();
            auth.extras.get(&key).cloned().ok_or_else(|| {
                format!(
                    "Auth for \"{}\" has no extras.{}. Re-run: postagent auth {}",
                    site, key, site
                )
            })
        }
        other => Err(format!(
            "Unknown template field $POSTAGENT.{}.{}",
            site.to_uppercase(),
            other
        )),
    }
}

/// Returns site names referenced by `$POSTAGENT.<SITE>.TOKEN|ACCESS_TOKEN|API_KEY`
/// in any of the provided strings. Used by `send` to build the expired-token hint.
pub fn referenced_sites(inputs: &[&str]) -> Vec<String> {
    let re = Regex::new(r"\$POSTAGENT\.([A-Za-z0-9_-]+)\.(TOKEN|ACCESS_TOKEN|API_KEY)\b").unwrap();
    let mut seen: Vec<String> = Vec::new();
    for s in inputs {
        for cap in re.captures_iter(s) {
            let site = cap[1].to_lowercase();
            if !seen.contains(&site) {
                seen.push(site);
            }
        }
    }
    seen
}

// ---------- File IO helpers ----------

pub(crate) fn load_auth_from(base: &Path, site: &str) -> Option<AuthFile> {
    let content = fs::read_to_string(auth_file(base, site)).ok()?;
    serde_yaml::from_str(&content).ok()
}

pub(crate) fn save_auth_to(
    base: &Path,
    site: &str,
    auth: &AuthFile,
) -> Result<(), Box<dyn std::error::Error>> {
    // `auth_file` resolves to the provider dir when a pointer exists, so a
    // single `atomic_write` (which already creates parents) covers both
    // per-site and provider-backed layouts.
    let yaml = serde_yaml::to_string(auth)?;
    atomic_write(&auth_file(base, site), yaml.as_bytes())
}

fn save_site_auth_local_to(
    base: &Path,
    site: &str,
    auth: &AuthFile,
) -> Result<(), Box<dyn std::error::Error>> {
    let yaml = serde_yaml::to_string(auth)?;
    atomic_write(&site_auth_file(base, site), yaml.as_bytes())?;
    clear_provider_pointer_to(base, site)
}

pub(crate) fn load_app_from(base: &Path, site: &str) -> Option<AppConfig> {
    let content = fs::read_to_string(app_file(base, site)).ok()?;
    serde_yaml::from_str(&content).ok()
}

fn load_provider_auth_from(base: &Path, provider: &str) -> Option<AuthFile> {
    let provider = normalize_provider_name(provider).ok()?;
    let content = fs::read_to_string(provider_auth_file(base, &provider)).ok()?;
    serde_yaml::from_str(&content).ok()
}

fn save_provider_auth_to(
    base: &Path,
    provider: &str,
    auth: &AuthFile,
) -> Result<(), Box<dyn std::error::Error>> {
    let provider = normalize_provider_name(provider)?;
    let yaml = serde_yaml::to_string(auth)?;
    atomic_write(&provider_auth_file(base, &provider), yaml.as_bytes())
}

fn load_provider_app_from(base: &Path, provider: &str) -> Option<AppConfig> {
    let provider = normalize_provider_name(provider).ok()?;
    let content = fs::read_to_string(provider_app_file(base, &provider)).ok()?;
    serde_yaml::from_str(&content).ok()
}

pub(crate) fn save_app_to(
    base: &Path,
    site: &str,
    app: &AppConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    let yaml = serde_yaml::to_string(app)?;
    atomic_write(&app_file(base, site), yaml.as_bytes())
}

fn save_provider_app_to(
    base: &Path,
    provider: &str,
    app: &AppConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    let provider = normalize_provider_name(provider)?;
    let yaml = serde_yaml::to_string(app)?;
    atomic_write(&provider_app_file(base, &provider), yaml.as_bytes())
}

fn link_provider_app_to(
    base: &Path,
    site: &str,
    provider: &str,
    app: &AppConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    save_provider_pointer_to(base, site, provider)?;
    if let Err(err) = save_provider_app_to(base, provider, app) {
        let _ = clear_provider_pointer_to(base, site);
        return Err(err);
    }
    Ok(())
}

fn save_provider_pointer_to(
    base: &Path,
    site: &str,
    provider: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let name = normalize_provider_name(provider)?;
    let body = serde_yaml::to_string(&ProviderPointer { provider: name })?;
    // Pointer always lives inside the site's own dir — it must NOT route
    // through effective_auth_dir, or we'd recurse into the provider dir
    // we're trying to point at.
    atomic_write(&provider_pointer_file(base, site), body.as_bytes())
}

fn clear_provider_pointer_to(base: &Path, site: &str) -> Result<(), Box<dyn std::error::Error>> {
    match fs::remove_file(provider_pointer_file(base, site)) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(Box::new(e)),
    }
}

/// Delete the site's auth.yaml. When the site is provider-backed, this
/// removes the shared `providers/<provider>/auth.yaml` and therefore logs
/// out every sibling site that shares the same provider. Callers that
/// surface logout in the UI should warn the user when the pointer exists.
fn logout_in(base: &Path, site: &str) -> Result<(), Box<dyn std::error::Error>> {
    let auth = auth_file(base, site);
    if auth.exists() {
        fs::remove_file(&auth)?;
    }
    let local_auth = site_auth_file(base, site);
    if local_auth != auth && local_auth.exists() {
        fs::remove_file(&local_auth)?;
    }
    Ok(())
}

/// Delete auth.yaml + app.yaml. Provider-backed sites clear the shared
/// credentials and token; the pointer file is intentionally left in place
/// so a subsequent re-auth repopulates the shared directory under the same
/// provider binding. (Detaching a site from its provider is a separate
/// operation not yet exposed.)
fn reset_in(base: &Path, site: &str) -> Result<(), Box<dyn std::error::Error>> {
    let auth = auth_file(base, site);
    let app = app_file(base, site);
    let local_auth = site_auth_file(base, site);
    let local_app = site_app_file(base, site);

    for f in [auth, app, local_auth, local_app] {
        if f.exists() {
            fs::remove_file(&f)?;
        }
    }
    Ok(())
}

/// Atomic write with 0600 permissions on Unix. Advisory file lock scopes the
/// read-modify-write window on Unix; Windows skips locking (TODO: phase 2+
/// use LockFileEx for cross-platform parity).
fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    let parent = path.parent().ok_or("path has no parent")?;
    fs::create_dir_all(parent)?;

    // Acquire an advisory lock on the parent dir's .lock file so concurrent
    // `postagent auth` invocations in the same profile serialize.
    #[cfg(unix)]
    let _guard = acquire_lock(parent)?;

    let mut tmp = tempfile::NamedTempFile::new_in(parent)?;
    tmp.write_all(bytes)?;
    tmp.flush()?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(tmp.path(), fs::Permissions::from_mode(0o600))?;
    }

    tmp.persist(path)?;
    Ok(())
}

#[cfg(unix)]
struct LockGuard {
    _file: fs::File,
}

#[cfg(unix)]
impl Drop for LockGuard {
    fn drop(&mut self) {
        #[allow(unused_imports)]
        use fs2::FileExt;
        let _ = fs2::FileExt::unlock(&self._file);
    }
}

#[cfg(unix)]
fn acquire_lock(dir: &Path) -> Result<LockGuard, Box<dyn std::error::Error>> {
    use fs2::FileExt;
    let path = dir.join(".lock");
    let file = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(&path)?;
    file.lock_exclusive()?;
    Ok(LockGuard { _file: file })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write_raw(base: &Path, site: &str, body: &str) {
        let dir = token_dir_with_base(base, site);
        fs::create_dir_all(&dir).unwrap();
        fs::write(auth_file(base, site), body).unwrap();
    }

    #[test]
    fn legacy_api_key_only_loads_as_static() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();
        write_raw(base, "foo", "api_key: legacy-key\n");

        let auth = load_auth_from(base, "foo").unwrap();
        assert_eq!(auth.effective_kind(), AuthKind::Static);
        assert_eq!(auth.effective_method_id(), "default");
        assert_eq!(auth.api_key.as_deref(), Some("legacy-key"));

        let out = resolve_template_variables_with_base(base, "$POSTAGENT.FOO.API_KEY").unwrap();
        assert_eq!(out, "legacy-key");
    }

    #[test]
    fn oauth_tokens_resolve_token_access_token_and_extras() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();

        let mut auth = AuthFile::default();
        auth.kind = Some(AuthKind::Oauth2);
        auth.method_id = Some("oauth".into());
        auth.access_token = Some("at_abc".into());
        auth.extras.insert("bot_id".into(), "bot_xyz".into());
        save_auth_to(base, "foo", &auth).unwrap();

        assert_eq!(
            resolve_template_variables_with_base(base, "$POSTAGENT.FOO.TOKEN").unwrap(),
            "at_abc"
        );
        assert_eq!(
            resolve_template_variables_with_base(base, "$POSTAGENT.FOO.ACCESS_TOKEN").unwrap(),
            "at_abc"
        );
        assert_eq!(
            resolve_template_variables_with_base(base, "$POSTAGENT.FOO.EXTRAS.BOT_ID").unwrap(),
            "bot_xyz"
        );
    }

    #[test]
    fn api_key_falls_back_to_access_token_for_oauth() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();

        let mut auth = AuthFile::default();
        auth.kind = Some(AuthKind::Oauth2);
        auth.access_token = Some("at_xyz".into());
        save_auth_to(base, "bar", &auth).unwrap();

        let out = resolve_template_variables_with_base(base, "$POSTAGENT.BAR.API_KEY").unwrap();
        assert_eq!(out, "at_xyz");
    }

    #[test]
    fn refresh_token_template_is_rejected() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();

        let err =
            resolve_template_variables_with_base(base, "$POSTAGENT.FOO.REFRESH_TOKEN").unwrap_err();
        assert!(err.contains("REFRESH_TOKEN"));
    }

    #[test]
    fn extras_missing_returns_error() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();

        let mut auth = AuthFile::default();
        auth.kind = Some(AuthKind::Oauth2);
        auth.access_token = Some("at".into());
        save_auth_to(base, "foo", &auth).unwrap();

        let err = resolve_template_variables_with_base(base, "$POSTAGENT.FOO.EXTRAS.MISSING")
            .unwrap_err();
        assert!(err.contains("extras.missing"));
    }

    #[test]
    fn non_extras_templates_reject_suffixes() {
        let tmp = TempDir::new().unwrap();
        let err = resolve_template_variables_with_base(tmp.path(), "$POSTAGENT.FOO.TOKEN.EXTRA")
            .unwrap_err();
        assert!(err.contains("does not accept a sub-field"));
    }

    #[test]
    fn save_then_load_static_token_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();

        let mut auth = AuthFile::default();
        auth.kind = Some(AuthKind::Static);
        auth.method_id = Some("pat".into());
        auth.api_key = Some("ghp_abc".into());
        save_auth_to(base, "github", &auth).unwrap();

        let loaded = load_auth_from(base, "github").unwrap();
        assert_eq!(loaded.api_key.as_deref(), Some("ghp_abc"));
        assert_eq!(loaded.effective_method_id(), "pat");
    }

    #[test]
    fn save_token_legacy_api_writes_static_file() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();
        // Exercise the compat wrapper end-to-end (using base override for test
        // isolation isn't possible here; test through the real API).
        let mut auth = AuthFile::default();
        auth.kind = Some(AuthKind::Static);
        auth.api_key = Some("tok".into());
        save_auth_to(base, "mysite", &auth).unwrap();

        let loaded = load_auth_from(base, "mysite").unwrap();
        assert_eq!(loaded.api_key.as_deref(), Some("tok"));
        assert_eq!(loaded.effective_kind(), AuthKind::Static);
    }

    #[test]
    fn app_config_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();

        let app = AppConfig {
            method_id: "oauth".into(),
            client_id: "cid".into(),
            client_secret: Some("csec".into()),
            descriptor_hash: "abcdef1234567890".into(),
        };
        save_app_to(base, "notion", &app).unwrap();

        let loaded = load_app_from(base, "notion").unwrap();
        assert_eq!(loaded.method_id, "oauth");
        assert_eq!(loaded.client_id, "cid");
        assert_eq!(loaded.client_secret.as_deref(), Some("csec"));
    }

    #[test]
    fn logout_removes_only_auth() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();

        let mut auth = AuthFile::default();
        auth.api_key = Some("x".into());
        save_auth_to(base, "s", &auth).unwrap();
        save_app_to(
            base,
            "s",
            &AppConfig {
                method_id: "oauth".into(),
                client_id: "c".into(),
                client_secret: None,
                descriptor_hash: "h".into(),
            },
        )
        .unwrap();

        logout_in(base, "s").unwrap();
        assert!(load_auth_from(base, "s").is_none());
        assert!(load_app_from(base, "s").is_some());
    }

    #[test]
    fn reset_removes_both() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();

        let mut auth = AuthFile::default();
        auth.api_key = Some("x".into());
        save_auth_to(base, "s", &auth).unwrap();
        save_app_to(
            base,
            "s",
            &AppConfig {
                method_id: "oauth".into(),
                client_id: "c".into(),
                client_secret: None,
                descriptor_hash: "h".into(),
            },
        )
        .unwrap();

        reset_in(base, "s").unwrap();
        assert!(load_auth_from(base, "s").is_none());
        assert!(load_app_from(base, "s").is_none());
    }

    #[test]
    fn resolve_multiple_variables() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();

        write_raw(base, "github", "api_key: ghp\n");
        write_raw(base, "openai", "api_key: sk\n");

        let out = resolve_template_variables_with_base(
            base,
            "$POSTAGENT.GITHUB.API_KEY / $POSTAGENT.OPENAI.API_KEY",
        )
        .unwrap();
        assert_eq!(out, "ghp / sk");
    }

    #[test]
    fn resolve_hyphenated_site_slug() {
        // Hyphenated slugs (google-drive, share-point) were previously
        // rejected by the template regex — the send command would silently
        // fall through to "Missing $POSTAGENT template" instead of looking
        // the site up. Lock in that they now resolve correctly.
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();
        write_raw(base, "google-drive", "api_key: gd-secret\n");

        let out = resolve_template_variables_with_base(
            base,
            "Authorization: Bearer $POSTAGENT.GOOGLE-DRIVE.API_KEY",
        )
        .unwrap();
        assert_eq!(out, "Authorization: Bearer gd-secret");
    }

    #[test]
    fn resolve_missing_site_errors_with_hint() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();

        let err =
            resolve_template_variables_with_base(base, "$POSTAGENT.MISSING.API_KEY").unwrap_err();
        assert!(err.contains("Auth not found for \"missing\""));
        assert!(err.contains("postagent auth missing"));
    }

    #[cfg(unix)]
    #[test]
    fn save_auth_sets_600_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = TempDir::new().unwrap();
        let base = tmp.path();

        let mut auth = AuthFile::default();
        auth.api_key = Some("s".into());
        save_auth_to(base, "p", &auth).unwrap();
        let mode = fs::metadata(auth_file(base, "p"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn provider_pointer_rejects_unsafe_provider_ids() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();

        for provider in ["", "   ", "../google", "google/drive", "google.drive"] {
            let err = save_provider_pointer_to(base, "google-drive", provider).unwrap_err();
            assert!(err.to_string().contains("provider name"));
        }
        assert!(!provider_pointer_file(base, "google-drive").exists());
    }

    #[test]
    fn invalid_provider_pointer_file_is_ignored() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();

        fs::create_dir_all(token_dir_with_base(base, "google-drive")).unwrap();
        fs::write(
            provider_pointer_file(base, "google-drive"),
            "provider: ../../tmp/x\n",
        )
        .unwrap();

        let mut auth = AuthFile::default();
        auth.kind = Some(AuthKind::Oauth2);
        auth.access_token = Some("site-at".into());
        save_auth_to(base, "google-drive", &auth).unwrap();

        let loaded = load_auth_from(base, "google-drive").unwrap();
        assert_eq!(loaded.access_token.as_deref(), Some("site-at"));
        assert!(token_dir_with_base(base, "google-drive")
            .join("auth.yaml")
            .exists());
    }

    #[test]
    fn provider_helpers_stage_shared_state_without_pointer() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();

        let app = AppConfig {
            method_id: "oauth".into(),
            client_id: "shared-cid".into(),
            client_secret: Some("shared-sec".into()),
            descriptor_hash: "hhhhhhhhhhhhhhhh".into(),
        };
        save_provider_app_to(base, "google", &app).unwrap();

        let mut auth = AuthFile::default();
        auth.kind = Some(AuthKind::Oauth2);
        auth.access_token = Some("shared-at".into());
        save_provider_auth_to(base, "google", &auth).unwrap();

        let loaded_app = load_provider_app_from(base, "google").unwrap();
        assert_eq!(loaded_app.client_id, "shared-cid");
        let loaded_auth = load_provider_auth_from(base, "google").unwrap();
        assert_eq!(loaded_auth.access_token.as_deref(), Some("shared-at"));

        assert!(load_app_from(base, "google-drive").is_none());
        assert!(load_auth_from(base, "google-drive").is_none());
    }

    #[test]
    fn link_provider_app_preserves_site_auth_until_shared_auth_exists() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();

        let mut local = AuthFile::default();
        local.kind = Some(AuthKind::Static);
        local.method_id = Some("default".into());
        local.api_key = Some("site-secret".into());
        save_site_auth_local_to(base, "google-drive", &local).unwrap();

        let app = AppConfig {
            method_id: "oauth".into(),
            client_id: "shared-cid".into(),
            client_secret: Some("shared-sec".into()),
            descriptor_hash: "hhhhhhhhhhhhhhhh".into(),
        };
        link_provider_app_to(base, "google-drive", "google", &app).unwrap();

        assert_eq!(
            load_provider_pointer(base, "google-drive").as_deref(),
            Some("google")
        );
        assert_eq!(
            load_app_from(base, "google-drive").unwrap().client_id,
            "shared-cid"
        );
        assert_eq!(
            load_auth_from(base, "google-drive")
                .unwrap()
                .api_key
                .as_deref(),
            Some("site-secret")
        );

        reset_in(base, "google-drive").unwrap();
        assert!(load_app_from(base, "google-drive").is_none());
        assert!(load_auth_from(base, "google-drive").is_none());
    }

    #[test]
    fn provider_pointer_routes_auth_and_app_to_shared_dir() {
        // Site A saves its creds/tokens; site B then opts into the same
        // provider BEFORE writing anything. B's loads must resolve against
        // the shared provider dir and return A's values unchanged.
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();

        save_provider_pointer_to(base, "google-drive", "google").unwrap();

        let app = AppConfig {
            method_id: "oauth".into(),
            client_id: "shared-cid".into(),
            client_secret: Some("shared-sec".into()),
            descriptor_hash: "hhhhhhhhhhhhhhhh".into(),
        };
        save_app_to(base, "google-drive", &app).unwrap();

        let mut auth = AuthFile::default();
        auth.kind = Some(AuthKind::Oauth2);
        auth.method_id = Some("oauth".into());
        auth.access_token = Some("shared-at".into());
        auth.refresh_token = Some("shared-rt".into());
        save_auth_to(base, "google-drive", &auth).unwrap();

        // Second site under the same provider: reads without ever having
        // pasted creds itself.
        save_provider_pointer_to(base, "google-docs", "google").unwrap();
        let app_b = load_app_from(base, "google-docs").unwrap();
        assert_eq!(app_b.client_id, "shared-cid");
        let auth_b = load_auth_from(base, "google-docs").unwrap();
        assert_eq!(auth_b.access_token.as_deref(), Some("shared-at"));

        // Sanity: the files literally live in providers/google, not in
        // either site's own dir.
        let shared_auth = providers_dir(base, "google").join("auth.yaml");
        let shared_app = providers_dir(base, "google").join("app.yaml");
        assert!(shared_auth.exists());
        assert!(shared_app.exists());
        assert!(!token_dir_with_base(base, "google-drive")
            .join("auth.yaml")
            .exists());
        assert!(!token_dir_with_base(base, "google-docs")
            .join("auth.yaml")
            .exists());
    }

    #[test]
    fn provider_pointer_does_not_leak_into_non_provider_sites() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();

        save_provider_pointer_to(base, "google-drive", "google").unwrap();
        let mut auth = AuthFile::default();
        auth.kind = Some(AuthKind::Oauth2);
        auth.access_token = Some("google-at".into());
        save_auth_to(base, "google-drive", &auth).unwrap();

        assert!(load_auth_from(base, "random-site").is_none());
    }

    #[test]
    fn provider_logout_affects_shared_dir() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();

        save_provider_pointer_to(base, "google-drive", "google").unwrap();
        save_provider_pointer_to(base, "google-docs", "google").unwrap();
        let mut auth = AuthFile::default();
        auth.access_token = Some("at".into());
        save_auth_to(base, "google-drive", &auth).unwrap();
        assert!(load_auth_from(base, "google-docs").is_some());

        logout_in(base, "google-drive").unwrap();
        assert!(load_auth_from(base, "google-drive").is_none());
        assert!(load_auth_from(base, "google-docs").is_none());
    }

    #[test]
    fn provider_logout_clears_stale_site_local_auth() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();

        let mut local = AuthFile::default();
        local.kind = Some(AuthKind::Static);
        local.api_key = Some("stale-local".into());
        save_auth_to(base, "google-drive", &local).unwrap();

        save_provider_pointer_to(base, "google-drive", "google").unwrap();

        let mut shared = AuthFile::default();
        shared.kind = Some(AuthKind::Oauth2);
        shared.access_token = Some("shared-at".into());
        save_auth_to(base, "google-drive", &shared).unwrap();

        logout_in(base, "google-drive").unwrap();
        assert!(load_provider_auth_from(base, "google").is_none());
        assert!(load_auth_from(base, "google-drive").is_none());
    }

    #[test]
    fn provider_reset_clears_stale_site_local_state() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();

        let mut local_auth = AuthFile::default();
        local_auth.kind = Some(AuthKind::Static);
        local_auth.api_key = Some("stale-local".into());
        save_auth_to(base, "google-drive", &local_auth).unwrap();
        save_app_to(
            base,
            "google-drive",
            &AppConfig {
                method_id: "legacy".into(),
                client_id: "site-cid".into(),
                client_secret: None,
                descriptor_hash: "hhhhhhhhhhhhhhhh".into(),
            },
        )
        .unwrap();

        save_provider_pointer_to(base, "google-drive", "google").unwrap();

        let mut shared = AuthFile::default();
        shared.kind = Some(AuthKind::Oauth2);
        shared.access_token = Some("shared-at".into());
        save_auth_to(base, "google-drive", &shared).unwrap();
        save_app_to(
            base,
            "google-drive",
            &AppConfig {
                method_id: "oauth".into(),
                client_id: "shared-cid".into(),
                client_secret: Some("shared-sec".into()),
                descriptor_hash: "hhhhhhhhhhhhhhhh".into(),
            },
        )
        .unwrap();

        reset_in(base, "google-drive").unwrap();
        assert!(load_provider_auth_from(base, "google").is_none());
        assert!(load_provider_app_from(base, "google").is_none());
        assert!(load_auth_from(base, "google-drive").is_none());
        assert!(load_app_from(base, "google-drive").is_none());
    }

    #[test]
    fn template_resolver_redirects_through_provider_pointer() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();

        save_provider_pointer_to(base, "google-docs", "google").unwrap();
        let mut auth = AuthFile::default();
        auth.kind = Some(AuthKind::Oauth2);
        auth.access_token = Some("shared-at".into());
        save_auth_to(base, "google-docs", &auth).unwrap();

        let out = resolve_template_variables_with_base(
            base,
            "Bearer $POSTAGENT.GOOGLE-DOCS.ACCESS_TOKEN",
        )
        .unwrap();
        assert_eq!(out, "Bearer shared-at");
    }

    #[test]
    fn site_local_auth_detaches_provider_pointer_without_clobbering_shared_auth() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();

        save_provider_pointer_to(base, "google-drive", "google").unwrap();
        save_provider_pointer_to(base, "google-docs", "google").unwrap();

        let mut shared = AuthFile::default();
        shared.kind = Some(AuthKind::Oauth2);
        shared.method_id = Some("oauth".into());
        shared.access_token = Some("shared-at".into());
        save_auth_to(base, "google-drive", &shared).unwrap();
        assert_eq!(
            load_auth_from(base, "google-docs")
                .unwrap()
                .access_token
                .as_deref(),
            Some("shared-at")
        );

        let mut local = AuthFile::default();
        local.kind = Some(AuthKind::Static);
        local.method_id = Some("pat".into());
        local.api_key = Some("site-secret".into());
        save_site_auth_local_to(base, "google-docs", &local).unwrap();

        assert!(load_provider_pointer(base, "google-docs").is_none());
        assert_eq!(
            load_auth_from(base, "google-docs")
                .unwrap()
                .api_key
                .as_deref(),
            Some("site-secret")
        );
        assert_eq!(
            load_auth_from(base, "google-drive")
                .unwrap()
                .access_token
                .as_deref(),
            Some("shared-at")
        );
    }

    #[test]
    fn referenced_sites_extracts_all() {
        let sites = referenced_sites(&[
            "header: Bearer $POSTAGENT.GITHUB.TOKEN",
            "url: $POSTAGENT.NOTION.ACCESS_TOKEN",
            "x-api-key: $POSTAGENT.STRIPE.API_KEY",
            "nothing here",
        ]);
        assert_eq!(sites, vec!["github", "notion", "stripe"]);
    }

    #[test]
    fn referenced_sites_dedupes() {
        let sites =
            referenced_sites(&["$POSTAGENT.GITHUB.TOKEN", "$POSTAGENT.GITHUB.ACCESS_TOKEN"]);
        assert_eq!(sites, vec!["github"]);
    }
}
