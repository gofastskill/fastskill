//! Command modules for CLI

pub mod add;
pub mod analyze;
pub mod auth;
pub mod common;
pub mod disable;
pub mod eval;
pub mod init;
pub mod install;
pub mod list;
pub mod marketplace;
pub mod package;
pub mod publish;
pub mod read;
pub mod registry;
pub mod reindex;
pub mod remove;
pub mod repos;
pub mod resolve;
pub mod search;
pub mod serve;
pub mod show;
pub mod sources;
pub mod sync;
pub mod update;
pub mod version;

use clap::Subcommand;

#[derive(Debug, Subcommand)]
#[command(about = "FastSkill CLI commands")]
pub enum Commands {
    /// Add a skill (two modes: add to project manifest OR register skill locally)
    #[command(
        about = "Add a skill (two modes: add to project manifest OR register skill locally)",
        after_help = "Two operational modes:\n\n1. Manifest-managed: Adds dependency to skill-project.toml [dependencies]\n   - Requires skill-project.toml to exist\n   - Use 'fastskill install' afterward to apply changes\n\n2. Local-only: Register skill directly to local installation\n   - Works without skill-project.toml\n   - Skill available immediately but not in manifest\n\nExamples:\n  fastskill add pptx@1.2.3     # Add to manifest or local\n  fastskill add ./my-skill     # Add local folder\n  fastskill add --editable ./dev-skill  # Development mode"
    )]
    Add(add::AddArgs),

    /// Diagnostic and analysis commands
    #[command(
        about = "Diagnostic and analysis commands",
        after_help = "Examples:\n  fastskill analyze matrix\n  fastskill analyze matrix --threshold 0.8"
    )]
    Analyze(analyze::AnalyzeCommand),

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

    /// Evaluation commands for skill quality assurance
    #[command(
        about = "Evaluation commands for skill quality assurance",
        after_help = "Examples:\n  fastskill eval validate\n  fastskill eval run --agent codex --output-dir /tmp/evals"
    )]
    Eval(eval::EvalCommand),

    /// Initialize skill-project.toml for skill authors
    #[command(
        about = "Initialize skill-project.toml in current skill directory",
        after_help = "Examples:\n  fastskill init\n  fastskill init --yes"
    )]
    Init(init::InitArgs),

    /// Apply manifest: install skills from skill-project.toml [dependencies] (canonical command for manifest-driven workflow)
    #[command(
        about = "Apply manifest: install skills from skill-project.toml [dependencies] (canonical command for manifest-driven workflow)",
        after_help = "This is the primary command for installing project dependencies.\nReads from skill-project.toml [dependencies] and creates/updates skills.lock.\n\nExamples:\n  fastskill install\n  fastskill install --lock  # Install exact versions from skills.lock"
    )]
    Install(install::InstallArgs),

    /// List installed skills and reconcile with skill-project.toml and skills.lock (shows name, description, and flags by default; use --details for full info)
    #[command(
        about = "List installed skills and reconcile with skill-project.toml and skills.lock (shows name, description, and flags by default; use --details for full info)",
        after_help = "Examples:\n  fastskill list\n  fastskill list --details\n  fastskill list --json"
    )]
    List(list::ListArgs),

    /// Marketplace generation and management
    #[command(
        about = "Create and manage skill marketplace artifacts",
        after_help = "Examples:\n  fastskill marketplace create\n  fastskill marketplace create --path ./skills --name my-marketplace"
    )]
    Marketplace(marketplace::MarketplaceArgs),

    /// Reindex the vector index by scanning skills directory
    #[command(
        about = "Reindex the vector index for semantic search",
        after_help = "Examples:\n  fastskill reindex\n  fastskill reindex --force"
    )]
    Reindex(reindex::ReindexArgs),

    /// Uninstall skills (only way to stop using skills, removes from manifest and local installation)
    #[command(
        about = "Uninstall skills (only way to stop using skills, removes from manifest and local installation)",
        after_help = "Uninstalls skills completely - this is the only way to stop using skills.\nRemoves from both local installation and project manifest (skill-project.toml).\nUpdates skills.lock to reflect the removal.\n\nFor manifest-managed projects: removes from [dependencies] and updates lock.\nFor local-only skills: removes from local installation only.\n\nExamples:\n  fastskill remove my-skill-id\n  fastskill remove skill-a skill-b --force"
    )]
    Remove(remove::RemoveArgs),

    /// Package skills into ZIP artifacts
    #[command(
        about = "Package skills into ZIP artifacts with versioning",
        after_help = "PRESETS (recommended):\n  fastskill package auto              # Auto-detect changed skills\n  fastskill package skill <id>        # Package specific skill(s)\n\nADVANCED OPTIONS:\n  fastskill package --detect-changes  # Hash-based change detection\n  fastskill package --git-diff base head  # Git-based change detection\n  fastskill package --skills id1 id2  # Package specific skills\n  fastskill package --force           # Package all skills"
    )]
    Package(package::PackageArgs),

    /// Publish artifacts to registry API or local folder
    #[command(
        about = "Publish artifacts to remote API or local folder",
        after_help = "TARGET MODES (mutually exclusive):\n  --api-url <url>     Publish to API endpoint\n  --local-dir <path>   Publish to local directory\n  --registry <name>    Use configured repository\n  (default)            Use FASTSKILL_API_URL environment variable\n\nWAIT BEHAVIOR:\n  By default, waits for API validation to complete (--wait=true)\n  Use --wait=false to return immediately after upload\n\nEXAMPLES:\n  fastskill publish --api-url https://example.com\n  fastskill publish --local-dir ./output\n  fastskill publish --registry official\n  fastskill publish --api-url https://example.com --wait=false"
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
        hide = true,
        about = "Manage repositories and browse/search the registry catalog (remote)",
        after_help = "Examples:\n  fastskill registry list-skills\n  fastskill search \"query\""
    )]
    Registry(registry::RegistryArgs),

    /// Manage repository list and browse remote skill catalog
    #[command(
        about = "Manage repository list and browse remote skill catalog.",
        after_help = "Repository Management:\n  fastskill repos add my-repo --repo-type local /path/to/skills\n  fastskill repos remove my-repo\n  fastskill repos info my-repo\n  fastskill repos test my-repo\n  fastskill repos refresh\n\nCatalog Browsing:\n  fastskill repos skills\n  fastskill repos show pptx\n  fastskill repos versions pptx"
    )]
    Repos(repos::ReposArgs),

    /// Resolve skills as machine-readable JSON with canonical paths and optional content
    #[command(
        about = "Resolve skills as machine-readable JSON with canonical paths and optional content",
        after_help = "Examples:\n  fastskill resolve \"text processing\"\n  fastskill resolve \"pptx\" --limit 3 --include-content preview\n  fastskill resolve \"query\" --include-content full"
    )]
    Resolve(resolve::ResolveArgs),

    /// Search skills by query with explicit scope flags (remote default)
    #[command(
        about = "Search skills by query with explicit scope flags (remote default)",
        after_help = "Examples:\n  fastskill search \"text processing\"\n  fastskill search \"text processing\" --local\n  fastskill search \"pptx\" --repository my-repo\n  fastskill search \"query\" -l 5 --json"
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

    /// Sync installed skills to the agent's metadata file
    #[command(
        about = "Sync installed skills to the agent's metadata file",
        after_help = "Examples:\n  fastskill sync\n  fastskill sync --yes\n  fastskill sync --agent claude\n  fastskill sync --agents-file custom.md"
    )]
    Sync(sync::SyncArgs),

    /// Manage skill sources (repositories): list, add, remove, show, update, test, refresh; create marketplace
    #[command(
        hide = true,
        visible_alias = "source",
        after_help = "Examples:\n  fastskill sources list\n  fastskill sources add my-repo --repo-type local /path/to/skills"
    )]
    Sources(sources::SourcesArgs),

    /// Update skills to latest versions (behavior matrix: affects manifest, lock, and/or installed state)
    #[command(
        about = "Update skills to latest versions (behavior matrix: affects manifest, lock, and/or installed state)",
        after_help = "Behavior Matrix:\n- 'fastskill update' (no args): Updates all skills, modifies lock file\n- 'fastskill update <skill-id>': Updates specific skill, modifies lock file  \n- 'fastskill update --check': Check-only mode, no modifications\n- 'fastskill update --dry-run': Preview changes without applying\n\nRelated to 'fastskill install --lock' for applying lock-only updates.\n\nExamples:\n  fastskill update\n  fastskill update my-skill\n  fastskill update --check"
    )]
    Update(update::UpdateArgs),

    /// Display FastSkill version
    #[command(
        about = "Display FastSkill version",
        after_help = "Examples:\n  fastskill version"
    )]
    Version(version::VersionArgs),
}
