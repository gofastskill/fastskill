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
    #[command(
        about = "Add a new skill to the skills database",
        after_help = "Examples:\n  fastskill add ./my-skill\n  fastskill add pptx@1.2.3"
    )]
    Add(add::AddArgs),

    /// Authentication commands (login, logout, whoami)
    #[command(
        about = "Manage authentication for registries",
        after_help = "Examples:\n  fastskill auth login\n  fastskill auth whoami"
    )]
    Auth(auth::AuthArgs),

    /// Disable one or more skills
    #[command(
        about = "Disable skills by ID",
        after_help = "Examples:\n  fastskill disable my-skill-id"
    )]
    Disable(disable::DisableArgs),

    /// Initialize skill-project.toml for skill authors
    #[command(
        about = "Initialize skill-project.toml in current skill directory",
        after_help = "Examples:\n  fastskill init\n  fastskill init --yes"
    )]
    Init(init::InitArgs),

    /// Install skills from skill-project.toml [dependencies] into .claude/skills/; creates or updates skills.lock
    #[command(
        about = "Install skills from skill-project.toml [dependencies] into .claude/skills/; creates or updates skills.lock",
        after_help = "Examples:\n  fastskill install\n  fastskill install --lock"
    )]
    Install(install::InstallArgs),

    /// List installed skills and reconcile with skill-project.toml and skills.lock (shows installed, missing, extraneous, version mismatches)
    #[command(
        about = "List installed skills and reconcile with skill-project.toml and skills.lock (shows installed, missing, extraneous, version mismatches)",
        after_help = "Examples:\n  fastskill list\n  fastskill list --json"
    )]
    List(list::ListArgs),

    /// Reindex the vector index by scanning skills directory
    #[command(
        about = "Reindex the vector index for semantic search",
        after_help = "Examples:\n  fastskill reindex\n  fastskill reindex --force"
    )]
    Reindex(reindex::ReindexArgs),

    /// Remove one or more skills
    #[command(
        about = "Remove skills from the skills database",
        after_help = "Examples:\n  fastskill remove my-skill-id\n  fastskill remove skill-a skill-b --force"
    )]
    Remove(remove::RemoveArgs),

    /// Package skills into ZIP artifacts
    #[command(
        about = "Package skills into ZIP artifacts with versioning",
        after_help = "Examples:\n  fastskill package\n  fastskill package --skills pptx --output ./out"
    )]
    Package(package::PackageArgs),

    /// Publish artifacts to blob storage
    #[command(
        about = "Publish artifacts to S3 blob storage",
        after_help = "Examples:\n  fastskill publish\n  fastskill publish --target ./local-folder"
    )]
    Publish(publish::PublishArgs),

    /// Stream skill documentation (SKILL.md content) to stdout
    #[command(
        about = "Stream skill documentation (SKILL.md content) to stdout",
        after_help = "Examples:\n  fastskill read pptx\n  fastskill read scope/skill-id"
    )]
    Read(read::ReadArgs),

    /// Manage repositories and browse/search the registry catalog (remote)
    #[command(
        about = "Manage repositories and browse/search the registry catalog (remote)",
        after_help = "Examples:\n  fastskill registry list-skills\n  fastskill registry search \"query\""
    )]
    Registry(registry::RegistryArgs),

    /// Search installed skills by query (local semantic search)
    #[command(
        about = "Search installed skills by query (local semantic search)",
        after_help = "Examples:\n  fastskill search \"text processing\"\n  fastskill search \"query\" -l 5 -f json"
    )]
    Search(search::SearchArgs),

    /// Start the FastSkill HTTP server
    #[command(
        about = "Start the FastSkill HTTP API server",
        after_help = "Examples:\n  fastskill serve\n  fastskill serve --port 3000"
    )]
    Serve(serve::ServeArgs),

    /// Show metadata for one or all installed skills (name, version, description, source); use --tree for dependency tree
    #[command(
        about = "Show metadata for one or all installed skills (name, version, description, source); use --tree for dependency tree",
        after_help = "Examples:\n  fastskill show\n  fastskill show pptx --tree"
    )]
    Show(show::ShowArgs),

    /// Manage skill sources (repositories): list, add, remove, show, update, test, refresh; create marketplace
    #[command(
        visible_alias = "source",
        after_help = "Examples:\n  fastskill sources list\n  fastskill sources add my-repo --repo-type local /path/to/skills"
    )]
    Sources(sources::SourcesArgs),

    /// Update skills to latest from source
    #[command(
        about = "Update skills to latest versions",
        after_help = "Examples:\n  fastskill update\n  fastskill update --check"
    )]
    Update(update::UpdateArgs),

    /// Display FastSkill version
    #[command(
        about = "Display FastSkill version",
        after_help = "Examples:\n  fastskill version"
    )]
    Version(version::VersionArgs),
}
