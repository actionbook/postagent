use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

const DEFAULT_PROFILE: &str = "default";

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
}
