//! FastSkill CLI binary entry point

#[path = "../cli/mod.rs"]
mod cli;

use clap::{CommandFactory, Parser};
use cli::Cli;

#[tokio::main]
async fn main() {
    // Parse CLI arguments
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(e) => {
            // Handle help and version requests
            if e.kind() == clap::error::ErrorKind::DisplayHelp
                || e.kind() == clap::error::ErrorKind::DisplayVersion
            {
                let _ = e.print();
                std::process::exit(0);
            } else {
                // Print error message for invalid commands
                eprintln!("Error: {}", e);
                // Print help after error message
                let _ = Cli::command().print_help();
                std::process::exit(1);
            }
        }
    };

    // Execute the CLI command
    match cli.execute().await {
        Ok(()) => {
            std::process::exit(0);
        }
        Err(e) => {
            // Print error message
            eprintln!("Error: {}", e);
            std::process::exit(e.exit_code());
        }
    }
}
