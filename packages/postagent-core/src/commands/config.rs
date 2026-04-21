use regex::Regex;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

const DEFAULT_PROFILE: &str = "default";

/// Detects config keys that look like `postagent send` template tokens,
/// e.g. `GMAIL.API_KEY`, `GITHUB.TOKEN`, `NOTION.EXTRAS.WORKSPACE_ID`.
///
/// LLM-agent callers frequently confuse the two namespaces — the help text
/// mentions both `postagent config get apiKey` (registry config) and
/// `$POSTAGENT.<SITE>.API_KEY` (send-time template), and agents splice them
/// into `postagent config get GMAIL.API_KEY`. This command can never
/// succeed by design (per-site credentials are non-retrievable), so we
/// reject the shape up front with a pointer to the right tool.
fn template_shaped_key_pattern() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^(?P<site>[A-Za-z0-9_-]+)\.(?P<field>API_KEY|TOKEN|ACCESS_TOKEN|REFRESH_TOKEN|EXTRAS)(\.[A-Za-z0-9_]+)?$")
            .unwrap()
    })
}

/// Returns `Err(message)` when `key` looks like a `$POSTAGENT.<SITE>.*`
/// template reference rather than a real config key. The message explains
/// the distinction and points to the correct command, so an LLM agent
/// reading stderr can self-correct.
fn reject_if_template_shaped(action: &str, key: &str) -> Result<(), String> {
    let caps = match template_shaped_key_pattern().captures(key) {
        Some(c) => c,
        None => return Ok(()),
    };
    let site_upper = caps.name("site").unwrap().as_str().to_uppercase();
    let site_lower = site_upper.to_lowercase();
    let field = caps.name("field").unwrap().as_str();
    // Always recommend `.TOKEN` as the universal form — it dispatches on auth
    // kind (static api_key or OAuth access_token) and works regardless of how
    // the site was authenticated. EXTRAS is the one case where we must echo
    // back the specific sub-field, since there's no universal alternative.
    let template = if field == "EXTRAS" {
        format!("$POSTAGENT.{}.EXTRAS.<NAME>", site_upper)
    } else {
        format!("$POSTAGENT.{}.TOKEN", site_upper)
    };
    Err(format!(
        "\"{key}\" is a send-time template, not a config key.\n\
         Per-site credentials are never retrievable via `postagent config {action}` —\n\
         they only exist as substitutions inside `postagent send`.\n\
         \n\
         \u{2717} postagent config {action} {key}              (won't work, by design)\n\
         \u{2713} postagent send https://... -H 'Authorization: Bearer {template}'\n\
         \n\
         To check saved credentials:  postagent auth {site_lower} status\n\
         To save/refresh credentials: postagent auth {site_lower}",
        key = key,
        action = action,
        template = template,
        site_lower = site_lower,
    ))
}

fn config_file_with_base(base: &Path) -> PathBuf {
    base.join(".postagent")
        .join("profiles")
        .join(DEFAULT_PROFILE)
        .join("config.yaml")
}

fn config_file() -> PathBuf {
    let home = dirs::home_dir().expect("Cannot determine home directory");
    config_file_with_base(&home)
}

fn load_config(path: &Path) -> BTreeMap<String, String> {
    fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_yaml::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn get_value(key: &str) -> Option<String> {
    let path = config_file();
    load_config(&path).get(key).cloned()
}

fn non_empty(value: Option<String>) -> Option<String> {
    value.filter(|s| !s.is_empty())
}

fn resolve_api_key_from(
    env_api_key: Option<String>,
    config_api_key: Option<String>,
) -> Option<String> {
    non_empty(env_api_key).or_else(|| non_empty(config_api_key))
}

pub fn resolve_api_key() -> Option<String> {
    resolve_api_key_from(std::env::var("POSTAGENT_API_KEY").ok(), get_value("apiKey"))
}

fn save_config(
    path: &Path,
    config: &BTreeMap<String, String>,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let yaml = serde_yaml::to_string(config)?;
    fs::write(path, yaml)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }

    Ok(())
}

