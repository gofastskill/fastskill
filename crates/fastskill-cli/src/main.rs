//! FastSkill CLI binary entry point — cli-framework AppBuilder edition.
//!
//! Stage 1: all 21 active (non-deprecated, non-framework) commands are
//! registered as bridged commands (`spec: None`).  Each bridge closure
//! reconstructs the original typed `Args` struct from `FsState.raw_remaining_args`
//! and delegates to the existing `execute_*` functions, preserving all
//! existing behaviour.
//!
//! `Arc<FsState>` is captured at registration time by each command closure —
//! no `Any`-downcasting of `AppContext` is needed.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod arg_helpers;
mod auth_config;
mod commands;
mod config;
mod config_file;
mod context;
mod error;
pub mod runtime_selector;
mod utils;

use cli_framework::prelude::{AppBuilder, Command};
use context::{FsCtx, FsState};
use std::path::PathBuf;
use std::sync::Arc;

/// Register a command into the AppBuilder, rebinding `$builder` on success.
///
/// Calling conventions (last token(s) select how the execute fn is invoked):
///   (none)       — no service, `execute_fn(args)`
///   `global`     — no service, `execute_fn(args, state.global)`
///   `svc`        — service (&), `execute_fn(&svc, args)`
///   `svc_global` — service (&), `execute_fn(&svc, args, state.global)`
///   `svc_val`    — service (value), `execute_fn(svc, args)`
macro_rules! register_cmd {
    // No service, args only
    ($builder:ident, $id:literal, $summary:literal, $syntax:expr, $category:expr,
     $expose_mcp:literal, $args_type:ty, $state:expr,
     $module:ident, $fn:ident) => {
        let $builder = $builder.register_command({
            let state = Arc::clone(&$state);
            Command {
                id: $id,
                summary: $summary,
                syntax: $syntax,
                category: $category,
                spec: None,
                validator: None,
                expose_mcp: $expose_mcp,
                execute: Arc::new(move |_ctx, _args| {
                    let state = Arc::clone(&state);
                    Box::pin(async move {
                        let raw = state.raw_remaining_args.clone();
                        let args = parse_from_args::<$args_type>(&raw[1..])?;
                        $module::$fn(args).await?;
                        Ok(())
                    })
                }),
            }
        })?;
    };
    // No service, args + state.global
    ($builder:ident, $id:literal, $summary:literal, $syntax:expr, $category:expr,
     $expose_mcp:literal, $args_type:ty, $state:expr,
     $module:ident, $fn:ident, global) => {
        let $builder = $builder.register_command({
            let state = Arc::clone(&$state);
            Command {
                id: $id,
                summary: $summary,
                syntax: $syntax,
                category: $category,
                spec: None,
                validator: None,
                expose_mcp: $expose_mcp,
                execute: Arc::new(move |_ctx, _args| {
                    let state = Arc::clone(&state);
                    Box::pin(async move {
                        let raw = state.raw_remaining_args.clone();
                        let args = parse_from_args::<$args_type>(&raw[1..])?;
                        $module::$fn(args, state.global).await?;
                        Ok(())
                    })
                }),
            }
        })?;
    };
    // Service (&), args only
    ($builder:ident, $id:literal, $summary:literal, $syntax:expr, $category:expr,
     $expose_mcp:literal, $args_type:ty, $state:expr,
     $module:ident, $fn:ident, svc) => {
        let $builder = $builder.register_command({
            let state = Arc::clone(&$state);
            Command {
                id: $id,
                summary: $summary,
                syntax: $syntax,
                category: $category,
                spec: None,
                validator: None,
                expose_mcp: $expose_mcp,
                execute: Arc::new(move |_ctx, _args| {
                    let state = Arc::clone(&state);
                    Box::pin(async move {
                        let svc = state.service().await?;
                        let raw = state.raw_remaining_args.clone();
                        let args = parse_from_args::<$args_type>(&raw[1..])?;
                        $module::$fn(&svc, args).await?;
                        Ok(())
                    })
                }),
            }
        })?;
    };
    // Service (&), args + state.global
    ($builder:ident, $id:literal, $summary:literal, $syntax:expr, $category:expr,
     $expose_mcp:literal, $args_type:ty, $state:expr,
     $module:ident, $fn:ident, svc_global) => {
        let $builder = $builder.register_command({
            let state = Arc::clone(&$state);
            Command {
                id: $id,
                summary: $summary,
                syntax: $syntax,
                category: $category,
                spec: None,
                validator: None,
                expose_mcp: $expose_mcp,
                execute: Arc::new(move |_ctx, _args| {
                    let state = Arc::clone(&state);
                    Box::pin(async move {
                        let svc = state.service().await?;
                        let raw = state.raw_remaining_args.clone();
                        let args = parse_from_args::<$args_type>(&raw[1..])?;
                        $module::$fn(&svc, args, state.global).await?;
                        Ok(())
                    })
                }),
            }
        })?;
    };
    // Service (value), args only
    ($builder:ident, $id:literal, $summary:literal, $syntax:expr, $category:expr,
     $expose_mcp:literal, $args_type:ty, $state:expr,
     $module:ident, $fn:ident, svc_val) => {
        let $builder = $builder.register_command({
            let state = Arc::clone(&$state);
            Command {
                id: $id,
                summary: $summary,
                syntax: $syntax,
                category: $category,
                spec: None,
                validator: None,
                expose_mcp: $expose_mcp,
                execute: Arc::new(move |_ctx, _args| {
                    let state = Arc::clone(&state);
                    Box::pin(async move {
                        let svc = state.service().await?;
                        let raw = state.raw_remaining_args.clone();
                        let args = parse_from_args::<$args_type>(&raw[1..])?;
                        $module::$fn(svc, args).await?;
                        Ok(())
                    })
                }),
            }
        })?;
    };
}

