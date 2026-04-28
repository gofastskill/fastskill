//! Main CLI application structure

use clap::Parser;
use fastskill_core::FastSkillService;
use std::sync::Arc;

use crate::commands::{
    add, analyze, auth, disable, eval, init, install, list, marketplace, package, publish, read,
    registry, reindex, remove, repos, resolve, search, serve, show, sources, sync, update, version,
    Commands,
};
use crate::config::create_service_config;
use crate::error::{CliError, CliResult};

/// FastSkill CLI - Manage skills and run operations
#[derive(Debug, Parser)]
#[command(name = "fastskill")]
#[command(version = fastskill_core::VERSION)]
#[command(about = "FastSkill CLI - Manage skills and run operations")]
#[command(long_about = "FastSkill provides skill management through CLI.\n\n\
                         Skills directory is configured in skill-project.toml:\n\
                         [tool.fastskill]\n\
                         skills_directory = \"path/to/skills\"\n\n\
                         Run 'fastskill init --skills-dir <path>' to create a new project.\n\n\
                         Examples:\n\
                           fastskill add ./my-skill           # Add skill from folder\n\
                           fastskill repos add local-skills --repo-type local /path/to/skills    # Add local repository\n\
                           fastskill pptx                     # Read skill documentation (SKILL.md)\n\
                           fastskill search \"text processing\" # Search for skills\n\
                           fastskill disable skill-id          # Disable a skill")]
#[command(arg_required_else_help = true)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    /// Path to repositories.toml file (overrides default location)
    #[arg(long, help = "Path to repositories.toml file")]
    pub repositories_path: Option<std::path::PathBuf>,

    /// Enable verbose output
    #[arg(short, long, help = "Enable verbose output")]
    pub verbose: bool,

    /// Skill ID (positional argument when no subcommand is used - routes to read command)
    #[arg(help = "Skill ID to read (used when no subcommand is specified)")]
    pub skill_id: Option<String>,

    /// Use user-level global skills directory (~/.config/fastskill/skills)
    #[arg(long, help = "Use global skills directory")]
    pub global: bool,
}

impl Cli {
    /// Execute the CLI command
    pub async fn execute(self) -> CliResult<()> {
        // Display version on initialization
        println!("FastSkill {}", fastskill_core::VERSION);

        // Initialize logging with verbose flag
        fastskill_core::init_logging_with_verbose(self.verbose);

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
            return show::execute_show(args, self.global).await;
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

        if let Some(Commands::Sources(args)) = self.command {
            return sources::execute_sources(args).await;
        }

        if let Some(Commands::Repos(args)) = self.command {
            return repos::execute_repos(args).await;
        }

        if let Some(Commands::Marketplace(args)) = self.command {
            return marketplace::execute_marketplace(args).await;
        }

        if let Some(Commands::Auth(args)) = self.command {
            return auth::execute_auth(args).await;
        }

        if let Some(Commands::Eval(args)) = self.command {
            return eval::execute_eval(args).await;
        }

        // For other commands, we need to restore self.command, so we need a different approach
        // Actually, if we get here, command was None or not Init/Repository/Package/Publish/RegistryIndex
        let command = self.command;
        let global = self.global;

        // Extract skills_dir override from command if present
        let skills_dir_override = match &command {
            Some(Commands::List(args)) => args.skills_dir.clone(),
            Some(Commands::Search(args)) => args.skills_dir.clone(),
            Some(Commands::Remove(args)) => args.skills_dir.clone(),
            Some(Commands::Reindex(args)) => args.skills_dir.clone(),
            Some(Commands::Resolve(args)) => args.skills_dir.clone(),
            _ => None,
        };

        // Create service configuration with resolved skills directory
        let config = create_service_config(global, skills_dir_override, None)?;

        // Initialize service
        let mut service = FastSkillService::new(config)
            .await
            .map_err(CliError::Service)?;
        service.initialize().await.map_err(CliError::Service)?;

        // Handle commands
        match command {
            Some(Commands::Add(args)) => add::execute_add(&service, args, global).await,
            Some(Commands::Analyze(args)) => analyze::execute_analyze(&service, args).await,
            Some(Commands::Disable(args)) => disable::execute_disable(&service, args).await,
            Some(Commands::List(args)) => list::execute_list(&service, args, global).await,
            Some(Commands::Read(args)) => read::execute_read(Arc::new(service), args).await,
            Some(Commands::Sync(args)) => sync::execute_sync(&service, args).await,
            Some(Commands::Reindex(args)) => reindex::execute_reindex(&service, args).await,
            Some(Commands::Remove(args)) => remove::execute_remove(&service, args, global).await,
            Some(Commands::Resolve(args)) => resolve::execute_resolve(&service, args).await,
            Some(Commands::Search(args)) => search::execute_search(&service, args).await,
            Some(Commands::Serve(args)) => serve::execute_serve(Arc::new(service), args).await,
            Some(Commands::Init(_))
            | Some(Commands::Install(_))
            | Some(Commands::Update(_))
            | Some(Commands::Show(_))
            | Some(Commands::Package(_))
            | Some(Commands::Publish(_))
            | Some(Commands::Registry(_))
            | Some(Commands::Sources(_))
            | Some(Commands::Repos(_))
            | Some(Commands::Marketplace(_))
            | Some(Commands::Auth(_))
            | Some(Commands::Eval(_))
            | Some(Commands::Version(_)) => unreachable!("Handled above"),
            None => {
                // No subcommand - treat as skill ID for read command
                if let Some(skill_id) = self.skill_id {
                    let read_args = read::ReadArgs { skill_id };
                    read::execute_read(Arc::new(service), read_args).await
                } else {
                    // This shouldn't happen due to arg_required_else_help, but handle it
                    Err(CliError::Config(
                        "No command or skill ID specified".to_string(),
                    ))
                }
            }
        }
    }
}