pub fn run(
    action: &str,
    key: Option<&str>,
    value: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    match action {
        "set" => {
            let key = key.ok_or("Usage: postagent config set <KEY> <VALUE>")?;
            reject_if_template_shaped("set", key)?;
            let value = value.ok_or("Usage: postagent config set <KEY> <VALUE>")?;
            let path = config_file();
            let mut config = load_config(&path);
            config.insert(key.to_string(), value.to_string());
            save_config(&path, &config)?;
            println!("Config saved: {} = {}", key, value);
            Ok(())
        }
        "get" => {
            let key = key.ok_or("Usage: postagent config get <KEY>")?;
            reject_if_template_shaped("get", key)?;
            let path = config_file();
            let config = load_config(&path);
            match config.get(key) {
                Some(v) => println!("{}", v),
                None => {
                    eprintln!("Config key \"{}\" not found.", key);
                    std::process::exit(1);
                }
            }
            Ok(())
        }
        _ => Err(format!("Unknown config action: {}. Use 'set' or 'get'.", action).into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn set_and_load_config() {
        let tmp = TempDir::new().unwrap();
        let path = config_file_with_base(tmp.path());

        let mut config = BTreeMap::new();
        config.insert("apiKey".to_string(), "ak_test123".to_string());
        save_config(&path, &config).unwrap();

        let loaded = load_config(&path);
        assert_eq!(loaded.get("apiKey"), Some(&"ak_test123".to_string()));
    }

    #[test]
    fn load_nonexistent_returns_empty() {
        let tmp = TempDir::new().unwrap();
        let path = config_file_with_base(tmp.path());
        let config = load_config(&path);
        assert!(config.is_empty());
    }

    #[test]
    fn set_preserves_existing_keys() {
        let tmp = TempDir::new().unwrap();
        let path = config_file_with_base(tmp.path());

        let mut config = BTreeMap::new();
        config.insert("apiKey".to_string(), "ak_first".to_string());
        save_config(&path, &config).unwrap();

        let mut config = load_config(&path);
        config.insert("other".to_string(), "value".to_string());
        save_config(&path, &config).unwrap();

        let loaded = load_config(&path);
        assert_eq!(loaded.get("apiKey"), Some(&"ak_first".to_string()));
        assert_eq!(loaded.get("other"), Some(&"value".to_string()));
    }

    #[cfg(unix)]
    #[test]
    fn config_file_permissions_600() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = TempDir::new().unwrap();
        let path = config_file_with_base(tmp.path());

        let mut config = BTreeMap::new();
        config.insert("apiKey".to_string(), "secret".to_string());
        save_config(&path, &config).unwrap();

        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn resolve_api_key_prefers_non_empty_env_value() {
        let resolved =
            resolve_api_key_from(Some("env-key".to_string()), Some("config-key".to_string()));
        assert_eq!(resolved.as_deref(), Some("env-key"));
    }

    #[test]
    fn resolve_api_key_falls_back_when_env_is_empty() {
        let resolved = resolve_api_key_from(Some(String::new()), Some("config-key".to_string()));
        assert_eq!(resolved.as_deref(), Some("config-key"));
    }

    #[test]
    fn resolve_api_key_treats_blank_config_as_missing() {
        let resolved = resolve_api_key_from(None, Some(String::new()));
        assert_eq!(resolved, None);
    }

    #[test]
    fn rejects_per_site_template_shaped_keys() {
        for key in [
            "GMAIL.API_KEY",
            "GITHUB.TOKEN",
            "NOTION.ACCESS_TOKEN",
            "SLACK.REFRESH_TOKEN",
            "NOTION.EXTRAS.WORKSPACE_ID",
            "gmail.api_key", // common typo path — regex is case-sensitive on the FIELD
        ] {
            // Only upper-case FIELD suffixes match the send-time template shape;
            // lower-case variants must pass through untouched.
            let is_upper_field = key
                .split_once('.')
                .map(|(_, tail)| {
                    let field = tail.split('.').next().unwrap_or("");
                    matches!(
                        field,
                        "API_KEY" | "TOKEN" | "ACCESS_TOKEN" | "REFRESH_TOKEN" | "EXTRAS"
                    )
                })
                .unwrap_or(false);
            let result = reject_if_template_shaped("get", key);
            if is_upper_field {
                let err = result.expect_err(&format!("expected rejection for {}", key));
                assert!(err.contains("send-time template"));
                assert!(err.contains("postagent auth"));
            } else {
                assert!(result.is_ok(), "key {:?} should pass through", key);
            }
        }
    }

    #[test]
    fn regular_config_keys_are_accepted() {
        for key in ["apiKey", "baseUrl", "profile", "my_key", "server.url"] {
            assert!(
                reject_if_template_shaped("get", key).is_ok(),
                "key {:?} should pass through",
                key
            );
        }
    }
}
