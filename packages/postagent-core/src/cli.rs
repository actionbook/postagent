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
  manual [SITE] [GROUP] [ACTION]        Browse API reference (progressive discovery)
  auth <SITE>                           Save credentials for a site
  config <set|get> <KEY> [VALUE]        Manage config values
  send <CURL_QUERY>                     Send an HTTP request

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
        /// Output format: markdown / json
        #[arg(long, default_value = "markdown")]
        format: String,
    },
    /// Get site/group/action details (progressive discovery)
    #[command(after_help = "\
Examples:
  postagent manual notion                         List groups and actions
  postagent manual notion pages                   List actions in a group
  postagent manual notion pages create_page       Full action details
  postagent manual feishu --format json            JSON output")]
    Manual {
        /// Site name
        site: Option<String>,
        /// Group name
        group: Option<String>,
        /// Action name
        action: Option<String>,
        /// Output format: markdown / json
        #[arg(long, default_value = "markdown")]
        format: String,
    },
    /// Save credentials for a site
    #[command(after_help = "\
Examples:
  postagent auth github
  postagent auth openai

Saved keys can be referenced in `send` as $POSTAGENT.<SITE>.API_KEY
For example, after `postagent auth github`, use $POSTAGENT.GITHUB.API_KEY in headers.")]
    Auth {
        /// Site name
        site: String,
    },
    /// Set or get config values
    #[command(after_help = "\
Examples:
  postagent config set apiKey ak_xxxxxxxxxxxx
  postagent config get apiKey")]
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
  Use $POSTAGENT.<SITE>.API_KEY in URL, headers, or body to inject saved keys.
  Save a key first with `postagent auth <SITE>`.

Examples:
  postagent send https://api.example.com/users
  postagent send https://api.example.com/users -X POST -d '{\"name\":\"alice\"}'
  postagent send https://api.example.com/me -H 'Authorization: Bearer $POSTAGENT.GITHUB.API_KEY'")]
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
        let cli = Cli::parse_from(["postagent", "search", "github"]);
        assert!(matches!(cli.command, Commands::Search { ref keyword, ref format } if keyword == "github" && format == "markdown"));
    }

    #[test]
    fn parse_search_with_json_format() {
        let cli = Cli::parse_from(["postagent", "search", "test", "--format", "json"]);
        assert!(matches!(cli.command, Commands::Search { ref keyword, ref format } if keyword == "test" && format == "json"));
    }

    #[test]
    fn parse_manual_no_args() {
        let cli = Cli::parse_from(["postagent", "manual"]);
        assert!(matches!(
            cli.command,
            Commands::Manual { site: None, group: None, action: None, .. }
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
        assert!(matches!(cli.command, Commands::Auth { site } if site == "openai"));
    }

    #[test]
    fn parse_send_minimal() {
        let cli = Cli::parse_from(["postagent", "send", "https://example.com"]);
        assert!(matches!(
            cli.command,
            Commands::Send { ref url, method: None, ref header, data: None }
                if url == "https://example.com" && header.is_empty()
        ));
    }

    #[test]
    fn parse_send_with_method_and_headers() {
        let cli = Cli::parse_from([
            "postagent", "send", "https://api.example.com",
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
    fn format_flag_on_search() {
        let cli = Cli::parse_from(["postagent", "search", "test", "--format", "json"]);
        assert!(matches!(cli.command, Commands::Search { ref format, .. } if format == "json"));
    }

    #[test]
    fn default_format_is_markdown() {
        let cli = Cli::parse_from(["postagent", "search", "test"]);
        assert!(matches!(cli.command, Commands::Search { ref format, .. } if format == "markdown"));
    }

    #[test]
    fn format_flag_on_manual() {
        let cli = Cli::parse_from(["postagent", "manual", "github", "--format", "json"]);
        assert!(matches!(cli.command, Commands::Manual { ref format, .. } if format == "json"));
    }
}