fn or_exit<T, E: std::fmt::Display>(result: Result<T, E>, msg: &str) -> T {
    match result {
        Ok(v) => v,
        Err(e) => {
            eprintln!("{}: {}", msg, e);
            std::process::exit(1);
        }
    }
}

use commands::{
    add, analyze, auth, disable, eval, init, install, list, marketplace, package, publish, read,
    reindex, remove, repos, resolve, search, serve, show, skillopt, sync, update,
};

pub fn strip_global_flags(args: Vec<String>) -> (Option<PathBuf>, bool, bool, Vec<String>) {
    let mut skills_dir: Option<PathBuf> = None;
    let mut global = false;
    let mut verbose = false;
    let mut remaining: Vec<String> = Vec::with_capacity(args.len());
    let mut i = 0;

    while i < args.len() {
        let arg = &args[i];

        if arg == "--skills-dir" {
            if i + 1 < args.len() && !args[i + 1].starts_with('-') {
                skills_dir = Some(PathBuf::from(&args[i + 1]));
                i += 2;
            } else if i + 1 < args.len() {
                eprintln!(
                    "Error: --skills-dir requires a path value (got '{}' which looks like a flag)",
                    args[i + 1]
                );
                std::process::exit(1);
            } else {
                eprintln!(
                    "Error: --skills-dir requires a path value but appears at end of arguments"
                );
                std::process::exit(1);
            }
        } else if let Some(path) = arg.strip_prefix("--skills-dir=") {
            skills_dir = Some(PathBuf::from(path));
            i += 1;
        } else if arg == "--global" {
            global = true;
            i += 1;
        } else if arg == "--verbose" || arg == "-v" {
            verbose = true;
            i += 1;
        } else if arg == "--repositories-path" {
            // Accepted for backward compatibility with older callers; value is intentionally ignored.
            if i + 1 < args.len() {
                i += 2;
            } else {
                i += 1;
            }
        } else if arg.starts_with("--repositories-path=") {
            // Accepted for backward compatibility; value is intentionally ignored.
            i += 1;
        } else {
            remaining.push(arg.clone());
            i += 1;
        }
    }

    (skills_dir, global, verbose, remaining)
}

fn parse_from_args<T: clap::Args>(raw: &[String]) -> anyhow::Result<T> {
    let cmd = T::augment_args(clap::Command::new("fastskill"));
    let matches = cmd
        .try_get_matches_from(raw.iter().map(|s| s.as_str()))
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    T::from_arg_matches(&matches).map_err(|e| anyhow::anyhow!("{}", e))
}

#[tokio::main]
async fn main() {
    let raw: Vec<String> = std::env::args().collect();
    let (skills_dir, global, verbose, remaining_args) = strip_global_flags(raw);

    fastskill_core::init_logging_with_verbose(verbose);

    let state = Arc::new(FsState::new(
        skills_dir,
        global,
        verbose,
        remaining_args.clone(),
    ));
    let ctx = FsCtx;

    let builder = or_exit(
        build_app(AppBuilder::new(), Arc::clone(&state)),
        "Error building app",
    );

    let mut app = or_exit(
        builder
            .with_version("fastskill", fastskill_core::VERSION)
            .with_git_sha_short(None)
            .build(ctx),
        "Error initialising app",
    );

    match app.run_with_args(remaining_args).await {
        Ok(()) => std::process::exit(0),
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    }
}

