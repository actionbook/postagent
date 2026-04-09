use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

const DEFAULT_PROFILE: &str = "default";

#[derive(Serialize, Deserialize, Default)]
struct AuthConfig {
    #[serde(flatten)]
    fields: BTreeMap<String, String>,
}

fn token_dir_with_base(base: &Path, site: &str) -> PathBuf {
    base.join(".postagent")
        .join("profiles")
        .join(DEFAULT_PROFILE)
        .join(site.to_lowercase())
}

fn auth_file(base: &Path, site: &str) -> PathBuf {
    token_dir_with_base(base, site).join("auth.yaml")
}

pub fn save_token(site: &str, token: &str) -> Result<(), Box<dyn std::error::Error>> {
    let home = dirs::home_dir().expect("Cannot determine home directory");
    save_token_to(&home, site, token)
}

fn save_token_to(base: &Path, site: &str, token: &str) -> Result<(), Box<dyn std::error::Error>> {
    let dir = token_dir_with_base(base, site);
    fs::create_dir_all(&dir)?;
    let file = auth_file(base, site);

    // Load existing config or create new
    let mut config = load_config(base, site).unwrap_or_default();
    config.fields.insert("api_key".to_string(), token.to_string());

    let yaml = serde_yaml::to_string(&config)?;
    fs::write(&file, yaml)?;
    set_file_permissions(&file)?;
    Ok(())
}

pub fn load_token(site: &str) -> Option<String> {
    let home = dirs::home_dir().expect("Cannot determine home directory");
    load_token_from(&home, site)
}

fn load_config(base: &Path, site: &str) -> Option<AuthConfig> {
    let content = fs::read_to_string(auth_file(base, site)).ok()?;
    serde_yaml::from_str(&content).ok()
}

fn load_token_from(base: &Path, site: &str) -> Option<String> {
    let config = load_config(base, site)?;
    config.fields.get("api_key").cloned()
}

pub fn resolve_template_variables(input: &str) -> Result<String, String> {
    let home = dirs::home_dir().expect("Cannot determine home directory");
    resolve_template_variables_with_base(&home, input)
}

fn resolve_template_variables_with_base(base: &Path, input: &str) -> Result<String, String> {
    let re = Regex::new(r"\$POSTAGENT\.([A-Za-z0-9_]+)\.API_KEY").unwrap();
    let mut result = input.to_string();
    for cap in re.captures_iter(input) {
        let site = cap[1].to_lowercase();
        let token = load_token_from(base, &site).ok_or_else(|| {
            format!(
                "Auth not found for \"{}\". Run: postagent auth {}",
                site, site
            )
        })?;
        result = result.replace(&cap[0], &token);
    }
    Ok(result)
}

#[cfg(unix)]
fn set_file_permissions(path: &std::path::Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
}

#[cfg(windows)]
fn set_file_permissions(_path: &std::path::Path) -> std::io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn save_then_load_token() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();

        save_token_to(base, "mysite", "secret-key-123").unwrap();
        let loaded = load_token_from(base, "mysite");
        assert_eq!(loaded, Some("secret-key-123".to_string()));
    }

    #[test]
    fn load_token_nonexistent_site_returns_none() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();

        let loaded = load_token_from(base, "nonexistent");
        assert_eq!(loaded, None);
    }

    #[test]
    fn save_token_is_case_insensitive() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();

        save_token_to(base, "MySite", "token-abc").unwrap();
        let loaded = load_token_from(base, "mysite");
        assert_eq!(loaded, Some("token-abc".to_string()));
    }

    #[test]
    fn load_token_from_yaml() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();

        // Manually write a YAML auth file
        let dir = token_dir_with_base(base, "yamltest");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("auth.yaml"), "api_key: my-token\n").unwrap();

        let loaded = load_token_from(base, "yamltest");
        assert_eq!(loaded, Some("my-token".to_string()));
    }

    #[test]
    fn resolve_single_variable() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();

        save_token_to(base, "github", "ghp_abc123").unwrap();

        let result =
            resolve_template_variables_with_base(base, "Bearer $POSTAGENT.GITHUB.API_KEY");
        assert_eq!(result, Ok("Bearer ghp_abc123".to_string()));
    }

    #[test]
    fn resolve_multiple_variables() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();

        save_token_to(base, "github", "ghp_abc").unwrap();
        save_token_to(base, "openai", "sk-xyz").unwrap();

        let input = "$POSTAGENT.GITHUB.API_KEY and $POSTAGENT.OPENAI.API_KEY";
        let result = resolve_template_variables_with_base(base, input);
        assert_eq!(result, Ok("ghp_abc and sk-xyz".to_string()));
    }

    #[test]
    fn resolve_no_variables_returns_unchanged() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();

        let input = "https://api.example.com/v1/data";
        let result = resolve_template_variables_with_base(base, input);
        assert_eq!(result, Ok(input.to_string()));
    }

    #[test]
    fn resolve_missing_token_returns_error() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();

        let result = resolve_template_variables_with_base(
            base,
            "Bearer $POSTAGENT.MISSING.API_KEY",
        );
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.contains("Auth not found for \"missing\""));
        assert!(err.contains("postagent auth missing"));
    }

    #[test]
    fn token_dir_structure_is_correct() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();

        let dir = token_dir_with_base(base, "MyApi");
        assert_eq!(
            dir,
            base.join(".postagent")
                .join("profiles")
                .join("default")
                .join("myapi")
        );
    }

    #[cfg(unix)]
    #[test]
    fn save_token_sets_file_permissions_to_600() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = TempDir::new().unwrap();
        let base = tmp.path();

        save_token_to(base, "permtest", "secret").unwrap();
        let file = auth_file(base, "permtest");
        let mode = fs::metadata(&file).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
}
