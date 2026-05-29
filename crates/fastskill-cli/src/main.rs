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
mod utils;

use cli_framework::prelude::AppBuilder;
use context::{FsCtx, FsState};
use std::path::PathBuf;
use std::sync::Arc;

use commands::{
    add, analyze, auth, disable, eval, init, install, list, marketplace, package, publish, read,
    reindex, remove, repos, resolve, search, serve, show, sync, update,
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
            if i + 1 < args.len() {
                i += 2;
            } else {
                i += 1;
            }
        } else if arg.starts_with("--repositories-path=") {
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

    let builder = match build_app(AppBuilder::new(), Arc::clone(&state)) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("Error building app: {}", e);
            std::process::exit(1);
        }
    };

    let mut app = match builder
        .with_version("fastskill", fastskill_core::VERSION)
        .with_git_sha_short(None)
        .build(ctx)
    {
        Ok(a) => a,
        Err(e) => {
            eprintln!("Error initialising app: {}", e);
            std::process::exit(1);
        }
    };

    match app.run_with_args(remaining_args).await {
        Ok(()) => std::process::exit(0),
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    }
}

fn build_app(builder: AppBuilder, state: Arc<FsState>) -> anyhow::Result<AppBuilder> {
    use cli_framework::prelude::Command;

    let builder = builder
        // ── No-service commands ──────────────────────────────────────────────
        .register_command({
            let state = Arc::clone(&state);
            Command {
                id: "init",
                summary: "Initialize skill-project.toml in current skill directory",
                syntax: Some("init [OPTIONS]"),
                category: Some("project"),
                spec: None,
                validator: None,
                expose_mcp: false,
                execute: Arc::new(move |_ctx, _args| {
                    let state = Arc::clone(&state);
                    Box::pin(async move {
                        let raw = state.raw_remaining_args.clone();
                        let args = parse_from_args::<init::InitArgs>(&raw[1..])?;
                        init::execute_init(args).await?;
                        Ok(())
                    })
                }),
            }
        })?
        .register_command({
            let state = Arc::clone(&state);
            Command {
                id: "install",
                summary: "Apply manifest: install skills from skill-project.toml [dependencies]",
                syntax: Some("install [OPTIONS]"),
                category: Some("packages"),
                spec: None,
                validator: None,
                expose_mcp: false,
                execute: Arc::new(move |_ctx, _args| {
                    let state = Arc::clone(&state);
                    Box::pin(async move {
                        let raw = state.raw_remaining_args.clone();
                        let args = parse_from_args::<install::InstallArgs>(&raw[1..])?;
                        install::execute_install(args).await?;
                        Ok(())
                    })
                }),
            }
        })?
        .register_command({
            let state = Arc::clone(&state);
            Command {
                id: "update",
                summary: "Update skills to latest versions",
                syntax: Some("update [SKILL_ID] [OPTIONS]"),
                category: Some("packages"),
                spec: None,
                validator: None,
                expose_mcp: false,
                execute: Arc::new(move |_ctx, _args| {
                    let state = Arc::clone(&state);
                    Box::pin(async move {
                        let raw = state.raw_remaining_args.clone();
                        let args = parse_from_args::<update::UpdateArgs>(&raw[1..])?;
                        update::execute_update(args, state.global).await?;
                        Ok(())
                    })
                }),
            }
        })?
        .register_command({
            let state = Arc::clone(&state);
            Command {
                id: "show",
                summary: "Show metadata for one or all installed skills",
                syntax: Some("show [SKILL_ID] [OPTIONS]"),
                category: Some("discovery"),
                spec: None,
                validator: None,
                expose_mcp: true,
                execute: Arc::new(move |_ctx, _args| {
                    let state = Arc::clone(&state);
                    Box::pin(async move {
                        let raw = state.raw_remaining_args.clone();
                        let args = parse_from_args::<show::ShowArgs>(&raw[1..])?;
                        show::execute_show(args, state.global).await?;
                        Ok(())
                    })
                }),
            }
        })?
        .register_command({
            let state = Arc::clone(&state);
            Command {
                id: "package",
                summary: "Package skills into ZIP artifacts with versioning",
                syntax: Some("package [OPTIONS]"),
                category: Some("publishing"),
                spec: None,
                validator: None,
                expose_mcp: false,
                execute: Arc::new(move |_ctx, _args| {
                    let state = Arc::clone(&state);
                    Box::pin(async move {
                        let raw = state.raw_remaining_args.clone();
                        let args = parse_from_args::<package::PackageArgs>(&raw[1..])?;
                        package::execute_package(args).await?;
                        Ok(())
                    })
                }),
            }
        })?
        .register_command({
            let state = Arc::clone(&state);
            Command {
                id: "publish",
                summary: "Publish artifacts to remote API or local folder",
                syntax: Some("publish [OPTIONS]"),
                category: Some("publishing"),
                spec: None,
                validator: None,
                expose_mcp: false,
                execute: Arc::new(move |_ctx, _args| {
                    let state = Arc::clone(&state);
                    Box::pin(async move {
                        let raw = state.raw_remaining_args.clone();
                        let args = parse_from_args::<publish::PublishArgs>(&raw[1..])?;
                        publish::execute_publish(args).await?;
                        Ok(())
                    })
                }),
            }
        })?
        // ── Subcommand-tree commands (no service) ───────────────────────────
        .register_command({
            let state = Arc::clone(&state);
            Command {
                id: "repos",
                summary: "Manage repository list and browse remote skill catalog",
                syntax: Some("repos <SUBCOMMAND>"),
                category: Some("repositories"),
                spec: None,
                validator: None,
                expose_mcp: true,
                execute: Arc::new(move |_ctx, _args| {
                    let state = Arc::clone(&state);
                    Box::pin(async move {
                        let raw = state.raw_remaining_args.clone();
                        let args = parse_from_args::<repos::ReposArgs>(&raw[1..])?;
                        repos::execute_repos(args).await?;
                        Ok(())
                    })
                }),
            }
        })?
        .register_command({
            let state = Arc::clone(&state);
            Command {
                id: "marketplace",
                summary: "Create and manage skill marketplace artifacts",
                syntax: Some("marketplace <SUBCOMMAND>"),
                category: Some("publishing"),
                spec: None,
                validator: None,
                expose_mcp: true,
                execute: Arc::new(move |_ctx, _args| {
                    let state = Arc::clone(&state);
                    Box::pin(async move {
                        let raw = state.raw_remaining_args.clone();
                        let args = parse_from_args::<marketplace::MarketplaceArgs>(&raw[1..])?;
                        marketplace::execute_marketplace(args).await?;
                        Ok(())
                    })
                }),
            }
        })?
        .register_command({
            let state = Arc::clone(&state);
            Command {
                id: "auth",
                summary: "Manage authentication for registries",
                syntax: Some("auth <login|logout|whoami>"),
                category: Some("auth"),
                spec: None,
                validator: None,
                expose_mcp: false,
                execute: Arc::new(move |_ctx, _args| {
                    let state = Arc::clone(&state);
                    Box::pin(async move {
                        let raw = state.raw_remaining_args.clone();
                        let args = parse_from_args::<auth::AuthArgs>(&raw[1..])?;
                        auth::execute_auth(args).await?;
                        Ok(())
                    })
                }),
            }
        })?
        .register_command({
            let state = Arc::clone(&state);
            Command {
                id: "eval",
                summary: "Evaluation commands for skill quality assurance",
                syntax: Some("eval <run|validate>"),
                category: Some("quality"),
                spec: None,
                validator: None,
                expose_mcp: true,
                execute: Arc::new(move |_ctx, _args| {
                    let state = Arc::clone(&state);
                    Box::pin(async move {
                        let raw = state.raw_remaining_args.clone();
                        let args = parse_from_args::<eval::EvalCommand>(&raw[1..])?;
                        eval::execute_eval(args).await?;
                        Ok(())
                    })
                }),
            }
        })?
        // ── Service commands ─────────────────────────────────────────────────
        .register_command({
            let state = Arc::clone(&state);
            Command {
                id: "add",
                summary: "Add a skill (from local path, zip, git URL, or registry ID)",
                syntax: Some("add <SOURCE> [OPTIONS]"),
                category: Some("packages"),
                spec: None,
                validator: None,
                expose_mcp: false,
                execute: Arc::new(move |_ctx, _args| {
                    let state = Arc::clone(&state);
                    Box::pin(async move {
                        let svc = state.service().await?;
                        let raw = state.raw_remaining_args.clone();
                        let args = parse_from_args::<add::AddArgs>(&raw[1..])?;
                        add::execute_add(&svc, args, state.global).await?;
                        Ok(())
                    })
                }),
            }
        })?
        .register_command({
            let state = Arc::clone(&state);
            Command {
                id: "analyze",
                summary: "Diagnostic and analysis commands",
                syntax: Some("analyze <matrix|duplicates|cluster>"),
                category: Some("analysis"),
                spec: None,
                validator: None,
                expose_mcp: true,
                execute: Arc::new(move |_ctx, _args| {
                    let state = Arc::clone(&state);
                    Box::pin(async move {
                        let svc = state.service().await?;
                        let raw = state.raw_remaining_args.clone();
                        let args = parse_from_args::<analyze::AnalyzeCommand>(&raw[1..])?;
                        analyze::execute_analyze(&svc, args).await?;
                        Ok(())
                    })
                }),
            }
        })?
        .register_command({
            let state = Arc::clone(&state);
            Command {
                id: "disable",
                summary: "Disable skills by ID",
                syntax: Some("disable <SKILL_ID>..."),
                category: Some("packages"),
                spec: None,
                validator: None,
                expose_mcp: false,
                execute: Arc::new(move |_ctx, _args| {
                    let state = Arc::clone(&state);
                    Box::pin(async move {
                        let svc = state.service().await?;
                        let raw = state.raw_remaining_args.clone();
                        let args = parse_from_args::<disable::DisableArgs>(&raw[1..])?;
                        disable::execute_disable(&svc, args).await?;
                        Ok(())
                    })
                }),
            }
        })?
        .register_command({
            let state = Arc::clone(&state);
            Command {
                id: "list",
                summary: "List installed skills and reconcile with skill-project.toml",
                syntax: Some("list [OPTIONS]"),
                category: Some("discovery"),
                spec: None,
                validator: None,
                expose_mcp: true,
                execute: Arc::new(move |_ctx, _args| {
                    let state = Arc::clone(&state);
                    Box::pin(async move {
                        let svc = state.service().await?;
                        let raw = state.raw_remaining_args.clone();
                        let args = parse_from_args::<list::ListArgs>(&raw[1..])?;
                        list::execute_list(&svc, args, state.global).await?;
                        Ok(())
                    })
                }),
            }
        })?
        .register_command({
            let state = Arc::clone(&state);
            Command {
                id: "read",
                summary: "Stream skill documentation (SKILL.md content) to stdout",
                syntax: Some("read <SKILL_ID>"),
                category: Some("discovery"),
                spec: None,
                validator: None,
                expose_mcp: true,
                execute: Arc::new(move |_ctx, _args| {
                    let state = Arc::clone(&state);
                    Box::pin(async move {
                        let svc = state.service().await?;
                        let raw = state.raw_remaining_args.clone();
                        let args = parse_from_args::<read::ReadArgs>(&raw[1..])?;
                        read::execute_read(svc, args).await?;
                        Ok(())
                    })
                }),
            }
        })?
        .register_command({
            let state = Arc::clone(&state);
            Command {
                id: "sync",
                summary: "Sync installed skills to the agent's metadata file",
                syntax: Some("sync [OPTIONS]"),
                category: Some("packages"),
                spec: None,
                validator: None,
                expose_mcp: false,
                execute: Arc::new(move |_ctx, _args| {
                    let state = Arc::clone(&state);
                    Box::pin(async move {
                        let svc = state.service().await?;
                        let raw = state.raw_remaining_args.clone();
                        let args = parse_from_args::<sync::SyncArgs>(&raw[1..])?;
                        sync::execute_sync(&svc, args).await?;
                        Ok(())
                    })
                }),
            }
        })?
        .register_command({
            let state = Arc::clone(&state);
            Command {
                id: "reindex",
                summary: "Reindex the vector index for semantic search",
                syntax: Some("reindex [OPTIONS]"),
                category: Some("packages"),
                spec: None,
                validator: None,
                expose_mcp: false,
                execute: Arc::new(move |_ctx, _args| {
                    let state = Arc::clone(&state);
                    Box::pin(async move {
                        let svc = state.service().await?;
                        let raw = state.raw_remaining_args.clone();
                        let args = parse_from_args::<reindex::ReindexArgs>(&raw[1..])?;
                        reindex::execute_reindex(&svc, args).await?;
                        Ok(())
                    })
                }),
            }
        })?
        .register_command({
            let state = Arc::clone(&state);
            Command {
                id: "remove",
                summary: "Uninstall skills (removes from manifest and local installation)",
                syntax: Some("remove <SKILL_ID>... [OPTIONS]"),
                category: Some("packages"),
                spec: None,
                validator: None,
                expose_mcp: false,
                execute: Arc::new(move |_ctx, _args| {
                    let state = Arc::clone(&state);
                    Box::pin(async move {
                        let svc = state.service().await?;
                        let raw = state.raw_remaining_args.clone();
                        let args = parse_from_args::<remove::RemoveArgs>(&raw[1..])?;
                        remove::execute_remove(&svc, args, state.global).await?;
                        Ok(())
                    })
                }),
            }
        })?
        .register_command({
            let state = Arc::clone(&state);
            Command {
                id: "resolve",
                summary: "Resolve skills as machine-readable JSON with canonical paths",
                syntax: Some("resolve <QUERY> [OPTIONS]"),
                category: Some("discovery"),
                spec: None,
                validator: None,
                expose_mcp: true,
                execute: Arc::new(move |_ctx, _args| {
                    let state = Arc::clone(&state);
                    Box::pin(async move {
                        let svc = state.service().await?;
                        let raw = state.raw_remaining_args.clone();
                        let args = parse_from_args::<resolve::ResolveArgs>(&raw[1..])?;
                        resolve::execute_resolve(&svc, args).await?;
                        Ok(())
                    })
                }),
            }
        })?
        .register_command({
            let state = Arc::clone(&state);
            Command {
                id: "search",
                summary: "Search skills by query with explicit scope flags",
                syntax: Some("search <QUERY> [--local|--remote] [OPTIONS]"),
                category: Some("discovery"),
                spec: None,
                validator: None,
                expose_mcp: true,
                execute: Arc::new(move |_ctx, _args| {
                    let state = Arc::clone(&state);
                    Box::pin(async move {
                        let svc = state.service().await?;
                        let raw = state.raw_remaining_args.clone();
                        let args = parse_from_args::<search::SearchArgs>(&raw[1..])?;
                        search::execute_search(&svc, args).await?;
                        Ok(())
                    })
                }),
            }
        })?
        .register_command({
            let state = Arc::clone(&state);
            Command {
                id: "serve",
                summary: "Start the FastSkill HTTP API server",
                syntax: Some("serve [OPTIONS]"),
                category: Some("server"),
                spec: None,
                validator: None,
                expose_mcp: false,
                execute: Arc::new(move |_ctx, _args| {
                    let state = Arc::clone(&state);
                    Box::pin(async move {
                        let svc = state.service().await?;
                        let raw = state.raw_remaining_args.clone();
                        let args = parse_from_args::<serve::ServeArgs>(&raw[1..])?;
                        serve::execute_serve(svc, args).await?;
                        Ok(())
                    })
                }),
            }
        })?;

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
