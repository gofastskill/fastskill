//! FastSkill CLI binary entry point

#[path = "../cli/mod.rs"]
mod cli;

use clap::Parser;
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
                eprintln!("Error: {}", e);
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
            // Error message already printed by command
            std::process::exit(e.exit_code());
        }
    }
}
