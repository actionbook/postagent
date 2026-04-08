use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "postagent-core",
    version,
    about = "CLI collection tool for agents",
    disable_help_subcommand = true
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,

    /// Output raw JSON instead of formatted text
    #[arg(long, global = true)]
    pub json: bool,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Search projects by keyword
    Search {
        /// Search query
        query: String,
    },
    /// Get project/group/action details (progressive discovery)
    Manual {
        /// Project name
        project: Option<String>,
        /// Group name
        group: Option<String>,
        /// Action name
        action: Option<String>,
    },
    /// Save API key for a project
    Auth {
        /// Project name
        project: String,
    },
    /// Send an HTTP request
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
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn parse_search_command() {
        let cli = Cli::parse_from(["postagent-core", "search", "github"]);
        assert!(matches!(cli.command, Commands::Search { query } if query == "github"));
        assert!(!cli.json);
    }

    #[test]
    fn parse_search_with_json_flag() {
        let cli = Cli::parse_from(["postagent-core", "--json", "search", "test"]);
        assert!(matches!(cli.command, Commands::Search { query } if query == "test"));
        assert!(cli.json);
    }

    #[test]
    fn parse_manual_no_args() {
        let cli = Cli::parse_from(["postagent-core", "manual"]);
        assert!(matches!(
            cli.command,
            Commands::Manual { project: None, group: None, action: None }
        ));
    }

    #[test]
    fn parse_manual_project_only() {
        let cli = Cli::parse_from(["postagent-core", "manual", "github"]);
        assert!(matches!(
            cli.command,
            Commands::Manual { project: Some(ref p), group: None, action: None } if p == "github"
        ));
    }

    #[test]
    fn parse_manual_project_and_group() {
        let cli = Cli::parse_from(["postagent-core", "manual", "github", "repos"]);
        assert!(matches!(
            cli.command,
            Commands::Manual { project: Some(ref p), group: Some(ref g), action: None }
                if p == "github" && g == "repos"
        ));
    }

    #[test]
    fn parse_manual_all_three_levels() {
        let cli = Cli::parse_from(["postagent-core", "manual", "github", "repos", "list"]);
        assert!(matches!(
            cli.command,
            Commands::Manual { project: Some(ref p), group: Some(ref g), action: Some(ref a) }
                if p == "github" && g == "repos" && a == "list"
        ));
    }

    #[test]
    fn parse_auth_command() {
        let cli = Cli::parse_from(["postagent-core", "auth", "openai"]);
        assert!(matches!(cli.command, Commands::Auth { project } if project == "openai"));
    }

    #[test]
    fn parse_send_minimal() {
        let cli = Cli::parse_from(["postagent-core", "send", "https://example.com"]);
        assert!(matches!(
            cli.command,
            Commands::Send { ref url, method: None, ref header, data: None }
                if url == "https://example.com" && header.is_empty()
        ));
    }

    #[test]
    fn parse_send_with_method_and_headers() {
        let cli = Cli::parse_from([
            "postagent-core", "send", "https://api.example.com",
            "-X", "POST",
            "-H", "Content-Type: application/json",
            "-H", "Authorization: Bearer token",
            "-d", r#"{"key":"value"}"#,
        ]);
        match cli.command {
            Commands::Send { url, method, header, data } => {
                assert_eq!(url, "https://api.example.com");
                assert_eq!(method, Some("POST".to_string()));
                assert_eq!(header.len(), 2);
                assert_eq!(header[0], "Content-Type: application/json");
                assert_eq!(header[1], "Authorization: Bearer token");
                assert_eq!(data, Some(r#"{"key":"value"}"#.to_string()));
            }
            _ => panic!("expected Send command"),
        }
    }

    #[test]
    fn json_flag_is_global() {
        let cli = Cli::parse_from(["postagent-core", "search", "test", "--json"]);
        assert!(cli.json);
    }

    #[test]
    fn default_is_not_json() {
        let cli = Cli::parse_from(["postagent-core", "search", "test"]);
        assert!(!cli.json);
    }
}
