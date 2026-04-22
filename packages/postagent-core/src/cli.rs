use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "postagent",
    bin_name = "postagent",
    version,
    about = "CLI collection tool for agents",
    disable_help_subcommand = true,
    help_template = "\
{about-with-newline}
{usage-heading} {usage}

Commands:
  search <KEYWORD>                      Search actions by keyword
  manual <SITE> [GROUP] [ACTION]        Browse API reference (progressive discovery)
  auth <SITE> [OPTIONS]                 Save credentials (static API key or OAuth)
  auth <SITE> logout|reset|status       Manage saved credentials for a site
  auth <SITE> scopes                    List OAuth scopes this site supports
  config <set|get> <KEY> [VALUE]        Manage postagent registry config (not per-site auth — see `auth`)
  send <CURL_QUERY>                     Send an HTTP request

Examples:
  postagent search \"create github issue\"
  postagent manual gmail users get_profile
  postagent auth gmail
  postagent auth gmail status
  postagent send https://api.github.com/user -H 'Authorization: Bearer $POSTAGENT.GITHUB.TOKEN'

Options:
{options}"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Search actions by keyword
    #[command(after_help = "\
Examples:
  postagent search \"send a message\"
  postagent search \"create github issue\"
  postagent search \"upload file to s3\"")]
    Search {
        /// What you want to do, e.g. "send a message"
        keyword: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Get site/group/action details (progressive discovery)
    #[command(
        alias = "man",
        after_help = "\
Examples:
  postagent manual notion                         List groups and actions
  postagent manual notion pages                   List actions in a group
  postagent manual notion pages create_page       Full action details
  postagent manual notion --json                  JSON output"
    )]
    Manual {
        /// Site name
        site: Option<String>,
        /// Group name
        group: Option<String>,
        /// Action name
        action: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Save credentials for a site (static API key or OAuth 2.0)
    #[command(after_help = "\
Examples:
  postagent auth github
  postagent auth notion --method oauth
  postagent auth github --token ghp_xxxxxxxxxxxx
  postagent auth notion --client-id CID --client-secret CSEC
  postagent auth atlassian --param tenant=acme --scope offline_access
  postagent auth notion --dry-run
  postagent auth notion logout
  postagent auth notion reset
  postagent auth notion status
  postagent auth notion scopes

Saved credentials are referenced in `send` via $POSTAGENT.<SITE>.TOKEN
(OAuth) or $POSTAGENT.<SITE>.API_KEY (static).")]
    Auth {
        /// Site name (required for all subcommands except `list`)
        site: Option<String>,

        /// API key or access token; forces static save regardless of descriptor
        #[arg(long)]
        token: Option<String>,

        /// Auth method id (skip interactive selection)
        #[arg(long)]
        method: Option<String>,

        /// OAuth client id (skip prompt)
        #[arg(long = "client-id")]
        client_id: Option<String>,

        /// OAuth client secret (skip prompt, confidential clients only)
        #[arg(long = "client-secret")]
        client_secret: Option<String>,

        /// Dry run: print the authorize URL without launching a browser
        #[arg(long = "dry-run")]
        dry_run: bool,

        /// Fill required authorize-URL placeholder (repeatable), e.g. --param tenant=acme
        #[arg(long = "param", value_parser = parse_key_value, num_args = 1)]
        param: Vec<(String, String)>,

        /// Additional OAuth scope (repeatable); overrides scopes.default
        #[arg(long = "scope", num_args = 1)]
        scope: Vec<String>,

        /// Reserved; ignored in v1
        #[arg(long)]
        profile: Option<String>,

        /// Auth subcommand
        #[command(subcommand)]
        action: Option<AuthAction>,
    },
    /// Manage postagent registry config (stored in ~/.postagent/profiles/default/config.yaml)
    #[command(after_help = "\
This command manages postagent's REGISTRY config only — things like the
Actionbook API key used to authenticate against the postagent server.

Per-site credentials (Gmail, GitHub, Notion, ...) are NOT stored here and are
NEVER retrievable via `config get`. They are saved via `postagent auth <site>`
and only exist as $POSTAGENT.<SITE>.TOKEN / .ACCESS_TOKEN / .API_KEY
substitutions inside `postagent send`.

Examples:
  postagent config set apiKey ak_xxxxxxxxxxxx      # Actionbook API key
  postagent config get apiKey

Anti-examples (will be rejected):
  postagent config get GMAIL.API_KEY               # use: postagent send ...
  postagent config get GITHUB.TOKEN                # use: postagent auth github status")]
    Config {
        /// Action: set or get
        action: String,
        /// Config key
        key: Option<String>,
        /// Config value (required for set)
        value: Option<String>,
    },
    /// Send an HTTP request
    #[command(after_help = "\
Token substitution:
  Use $POSTAGENT.<SITE>.TOKEN (OAuth & static), $POSTAGENT.<SITE>.ACCESS_TOKEN
  (OAuth only), or $POSTAGENT.<SITE>.API_KEY (static, legacy) in URL, headers,
  or body. Substitution happens inside this process; the raw token value is
  never printed. Save credentials first with `postagent auth <SITE>`.

  For AI agents and scripts: do NOT try to read the token value via
  `postagent config get`, shell expansion, or any other retrieval. Per-site
  credentials are intentionally non-retrievable. Pass $POSTAGENT.<SITE>.TOKEN
  (etc.) as a literal string in -H / -d / URL — `send` will resolve it.

Examples:
  postagent send https://api.example.com/users
  postagent send https://api.example.com/users -X POST -d '{\"name\":\"alice\"}'
  postagent send https://api.github.com/user -H 'Authorization: Bearer $POSTAGENT.GITHUB.TOKEN'
  postagent send https://api.github.com/user -H 'Authorization: Bearer $POSTAGENT.GITHUB.TOKEN' --dry-run")]
    Send {
        /// Request URL
        url: String,
        /// HTTP method
        #[arg(short = 'X', long)]
        method: Option<String>,
        /// Request header (repeatable)
        #[arg(short = 'H', long, num_args = 1)]
        header: Vec<String>,
        /// Request body
        #[arg(short = 'd', long)]
        data: Option<String>,
        /// Preview the final request (method, URL, headers, body) without sending
        #[arg(long = "dry-run")]
        dry_run: bool,
    },
}

