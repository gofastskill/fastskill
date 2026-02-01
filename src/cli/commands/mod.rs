//! Command modules for CLI

pub mod add;
pub mod auth;
pub mod disable;
pub mod init;
pub mod install;
pub mod list;
pub mod package;
pub mod publish;
pub mod read;
pub mod registry;
pub mod reindex;
pub mod remove;
pub mod search;
pub mod serve;
pub mod show;
pub mod sources;
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

    /// Install skills from skill-project.toml [dependencies] into .claude/skills/; creates or updates skills.lock
    #[command(
        about = "Install skills from skill-project.toml [dependencies] into .claude/skills/; creates or updates skills.lock"
    )]
    Install(install::InstallArgs),

    /// List installed skills and reconcile with skill-project.toml and skills.lock (shows installed, missing, extraneous, version mismatches)
    #[command(
        about = "List installed skills and reconcile with skill-project.toml and skills.lock (shows installed, missing, extraneous, version mismatches)"
    )]
    List(list::ListArgs),

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

    /// Stream skill documentation (SKILL.md content) to stdout
    #[command(about = "Stream skill documentation (SKILL.md content) to stdout")]
    Read(read::ReadArgs),

    /// Manage repositories and browse/search the registry catalog (remote)
    #[command(about = "Manage repositories and browse/search the registry catalog (remote)")]
    Registry(registry::RegistryArgs),

    /// Search installed skills by query (local semantic search)
    #[command(about = "Search installed skills by query (local semantic search)")]
    Search(search::SearchArgs),

    /// Start the FastSkill HTTP server
    #[command(about = "Start the FastSkill HTTP API server")]
    Serve(serve::ServeArgs),

    /// Show metadata for one or all installed skills (name, version, description, source); use --tree for dependency tree
    #[command(
        about = "Show metadata for one or all installed skills (name, version, description, source); use --tree for dependency tree"
    )]
    Show(show::ShowArgs),

    /// Manage skill sources (repositories): list, add, remove, show, update, test, refresh; create marketplace
    #[command(visible_alias = "source")]
    Sources(sources::SourcesArgs),

    /// Update skills to latest from source
    #[command(about = "Update skills to latest versions")]
    Update(update::UpdateArgs),

    /// Display FastSkill version
    #[command(about = "Display FastSkill version")]
    Version(version::VersionArgs),
}
