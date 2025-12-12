//! Main CLI application structure

use clap::Parser;
use fastskill::FastSkillService;
use std::sync::Arc;

use crate::cli::commands::{
    add, auth, disable, init, install, list, package, publish, read, registry, reindex, remove,
    search, serve, show, update, version, Commands,
};
use crate::cli::config::create_service_config;
use crate::cli::error::{CliError, CliResult};

/// FastSkill CLI - Manage skills and run operations
#[derive(Debug, Parser)]
#[command(name = "fastskill")]
#[command(version = fastskill::VERSION)]
#[command(about = "FastSkill CLI - Manage skills and run operations")]
#[command(long_about = "FastSkill provides skill management through CLI.\n\n\
                         Skills directory is resolved using this priority:\n\
                         1. skills_directory from .fastskill.yaml (if exists)\n\
                         2. Walk up directory tree to find existing .claude/skills/\n\
                         3. Default to .claude/skills/ in current directory\n\n\
                         Examples:\n\
                           fastskill add ./my-skill           # Add skill from folder\n\
                           fastskill registry add local-skills --repo-type local /path/to/skills  # Add local repository\n\
                           fastskill \"text processing\"        # Search for skills\n\
                           fastskill disable skill-id          # Disable a skill")]
#[command(arg_required_else_help = true)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    /// Path to repositories.toml file (overrides default location)
    #[arg(long, help = "Path to .claude/repositories.toml file")]
    pub repositories_path: Option<std::path::PathBuf>,

    /// Enable verbose output
    #[arg(short, long, help = "Enable verbose output")]
    pub verbose: bool,

    /// Search query (positional argument when no subcommand is used)
    #[arg(help = "Search query string (used when no subcommand is specified)")]
    pub query: Option<String>,
}

impl Cli {
    /// Execute the CLI command
    pub async fn execute(self) -> CliResult<()> {
        // Display version on initialization
        println!("FastSkill {}", fastskill::VERSION);

        // Initialize logging
        fastskill::init_logging();

        // Handle version command early (no service needed)
        if let Some(Commands::Version(args)) = self.command {
            return version::execute_version(args).await;
        }

        // Handle init, install, update, show commands before service initialization
        if let Some(Commands::Init(args)) = self.command {
            return init::execute_init(args).await;
        }

        if let Some(Commands::Install(args)) = self.command {
            return install::execute_install(args).await;
        }

        if let Some(Commands::Update(args)) = self.command {
            return update::execute_update(args).await;
        }

        if let Some(Commands::Show(args)) = self.command {
            return show::execute_show(args).await;
        }

        if let Some(Commands::Package(args)) = self.command {
            return package::execute_package(args).await;
        }

        if let Some(Commands::Publish(args)) = self.command {
            return publish::execute_publish(args).await;
        }

        if let Some(Commands::Registry(args)) = self.command {
            return registry::execute_registry(args).await;
        }

        if let Some(Commands::Auth(args)) = self.command {
            return auth::execute_auth(args).await;
        }

        // For other commands, we need to restore self.command, so we need a different approach
        // Actually, if we get here, command was None or not Init/Repository/Package/Publish/RegistryIndex
        let command = self.command;

        // Create service configuration with resolved skills directory
        let config = create_service_config(None, None)?;

        if self.verbose {
            println!(
                "Using skills directory: {}",
                config.skill_storage_path.display()
            );
        }

        // Initialize service
        let mut service = FastSkillService::new(config).await.map_err(CliError::Service)?;
        service.initialize().await.map_err(CliError::Service)?;

        // Handle commands
        match command {
            Some(Commands::Add(args)) => add::execute_add(&service, args).await,
            Some(Commands::Disable(args)) => disable::execute_disable(&service, args).await,
            Some(Commands::List(args)) => list::execute_list(&service, args).await,
            Some(Commands::Read(args)) => read::execute_read(Arc::new(service), args).await,
            Some(Commands::Reindex(args)) => reindex::execute_reindex(&service, args).await,
            Some(Commands::Remove(args)) => remove::execute_remove(&service, args).await,
            Some(Commands::Search(args)) => search::execute_search(&service, args).await,
            Some(Commands::Serve(args)) => serve::execute_serve(&service, args).await,
            Some(Commands::Init(_))
            | Some(Commands::Install(_))
            | Some(Commands::Update(_))
            | Some(Commands::Show(_))
            | Some(Commands::Package(_))
            | Some(Commands::Publish(_))
            | Some(Commands::Registry(_))
            | Some(Commands::Auth(_))
            | Some(Commands::Version(_)) => unreachable!("Handled above"),
            None => {
                // No subcommand - treat as search query
                if let Some(query) = self.query {
                    let search_args = search::SearchArgs {
                        query,
                        limit: 10,
                        format: "table".to_string(),
                    };
                    search::execute_search(&service, search_args).await
                } else {
                    // This shouldn't happen due to arg_required_else_help, but handle it
                    Err(CliError::Config(
                        "No command or search query specified".to_string(),
                    ))
                }
            }
        }
    }
}
