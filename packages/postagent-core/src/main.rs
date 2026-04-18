mod api_response;
mod cli;
mod commands;
mod config;
mod descriptor;
mod error;
mod formatter;
mod markdown;
mod oauth;
mod token;

use clap::{CommandFactory, Parser};
use cli::{AuthAction, Cli, Commands};

fn main() {
    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Search { keyword, json } => commands::search::run(&keyword, json),
        Commands::Manual {
            site,
            group,
            action,
            json,
        } => {
            let result = commands::manual::run(
                site.as_deref(),
                group.as_deref(),
                action.as_deref(),
                json,
            );
            if let Err(ref e) = result {
                if e.to_string() == "show_help" {
                    Cli::command().print_help().ok();
                    println!();
                    return;
                }
            }
            result
        }
        Commands::Auth {
            site,
            token,
            method,
            client_id,
            client_secret,
            no_browser,
            param,
            scope,
            profile,
            action,
        } => {
            if profile.is_some() {
                eprintln!("--profile is reserved and ignored in v1");
            }
            match (site.as_deref(), action) {
                (None, Some(AuthAction::List)) | (Some("list"), None) => commands::auth::list(),
                (Some(s), Some(AuthAction::Logout)) => commands::auth::logout(s),
                (Some(s), Some(AuthAction::ResetApp)) => commands::auth::reset_app(s),
                (Some(s), Some(AuthAction::Status)) => commands::auth::status(s),
                (Some(_), Some(AuthAction::List)) => {
                    eprintln!("Use `postagent auth list` without a site.");
                    std::process::exit(1);
                }
                (Some(s), None) => commands::auth::login(commands::auth::LoginArgs {
                    site: s,
                    token: token.as_deref(),
                    method: method.as_deref(),
                    client_id: client_id.as_deref(),
                    client_secret: client_secret.as_deref(),
                    no_browser,
                    params: &param,
                    scopes: &scope,
                }),
                (None, None) => {
                    eprintln!("Usage: postagent auth <site> [options]");
                    eprintln!("       postagent auth list");
                    std::process::exit(1);
                }
                (None, Some(_)) => {
                    eprintln!("This subcommand requires a <site>.");
                    std::process::exit(1);
                }
            }
        }
        Commands::Config { action, key, value } => {
            commands::config::run(&action, key.as_deref(), value.as_deref())
        }
        Commands::Send {
            url,
            method,
            header,
            data,
        } => commands::send::run(&url, method.as_deref(), &header, data.as_deref()),
    };

    if let Err(e) = result {
        eprintln!("{}", e);
        std::process::exit(1);
    }
}
