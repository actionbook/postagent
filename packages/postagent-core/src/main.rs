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
        Commands::Search { query } => commands::search::run(query, cli.json),
        Commands::Manual {
            project,
            group,
            action,
        } => {
            let result = commands::manual::run(
                project.as_deref(),
                group.as_deref(),
                action.as_deref(),
                cli.json,
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
        Commands::Auth { project } => commands::auth::run(project),
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
