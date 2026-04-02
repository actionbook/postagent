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

    /// Output format: markdown / json
    #[arg(long, default_value = "markdown", global = true)]
    pub format: String,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Search projects by keyword
    Search {
        /// Search query
        query: String,
    },
    /// Get project/resource/action details (progressive discovery)
    Help {
        /// Project name
        project: Option<String>,
        /// Resource name
        resource: Option<String>,
        /// Action name
        action: Option<String>,
    },
    /// Save API key for a project
    Auth {
        /// Project name
        project: String,
    },
    /// Send an HTTP request
    Request {
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
        assert_eq!(cli.format, "markdown");
    }

    #[test]
    fn parse_search_with_json_format() {
        let cli = Cli::parse_from(["postagent-core", "--format", "json", "search", "test"]);
        assert!(matches!(cli.command, Commands::Search { query } if query == "test"));
        assert_eq!(cli.format, "json");
    }

    #[test]
    fn parse_help_no_args() {
        let cli = Cli::parse_from(["postagent-core", "help"]);
        assert!(matches!(
            cli.command,
            Commands::Help { project: None, resource: None, action: None }
        ));
    }

    #[test]
    fn parse_help_project_only() {
        let cli = Cli::parse_from(["postagent-core", "help", "github"]);
        assert!(matches!(
            cli.command,
            Commands::Help { project: Some(ref p), resource: None, action: None } if p == "github"
        ));
    }

    #[test]
    fn parse_help_project_and_resource() {
        let cli = Cli::parse_from(["postagent-core", "help", "github", "repos"]);
        assert!(matches!(
            cli.command,
            Commands::Help { project: Some(ref p), resource: Some(ref r), action: None }
                if p == "github" && r == "repos"
        ));
    }

    #[test]
    fn parse_help_all_three_levels() {
        let cli = Cli::parse_from(["postagent-core", "help", "github", "repos", "list"]);
        assert!(matches!(
            cli.command,
            Commands::Help { project: Some(ref p), resource: Some(ref r), action: Some(ref a) }
                if p == "github" && r == "repos" && a == "list"
        ));
    }

    #[test]
    fn parse_auth_command() {
        let cli = Cli::parse_from(["postagent-core", "auth", "openai"]);
        assert!(matches!(cli.command, Commands::Auth { project } if project == "openai"));
    }

    #[test]
    fn parse_request_minimal() {
        let cli = Cli::parse_from(["postagent-core", "request", "https://example.com"]);
        assert!(matches!(
            cli.command,
            Commands::Request { ref url, method: None, ref header, data: None }
                if url == "https://example.com" && header.is_empty()
        ));
    }

    #[test]
    fn parse_request_with_method_and_headers() {
        let cli = Cli::parse_from([
            "postagent-core", "request", "https://api.example.com",
            "-X", "POST",
            "-H", "Content-Type: application/json",
            "-H", "Authorization: Bearer token",
            "-d", r#"{"key":"value"}"#,
        ]);
        match cli.command {
            Commands::Request { url, method, header, data } => {
                assert_eq!(url, "https://api.example.com");
                assert_eq!(method, Some("POST".to_string()));
                assert_eq!(header.len(), 2);
                assert_eq!(header[0], "Content-Type: application/json");
                assert_eq!(header[1], "Authorization: Bearer token");
                assert_eq!(data, Some(r#"{"key":"value"}"#.to_string()));
            }
            _ => panic!("expected Request command"),
        }
    }

    #[test]
    fn format_flag_is_global() {
        let cli = Cli::parse_from(["postagent-core", "search", "test", "--format", "json"]);
        assert_eq!(cli.format, "json");
    }

    #[test]
    fn default_format_is_markdown() {
        let cli = Cli::parse_from(["postagent-core", "search", "test"]);
        assert_eq!(cli.format, "markdown");
    }
}