fn build_app(builder: AppBuilder, state: Arc<FsState>) -> anyhow::Result<AppBuilder> {
    // ── No-service commands ──────────────────────────────────────────────────
    register_cmd!(
        builder,
        "init",
        "Initialize skill-project.toml in current skill directory",
        Some("init [OPTIONS]"),
        Some("project"),
        false,
        init::InitArgs,
        state,
        init,
        execute_init
    );
    register_cmd!(
        builder,
        "install",
        "Apply manifest: install skills from skill-project.toml [dependencies]",
        Some("install [OPTIONS]"),
        Some("packages"),
        false,
        install::InstallArgs,
        state,
        install,
        execute_install
    );
    register_cmd!(
        builder,
        "update",
        "Update skills to latest versions",
        Some("update [SKILL_ID] [OPTIONS]"),
        Some("packages"),
        false,
        update::UpdateArgs,
        state,
        update,
        execute_update,
        global
    );
    register_cmd!(
        builder,
        "show",
        "Show metadata for one or all installed skills",
        Some("show [SKILL_ID] [OPTIONS]"),
        Some("discovery"),
        true,
        show::ShowArgs,
        state,
        show,
        execute_show,
        global
    );
    register_cmd!(
        builder,
        "package",
        "Package skills into ZIP artifacts with versioning",
        Some("package [OPTIONS]"),
        Some("publishing"),
        false,
        package::PackageArgs,
        state,
        package,
        execute_package
    );
    register_cmd!(
        builder,
        "publish",
        "Publish artifacts to remote API or local folder",
        Some("publish [OPTIONS]"),
        Some("publishing"),
        false,
        publish::PublishArgs,
        state,
        publish,
        execute_publish
    );
    // ── Subcommand-tree commands (no service) ────────────────────────────────
    register_cmd!(
        builder,
        "repos",
        "Manage repository list and browse remote skill catalog",
        Some("repos <SUBCOMMAND>"),
        Some("repositories"),
        true,
        repos::ReposArgs,
        state,
        repos,
        execute_repos
    );
    register_cmd!(
        builder,
        "marketplace",
        "Create and manage skill marketplace artifacts",
        Some("marketplace <SUBCOMMAND>"),
        Some("publishing"),
        true,
        marketplace::MarketplaceArgs,
        state,
        marketplace,
        execute_marketplace
    );
    register_cmd!(
        builder,
        "auth",
        "Manage authentication for registries",
        Some("auth <login|logout|whoami>"),
        Some("auth"),
        false,
        auth::AuthArgs,
        state,
        auth,
        execute_auth
    );
    register_cmd!(
        builder,
        "eval",
        "Evaluation commands for skill quality assurance",
        Some("eval <run|validate>"),
        Some("quality"),
        true,
        eval::EvalCommand,
        state,
        eval,
        execute_eval
    );
    register_cmd!(
        builder,
        "skillopt",
        "Iterative skill-document optimization via text-gradient",
        Some("skillopt <run|resume|status|inspect|export>"),
        Some("quality"),
        false,
        skillopt::SkillOptCommand,
        state,
        skillopt,
        execute_skillopt
    );
    // ── Service commands ─────────────────────────────────────────────────────
    register_cmd!(
        builder,
        "add",
        "Add a skill (from local path, zip, git URL, or registry ID)",
        Some("add <SOURCE> [OPTIONS]"),
        Some("packages"),
        false,
        add::AddArgs,
        state,
        add,
        execute_add,
        svc_global
    );
    register_cmd!(
        builder,
        "analyze",
        "Diagnostic and analysis commands",
        Some("analyze <matrix|duplicates|cluster>"),
        Some("analysis"),
        true,
        analyze::AnalyzeCommand,
        state,
        analyze,
        execute_analyze,
        svc
    );
    register_cmd!(
        builder,
        "disable",
        "Disable skills by ID",
        Some("disable <SKILL_ID>..."),
        Some("packages"),
        false,
        disable::DisableArgs,
        state,
        disable,
        execute_disable,
        svc
    );
    register_cmd!(
        builder,
        "list",
        "List installed skills and reconcile with skill-project.toml",
        Some("list [OPTIONS]"),
        Some("discovery"),
        true,
        list::ListArgs,
        state,
        list,
        execute_list,
        svc_global
    );
    register_cmd!(
        builder,
        "read",
        "Stream skill documentation (SKILL.md content) to stdout",
        Some("read <SKILL_ID>"),
        Some("discovery"),
        true,
        read::ReadArgs,
        state,
        read,
        execute_read,
        svc_val
    );
    register_cmd!(
        builder,
        "sync",
        "Sync installed skills to the agent's metadata file",
        Some("sync [OPTIONS]"),
        Some("packages"),
        false,
        sync::SyncArgs,
        state,
        sync,
        execute_sync,
        svc
    );
    register_cmd!(
        builder,
        "reindex",
        "Reindex the vector index for semantic search",
        Some("reindex [OPTIONS]"),
        Some("packages"),
        false,
        reindex::ReindexArgs,
        state,
        reindex,
        execute_reindex,
        svc
    );
    register_cmd!(
        builder,
        "remove",
        "Uninstall skills (removes from manifest and local installation)",
        Some("remove <SKILL_ID>... [OPTIONS]"),
        Some("packages"),
        false,
        remove::RemoveArgs,
        state,
        remove,
        execute_remove,
        svc_global
    );
    register_cmd!(
        builder,
        "resolve",
        "Resolve skills as machine-readable JSON with canonical paths",
        Some("resolve <QUERY> [OPTIONS]"),
        Some("discovery"),
        true,
        resolve::ResolveArgs,
        state,
        resolve,
        execute_resolve,
        svc
    );
    register_cmd!(
        builder,
        "search",
        "Search skills by query with explicit scope flags",
        Some("search <QUERY> [--local|--remote] [OPTIONS]"),
        Some("discovery"),
        true,
        search::SearchArgs,
        state,
        search,
        execute_search,
        svc
    );
    register_cmd!(
        builder,
        "serve",
        "Start the FastSkill HTTP API server",
        Some("serve [OPTIONS]"),
        Some("server"),
        false,
        serve::ServeArgs,
        state,
        serve,
        execute_serve,
        svc_val
    );

    Ok(builder)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_skills_dir_equals_form() {
        let args = vec![
            "fastskill".to_string(),
            "--skills-dir=/tmp/skills".to_string(),
            "search".to_string(),
            "query".to_string(),
        ];
        let (skills_dir, global, verbose, remaining) = strip_global_flags(args);
        assert_eq!(skills_dir, Some(PathBuf::from("/tmp/skills")));
        assert!(!global);
        assert!(!verbose);
        assert_eq!(remaining, vec!["fastskill", "search", "query"]);
    }

    #[test]
    fn test_strip_skills_dir_space_form() {
        let args = vec![
            "fastskill".to_string(),
            "--skills-dir".to_string(),
            "/tmp/skills".to_string(),
            "search".to_string(),
        ];
        let (skills_dir, _global, _verbose, remaining) = strip_global_flags(args);
        assert_eq!(skills_dir, Some(PathBuf::from("/tmp/skills")));
        assert_eq!(remaining, vec!["fastskill", "search"]);
    }

    #[test]
    fn test_strip_skills_dir_absent() {
        let args = vec![
            "fastskill".to_string(),
            "search".to_string(),
            "query".to_string(),
        ];
        let (skills_dir, _global, _verbose, remaining) = strip_global_flags(args.clone());
        assert_eq!(skills_dir, None);
        assert_eq!(remaining, args);
    }

    #[test]
    fn test_strip_global_flag() {
        let args = vec![
            "fastskill".to_string(),
            "--global".to_string(),
            "list".to_string(),
        ];
        let (_skills_dir, global, _verbose, remaining) = strip_global_flags(args);
        assert!(global);
        assert_eq!(remaining, vec!["fastskill", "list"]);
    }

    #[test]
    fn test_strip_verbose_flag() {
        let args = vec![
            "fastskill".to_string(),
            "-v".to_string(),
            "search".to_string(),
        ];
        let (_skills_dir, _global, verbose, remaining) = strip_global_flags(args);
        assert!(verbose);
        assert_eq!(remaining, vec!["fastskill", "search"]);
    }

    #[test]
    fn test_strip_verbose_long_flag() {
        let args = vec![
            "fastskill".to_string(),
            "--verbose".to_string(),
            "list".to_string(),
        ];
        let (_skills_dir, _global, verbose, remaining) = strip_global_flags(args);
        assert!(verbose);
        assert_eq!(remaining, vec!["fastskill", "list"]);
    }

    #[test]
    fn test_fsstate_new_stores_fields() {
        let state = FsState::new(
            Some(PathBuf::from("/tmp/skills")),
            true,
            true,
            vec!["fastskill".to_string(), "list".to_string()],
        );
        assert_eq!(
            state.skills_dir_override,
            Some(PathBuf::from("/tmp/skills"))
        );
        assert!(state.global);
        assert!(state.verbose);
        assert_eq!(state.raw_remaining_args, vec!["fastskill", "list"]);
    }

    #[test]
    fn test_fsctx_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<FsCtx>();
    }
}
