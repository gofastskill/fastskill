//! Command modules for CLI

pub mod add;
pub mod auth;
pub mod disable;
pub mod init;
pub mod install;
pub mod package;
pub mod proxy;
pub mod publish;
pub mod registry;
pub mod reindex;
pub mod remove;
pub mod search;
pub mod serve;
pub mod show;
pub mod update;
pub mod version;

use clap::Subcommand;

#[derive(Debug, Subcommand)]
#[command(about = "FastSkill CLI commands")]
pub enum Commands {
    /// Add a new skill from zip file, folder, or git URL
    #[command(about = "Add a new skill to the skills database")]
    Add(add::AddArgs),

    /// Authentication commands (login, logout, whoami)
    #[command(about = "Manage authentication for registries")]
    Auth(auth::AuthArgs),

    /// Disable one or more skills
    #[command(about = "Disable skills by ID")]
    Disable(disable::DisableArgs),

    /// Initialize skill-project.toml for skill authors
    #[command(about = "Initialize skill-project.toml in current skill directory")]
    Init(init::InitArgs),

    /// Install skills from skills.toml to .claude/skills/ registry
    #[command(about = "Install skills from skills.toml to registry")]
    Install(install::InstallArgs),

    /// Start the FastSkill proxy server for Anthropic API interception
    #[command(about = "Start proxy server to intercept and enhance Anthropic API calls")]
    Proxy(proxy::ProxyCommand),

    /// Reindex the vector index by scanning skills directory
    #[command(about = "Reindex the vector index for semantic search")]
    Reindex(reindex::ReindexArgs),

    /// Remove one or more skills
    #[command(about = "Remove skills from the skills database")]
    Remove(remove::RemoveArgs),

    /// Package skills into ZIP artifacts
    #[command(about = "Package skills into ZIP artifacts with versioning")]
    Package(package::PackageArgs),

    /// Publish artifacts to blob storage
    #[command(about = "Publish artifacts to S3 blob storage")]
    Publish(publish::PublishArgs),

    /// Registry management commands
    #[command(about = "Manage repositories and browse skills")]
    Registry(registry::RegistryArgs),

    /// Search for skills
    #[command(about = "Search for skills by query")]
    Search(search::SearchArgs),

    /// Start the FastSkill HTTP server
    #[command(about = "Start the FastSkill HTTP API server")]
    Serve(serve::ServeArgs),

    /// Show skill details and dependency tree
    #[command(about = "Show skill information and dependencies")]
    Show(show::ShowArgs),

    /// Update skills to latest from source
    #[command(about = "Update skills to latest versions")]
    Update(update::UpdateArgs),

    /// Display FastSkill version
    #[command(about = "Display FastSkill version")]
    Version(version::VersionArgs),
}