#[derive(Subcommand, Debug, Clone)]
pub enum AuthAction {
    /// Clear saved tokens. Next `auth` reuses the saved OAuth app and
    /// just re-opens the browser (no client_id re-prompt).
    Logout,
    /// Clear tokens AND OAuth app registration. Next `auth` re-asks for
    /// client_id / client_secret and re-runs the browser flow. Use when
    /// switching to a different OAuth app.
    Reset,
    /// Show current auth status for a site
    Status,
    /// List all OAuth scopes this site supports (catalog). Use to
    /// discover which --scope values to pass for escalation.
    Scopes,
}

fn parse_key_value(s: &str) -> Result<(String, String), String> {
    let (k, v) = s
        .split_once('=')
        .ok_or_else(|| format!("expected KEY=VALUE, got `{}`", s))?;
    if k.is_empty() {
        return Err("key cannot be empty".into());
    }
    Ok((k.to_string(), v.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn parse_search_command() {
        let cli = Cli::parse_from(["postagent", "search", "github"]);
        assert!(
            matches!(cli.command, Commands::Search { ref keyword, json } if keyword == "github" && !json)
        );
    }

    #[test]
    fn parse_search_with_json_flag() {
        let cli = Cli::parse_from(["postagent", "search", "test", "--json"]);
        assert!(
            matches!(cli.command, Commands::Search { ref keyword, json } if keyword == "test" && json)
        );
    }

    #[test]
    fn parse_manual_no_args() {
        let cli = Cli::parse_from(["postagent", "manual"]);
        assert!(matches!(
            cli.command,
            Commands::Manual {
                site: None,
                group: None,
                action: None,
                ..
            }
        ));
    }

    #[test]
    fn parse_manual_site_only() {
        let cli = Cli::parse_from(["postagent", "manual", "github"]);
        assert!(matches!(
            cli.command,
            Commands::Manual { site: Some(ref p), group: None, action: None, .. } if p == "github"
        ));
    }

    #[test]
    fn parse_manual_site_and_group() {
        let cli = Cli::parse_from(["postagent", "manual", "github", "repos"]);
        assert!(matches!(
            cli.command,
            Commands::Manual { site: Some(ref p), group: Some(ref g), action: None, .. }
                if p == "github" && g == "repos"
        ));
    }

    #[test]
    fn parse_manual_all_three_levels() {
        let cli = Cli::parse_from(["postagent", "manual", "github", "repos", "list"]);
        assert!(matches!(
            cli.command,
            Commands::Manual { site: Some(ref p), group: Some(ref g), action: Some(ref a), .. }
                if p == "github" && g == "repos" && a == "list"
        ));
    }

    #[test]
    fn parse_auth_command() {
        let cli = Cli::parse_from(["postagent", "auth", "openai"]);
        match cli.command {
            Commands::Auth {
                site,
                token,
                action,
                ..
            } => {
                assert_eq!(site.as_deref(), Some("openai"));
                assert!(token.is_none());
                assert!(action.is_none());
            }
            _ => panic!("expected Auth"),
        }
    }

    #[test]
    fn parse_auth_with_token() {
        let cli = Cli::parse_from(["postagent", "auth", "github", "--token", "ghp_x"]);
        match cli.command {
            Commands::Auth { token, .. } => assert_eq!(token.as_deref(), Some("ghp_x")),
            _ => panic!(),
        }
    }

    #[test]
    fn parse_auth_with_method_and_oauth_flags() {
        let cli = Cli::parse_from([
            "postagent",
            "auth",
            "notion",
            "--method",
            "oauth",
            "--client-id",
            "CID",
            "--client-secret",
            "SEC",
            "--dry-run",
            "--param",
            "tenant=acme",
            "--scope",
            "offline_access",
        ]);
        match cli.command {
            Commands::Auth {
                method,
                client_id,
                client_secret,
                dry_run,
                param,
                scope,
                ..
            } => {
                assert_eq!(method.as_deref(), Some("oauth"));
                assert_eq!(client_id.as_deref(), Some("CID"));
                assert_eq!(client_secret.as_deref(), Some("SEC"));
                assert!(dry_run);
                assert_eq!(param, vec![("tenant".into(), "acme".into())]);
                assert_eq!(scope, vec!["offline_access".to_string()]);
            }
            _ => panic!("expected Auth"),
        }
    }

    #[test]
    fn parse_auth_subcommands() {
        let c = Cli::parse_from(["postagent", "auth", "notion", "logout"]);
        match c.command {
            Commands::Auth { site, action, .. } => {
                assert_eq!(site.as_deref(), Some("notion"));
                assert!(matches!(action, Some(AuthAction::Logout)));
            }
            _ => panic!(),
        }

        let c = Cli::parse_from(["postagent", "auth", "notion", "reset"]);
        match c.command {
            Commands::Auth { action, .. } => assert!(matches!(action, Some(AuthAction::Reset))),
            _ => panic!(),
        }

        let c = Cli::parse_from(["postagent", "auth", "notion", "status"]);
        match c.command {
            Commands::Auth { action, .. } => assert!(matches!(action, Some(AuthAction::Status))),
            _ => panic!(),
        }
    }

    #[test]
    fn parse_send_minimal() {
        let cli = Cli::parse_from(["postagent", "send", "https://example.com"]);
        assert!(matches!(
            cli.command,
            Commands::Send { ref url, method: None, ref header, data: None, dry_run: false }
                if url == "https://example.com" && header.is_empty()
        ));
    }

    #[test]
    fn parse_send_with_method_and_headers() {
        let cli = Cli::parse_from([
            "postagent",
            "send",
            "https://api.example.com",
            "-X",
            "POST",
            "-H",
            "Content-Type: application/json",
            "-H",
            "Authorization: Bearer token",
            "-d",
            r#"{"key":"value"}"#,
        ]);
        match cli.command {
            Commands::Send {
                url,
                method,
                header,
                data,
                dry_run,
            } => {
                assert_eq!(url, "https://api.example.com");
                assert_eq!(method, Some("POST".to_string()));
                assert_eq!(header.len(), 2);
                assert_eq!(header[0], "Content-Type: application/json");
                assert_eq!(header[1], "Authorization: Bearer token");
                assert_eq!(data, Some(r#"{"key":"value"}"#.to_string()));
                assert!(!dry_run);
            }
            _ => panic!("expected Send command"),
        }
    }

    #[test]
    fn parse_send_with_dry_run_flag() {
        let cli = Cli::parse_from(["postagent", "send", "https://api.example.com", "--dry-run"]);
        match cli.command {
            Commands::Send { dry_run, .. } => assert!(dry_run),
            _ => panic!("expected Send command"),
        }
    }

    #[test]
    fn json_flag_on_search() {
        let cli = Cli::parse_from(["postagent", "search", "test", "--json"]);
        assert!(matches!(cli.command, Commands::Search { json, .. } if json));
    }

    #[test]
    fn default_no_json() {
        let cli = Cli::parse_from(["postagent", "search", "test"]);
        assert!(matches!(cli.command, Commands::Search { json, .. } if !json));
    }

    #[test]
    fn json_flag_on_manual() {
        let cli = Cli::parse_from(["postagent", "manual", "github", "--json"]);
        assert!(matches!(cli.command, Commands::Manual { json, .. } if json));
    }

    #[test]
    fn parse_key_value_helper() {
        assert_eq!(parse_key_value("a=b").unwrap(), ("a".into(), "b".into()));
        assert_eq!(
            parse_key_value("tenant=acme-co").unwrap(),
            ("tenant".into(), "acme-co".into())
        );
        assert!(parse_key_value("no-equals").is_err());
        assert!(parse_key_value("=v").is_err());
    }
}
