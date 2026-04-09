mod api_response;
mod cli;
mod commands;
mod config;
mod error;
mod formatter;
mod token;

use clap::{CommandFactory, Parser};
use cli::{Cli, Commands};

fn main() {
    let cli = Cli::parse();

    let result = match &cli.command {
        Commands::Search { keyword, json } => commands::search::run(keyword, *json),
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
                *json,
            );
            // Handle the special "show_help" case (manual with no args)
            if let Err(ref e) = result {
                if e.to_string() == "show_help" {
                    Cli::command().print_help().ok();
                    println!();
                    return;
                }
            }
            result
        }
        Commands::Auth { site, token } => commands::auth::run(site, token.as_deref()),
        Commands::Config { action, key, value } => {
            commands::config::run(action, key.as_deref(), value.as_deref())
        }
        Commands::Send {
            url,
            method,
            header,
            data,
        } => commands::send::run(url, method.as_deref(), header, data.as_deref()),
    };

    if let Err(e) = result {
        eprintln!("{}", e);
        std::process::exit(1);
    }
}
