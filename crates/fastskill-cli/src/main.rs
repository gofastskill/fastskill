//! FastSkill CLI binary entry point — cli-framework AppBuilder edition.
//!
//! All commands are registered as typed `builder.register` calls that use
//! `IntoCommandSpec + FromArgValueMap`. Global flags (--skills-dir, --global,
//! --verbose) are declared via `builder.global_flag` and read at dispatch time
//! through `ctx.opt_global_args()`.
//!
//! `Arc<FsState>` is captured at registration time by each command closure —
//! no `Any`-downcasting of `AppContext` is needed.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod arg_helpers;
mod commands;
mod config;
mod config_file;
mod context;
mod dispatch_context;
mod error;
mod json_boundary;
mod output;
mod registration;
pub mod runtime_selector;
mod utils;

use cli_framework::prelude::AppBuilder;
use context::{FsCtx, FsState};
use dispatch_context::{global as ctx_global, skills_directory as ctx_skills_dir};
use json_boundary::{
    emit_json_error, emit_lifecycle_json_error, is_json_lifecycle, requests_json_output,
};
use std::sync::Arc;

fn or_exit<T, E: std::fmt::Display>(result: Result<T, E>, msg: &str) -> T {
    match result {
        Ok(v) => v,
        Err(e) => {
            eprintln!("{}: {}", msg, e);
            std::process::exit(1);
        }
    }
}

/// Whether this invocation is `fastskill mcp serve`, ignoring any flags that
/// appear between or around the two path segments.
fn is_mcp_serve(args: &[String]) -> bool {
    let positionals: Vec<&str> = args
        .iter()
        .skip(1)
        .filter(|a| !a.starts_with('-'))
        .map(String::as_str)
        .collect();
    matches!(positionals.first(), Some(&"mcp")) && positionals.contains(&"serve")
}

use commands::{
    add, analyze, bundle, cache, doctor, eval, init, install, list, marketplace, mcp, read,
    reindex, remove, repos, search, serve, skillopt, update,
};

/// The binary's name, as reported by `--version` and used to derive MCP tool
/// names (`fastskill_<command>`).
const APP_NAME: &str = "fastskill";

#[tokio::main]
async fn main() {
    let raw: Vec<String> = std::env::args().collect();
    let verbose = raw.iter().any(|a| a == "--verbose" || a == "-v");
    fastskill_core::init_logging_with_verbose(verbose);

    // Under `mcp serve` the process speaks JSON-RPC on stdout, so command output
    // is buffered and returned as the tool result instead of being printed.
    output::init(if is_mcp_serve(&raw) {
        output::Mode::Capture
    } else {
        output::Mode::Direct
    });

    let state = Arc::new(FsState::new());
    let ctx = FsCtx;

    let builder = or_exit(
        build_app(AppBuilder::new(), Arc::clone(&state)),
        "Error building app",
    );

    let mut app = or_exit(
        builder
            .with_version(APP_NAME, fastskill_core::VERSION)
            .with_git_sha_short(None)
            // Honour `expose_mcp`. The framework default (`AllCommands`)
            // ignores it, which exported `serve` as a tool -- an MCP call that
            // starts a server and never returns.
            .with_mcp_export_policy(cli_framework::mcp::McpToolExportPolicy::ExposeMcpOnly)
            .build(ctx),
        "Error initialising app",
    );

    // `mcp serve` exports the other commands as MCP tools, so it needs the
    // registry it is itself registered in. Publish it once the app is built.
    commands::mcp::set_command_registry(Arc::new(app.command_registry().clone()));

    let json_output = requests_json_output(&raw);
    let lifecycle_json = is_json_lifecycle(&raw);
    let global_scope = raw.iter().any(|arg| arg == "--global");
    let dry_run = raw
        .iter()
        .any(|arg| matches!(arg.as_str(), "--dry-run" | "--check"));
    let (result, captured) = if json_output {
        output::capture(app.run_with_args(raw)).await
    } else {
        (app.run_with_args(raw).await, String::new())
    };

    match result {
        Ok(()) => {
            if json_output && !captured.trim().is_empty() {
                output::emit(captured.trim_end());
            }
            std::process::exit(0)
        }
        Err(e) => {
            if json_output {
                if serde_json::from_str::<serde_json::Value>(captured.trim()).is_ok() {
                    output::emit(captured.trim_end());
                } else if lifecycle_json {
                    emit_lifecycle_json_error(&e, global_scope, dry_run);
                } else {
                    emit_json_error(&e);
                }
                if !lifecycle_json {
                    eprintln!("Error: {e}");
                }
                std::process::exit(1);
            }
            // `run_with_args` already writes a structured diagnostic to stderr
            // (via `DiagnosticReporter`) for usage errors — parse failures,
            // validation failures, unknown nested commands — before returning
            // `Err(UsageError(..))`. Printing `e` again here would duplicate
            // that message. Mirror `cli_framework::app::App::run()`'s own
            // handling: only print when the error was *not* already reported.
            if e.downcast_ref::<cli_framework::app::UsageError>().is_none() {
                eprintln!("Error: {}", e);
            }
            std::process::exit(1);
        }
    }
}

fn build_app(builder: AppBuilder, state: Arc<FsState>) -> anyhow::Result<AppBuilder> {
    use crate::registration::AppBuilderExt;
    use cli_framework::path;
    let builder = crate::registration::surface::configure(builder)?;

    // ── Typed commands (no service) ──────────────────────────────────────────
    let builder = builder
        .register_group(
            &path!["skill"],
            cli_framework::spec::command_tree::GroupMetadata {
                summary: "Discover, read, and manage individual skills",
                hidden: false,
                category: Some("Skills and projects"),
                help_order: Some(10),
            },
        )?
        .register_group(
            &path!["project"],
            cli_framework::spec::command_tree::GroupMetadata {
                summary: "Initialize projects and install declared dependencies",
                hidden: false,
                category: Some("Skills and projects"),
                help_order: Some(30),
            },
        )?
        .register_out(path!["project", "init"], |ctx, args: init::InitArgs| {
            let skills_dir = ctx_skills_dir(ctx).map(|p| p.display().to_string());
            async move {
                init::execute_init(args.with_skills_dir(skills_dir))
                    .await
                    .map_err(anyhow::Error::from)
            }
        })?
        .register_out(
            path!["project", "install"],
            |ctx, args: install::InstallArgs| {
                let global = ctx_global(ctx);
                let skills_dir = ctx_skills_dir(ctx);
                async move {
                    install::execute_install_scoped(args, global, skills_dir)
                        .await
                        .map_err(anyhow::Error::from)
                }
            },
        )?
        .register_out(path!["skill", "update"], |ctx, args: update::UpdateArgs| {
            let global = ctx_global(ctx);
            let skills_dir = ctx_skills_dir(ctx);
            async move {
                update::execute_update(args, global, skills_dir)
                    .await
                    .map_err(anyhow::Error::from)
            }
        })?;

    let builder = {
        use cli_framework::spec::command_tree::GroupMetadata;
        let state_bundle = Arc::clone(&state);
        builder
            .register_group(
                &path!["bundle"],
                GroupMetadata {
                    summary: "Build and manage publishable skill bundles",
                    hidden: false,
                    category: Some("Skills and projects"),
                    help_order: Some(20),
                },
            )?
            .register_out(path!["bundle", "build"], |ctx, args: bundle::BuildArgs| {
                let skills_dir = ctx_skills_dir(ctx);
                let global = ctx_global(ctx);
                async move {
                    bundle::execute_build(args, skills_dir, global)
                        .await
                        .map_err(anyhow::Error::from)
                }
            })?
            .register_out(path!["bundle", "add"], {
                let state = Arc::clone(&state_bundle);
                move |ctx, args: bundle::add::AddArgs| {
                    let skills_dir = ctx_skills_dir(ctx);
                    let global = ctx_global(ctx);
                    let state = Arc::clone(&state);
                    async move {
                        let artifact = bundle::add::preflight_add(&args, global)
                            .await
                            .map_err(anyhow::Error::from)?;
                        let service = state.service_with(false, skills_dir).await?;
                        bundle::add::execute_add_preflighted(service.as_ref(), args, artifact)
                            .await
                            .map_err(anyhow::Error::from)
                    }
                }
            })?
            .register_out(path!["bundle", "list"], {
                let state = Arc::clone(&state_bundle);
                move |ctx, args: bundle::list::ListArgs| {
                    let skills_dir = ctx_skills_dir(ctx);
                    let global = ctx_global(ctx);
                    let state = Arc::clone(&state);
                    async move {
                        if global {
                            return Err(anyhow::Error::from(crate::error::CliError::Validation(
                                "Bundle operations require a project Manifest and do not support --global"
                                    .to_string(),
                            )));
                        }
                        let service = state.service_with(false, skills_dir).await?;
                        bundle::list::execute_list(service.as_ref(), args, global)
                            .await
                            .map_err(anyhow::Error::from)
                    }
                }
            })?
            .register_out(path!["bundle", "update"], {
                let state = Arc::clone(&state_bundle);
                move |ctx, args: bundle::update::UpdateArgs| {
                    let skills_dir = ctx_skills_dir(ctx);
                    let global = ctx_global(ctx);
                    let state = Arc::clone(&state);
                    async move {
                        if global {
                            return Err(anyhow::Error::from(crate::error::CliError::Validation(
                                "Bundle operations require a project Manifest and do not support --global"
                                    .to_string(),
                            )));
                        }
                        let service = state.service_with(false, skills_dir).await?;
                        bundle::update::execute_update(service.as_ref(), args, global)
                            .await
                            .map_err(anyhow::Error::from)
                    }
                }
            })?
            .register_out(path!["bundle", "remove"], {
                let state = Arc::clone(&state_bundle);
                move |ctx, args: bundle::remove::RemoveArgs| {
                    let skills_dir = ctx_skills_dir(ctx);
                    let global = ctx_global(ctx);
                    let state = Arc::clone(&state);
                    async move {
                        if global {
                            return Err(anyhow::Error::from(crate::error::CliError::Validation(
                                "Bundle operations require a project Manifest and do not support --global"
                                    .to_string(),
                            )));
                        }
                        let service = state.service_with(false, skills_dir).await?;
                        bundle::remove::execute_remove(service.as_ref(), args, global)
                            .await
                            .map_err(anyhow::Error::from)
                    }
                }
            })?
            .register_out(
                path!["bundle", "override"],
                move |ctx, args: bundle::OverrideArgs| {
                    let skills_dir = ctx_skills_dir(ctx);
                    let global = ctx_global(ctx);
                    let state = Arc::clone(&state_bundle);
                    async move {
                        if global {
                            return Err(anyhow::Error::from(crate::error::CliError::Validation(
                                "Bundle operations require a project Manifest and do not support --global"
                                    .to_string(),
                            )));
                        }
                        let service = state.service_with(false, skills_dir).await?;
                        bundle::execute_override(args, service.as_ref(), false)
                            .await
                            .map_err(anyhow::Error::from)
                    }
                },
            )?
    };

    // ── Typed commands that need FsState (service injection) ─────────────────
    let builder = {
        let state_list = Arc::clone(&state);
        let state_read = Arc::clone(&state);
        builder
            .register_out(path!["skill", "list"], move |ctx, args: list::ListArgs| {
                let global = ctx_global(ctx);
                let skills_dir = ctx_skills_dir(ctx);
                let state = Arc::clone(&state_list);
                async move {
                    if global && skills_dir.is_some() {
                        return Err(anyhow::anyhow!(
                            "--global and --skills-dir cannot be combined"
                        ));
                    }
                    let svc = state.service_with(global, skills_dir).await?;
                    list::execute_list(&svc, args, global)
                        .await
                        .map_err(anyhow::Error::from)
                }
            })?
            .register_out(path!["skill", "read"], move |ctx, args: read::ReadArgs| {
                let global = ctx_global(ctx);
                let skills_dir = ctx_skills_dir(ctx);
                let state = Arc::clone(&state_read);
                async move {
                    if global && skills_dir.is_some() {
                        return Err(anyhow::anyhow!(
                            "--global and --skills-dir cannot be combined"
                        ));
                    }
                    let svc = state.service_with(global, skills_dir).await?;
                    read::execute_read(svc, args, global)
                        .await
                        .map_err(anyhow::Error::from)
                }
            })?
    };

    // ── repos: fully migrated to typed API ───────────────────────────────────
    let builder = {
        use cli_framework::spec::command_tree::GroupMetadata;
        builder
            .register_group(
                &path!["repo"],
                GroupMetadata {
                    summary: "Manage repository list and browse remote skill catalog",
                    hidden: false,
                    category: Some("Sources and distribution"),
                    help_order: Some(10),
                },
            )?
            .register_out(
                path!["repo", "list"],
                |_ctx, args: repos::ReposListArgs| async move {
                    repos::execute_repos_list(args)
                        .await
                        .map_err(anyhow::Error::from)
                },
            )?
            .register_out(
                path!["repo", "add"],
                |_ctx, args: repos::ReposAddArgs| async move {
                    repos::execute_repos_add(args)
                        .await
                        .map_err(anyhow::Error::from)
                },
            )?
            .register_out(
                path!["repo", "remove"],
                |_ctx, args: repos::ReposRemoveArgs| async move {
                    repos::execute_repos_remove(args)
                        .await
                        .map_err(anyhow::Error::from)
                },
            )?
            .register_out(
                path!["repo", "info"],
                |_ctx, args: repos::ReposInfoArgs| async move {
                    repos::execute_repos_info(args)
                        .await
                        .map_err(anyhow::Error::from)
                },
            )?
            .register_out(
                path!["repo", "update"],
                |_ctx, args: repos::ReposUpdateArgs| async move {
                    repos::execute_repos_update(args)
                        .await
                        .map_err(anyhow::Error::from)
                },
            )?
            .register_out(
                path!["repo", "test"],
                |_ctx, args: repos::ReposTestArgs| async move {
                    repos::execute_repos_test(args)
                        .await
                        .map_err(anyhow::Error::from)
                },
            )?
            .register_out(
                path!["repo", "refresh"],
                |_ctx, args: repos::ReposRefreshArgs| async move {
                    repos::execute_repos_refresh(args)
                        .await
                        .map_err(anyhow::Error::from)
                },
            )?
            .register_out(
                path!["repo", "skills"],
                |_ctx, args: repos::ReposSkillsArgs| async move {
                    repos::execute_repos_skills(args)
                        .await
                        .map_err(anyhow::Error::from)
                },
            )?
            .register_out(
                path!["repo", "show"],
                |_ctx, args: repos::ReposShowArgs| async move {
                    repos::execute_repos_show(args)
                        .await
                        .map_err(anyhow::Error::from)
                },
            )?
            .register_out(
                path!["repo", "versions"],
                |_ctx, args: repos::ReposVersionsArgs| async move {
                    repos::execute_repos_versions(args)
                        .await
                        .map_err(anyhow::Error::from)
                },
            )?
    };

    // ── cache: inspect/reclaim the on-disk skill content cache (PRD 006 US-006) ──
    let builder = {
        use cli_framework::spec::command_tree::GroupMetadata;
        builder
            .register_group(
                &path!["cache"],
                GroupMetadata {
                    summary: "Inspect and reclaim the on-disk skill content cache",
                    hidden: false,
                    category: Some("Operations"),
                    help_order: Some(20),
                },
            )?
            .register_out(
                path!["cache", "info"],
                |_ctx, args: cache::CacheInfoArgs| async move {
                    cache::execute_cache_info(args)
                        .await
                        .map_err(anyhow::Error::from)
                },
            )?
            .register_out(
                path!["cache", "clean"],
                |_ctx, args: cache::CacheCleanArgs| async move {
                    cache::execute_cache_clean(args)
                        .await
                        .map_err(anyhow::Error::from)
                },
            )?
    };

    // ── marketplace: fully migrated to typed API ─────────────────────────────
    let builder = {
        use cli_framework::spec::command_tree::GroupMetadata;
        builder
            .register_group(
                &path!["marketplace"],
                GroupMetadata {
                    summary: "Create and manage skill marketplace artifacts",
                    hidden: false,
                    category: Some("Sources and distribution"),
                    help_order: Some(20),
                },
            )?
            .register_out(
                path!["marketplace", "create"],
                |_ctx, args: marketplace::MarketplaceCreateArgs| async move {
                    marketplace::execute_marketplace_create(args)
                        .await
                        .map_err(anyhow::Error::from)
                },
            )?
    };

    // ── eval: fully migrated to typed API ────────────────────────────────────
    let builder = {
        use cli_framework::spec::command_tree::GroupMetadata;
        builder
            .register_group(
                &path!["eval"],
                GroupMetadata {
                    summary: "Evaluation commands for skill quality assurance",
                    hidden: false,
                    category: Some("Quality"),
                    help_order: Some(20),
                },
            )?
            .register_out(
                path!["eval", "validate"],
                |_ctx, args: eval::validate::ValidateArgs| async move {
                    eval::validate::execute_validate(args)
                        .await
                        .map_err(anyhow::Error::from)
                },
            )?
            .register_out(
                path!["eval", "run"],
                |_ctx, args: eval::run::RunArgs| async move {
                    eval::run::execute_run(args)
                        .await
                        .map_err(anyhow::Error::from)
                },
            )?
            .register_out(
                path!["eval", "judge"],
                |_ctx, args: eval::judge::JudgeArgs| async move {
                    eval::judge::execute_judge(args)
                        .await
                        .map_err(anyhow::Error::from)
                },
            )?
            .register_out(
                path!["eval", "report"],
                |_ctx, args: eval::report::ReportArgs| async move {
                    eval::report::execute_report(args)
                        .await
                        .map_err(anyhow::Error::from)
                },
            )?
            .register_out(
                path!["eval", "score"],
                |_ctx, args: eval::score::ScoreArgs| async move {
                    eval::score::execute_score(args)
                        .await
                        .map_err(anyhow::Error::from)
                },
            )?
            .register_out(
                path!["eval", "scorecard"],
                |_ctx, args: eval::scorecard::ScorecardArgs| async move {
                    eval::scorecard::execute_scorecard(args)
                        .await
                        .map_err(anyhow::Error::from)
                },
            )?
    };

    // ── optimize: fully migrated to typed API ────────────────────────────────────
    let builder = {
        use cli_framework::spec::command_tree::GroupMetadata;
        builder
            .register_group(
                &path!["optimization"],
                GroupMetadata {
                    summary: "Iterative skill-document optimization via text-gradient",
                    hidden: false,
                    category: Some("Quality"),
                    help_order: Some(30),
                },
            )?
            .register_out(
                path!["optimization", "run"],
                |_ctx, args: skillopt::run::RunArgs| async move {
                    skillopt::run::execute_run(args)
                        .await
                        .map_err(anyhow::Error::from)
                },
            )?
            .register_out(
                path!["optimization", "resume"],
                |_ctx, args: skillopt::resume::ResumeArgs| async move {
                    skillopt::resume::execute_resume(args)
                        .await
                        .map_err(anyhow::Error::from)
                },
            )?
            .register_out(
                path!["optimization", "status"],
                |_ctx, args: skillopt::status::StatusArgs| async move {
                    skillopt::status::execute_status(args)
                        .await
                        .map_err(anyhow::Error::from)
                },
            )?
            .register_out(
                path!["optimization", "inspect"],
                |_ctx, args: skillopt::inspect::InspectArgs| async move {
                    skillopt::inspect::execute_inspect(args)
                        .await
                        .map_err(anyhow::Error::from)
                },
            )?
            .register_out(
                path!["optimization", "export"],
                |_ctx, args: skillopt::export::ExportArgs| async move {
                    skillopt::export::execute_export(args)
                        .await
                        .map_err(anyhow::Error::from)
                },
            )?
    };

    // ── add: fully migrated to typed API ─────────────────────────────────────
    let builder = {
        let state_add = Arc::clone(&state);
        builder.register_out(path!["skill", "add"], move |ctx, args: add::AddArgs| {
            let global = ctx_global(ctx);
            let skills_dir = ctx_skills_dir(ctx);
            let state = Arc::clone(&state_add);
            async move {
                if global && skills_dir.is_some() {
                    return Err(anyhow::Error::from(crate::error::CliError::Validation(
                        "--global and --skills-dir cannot be used together".to_string(),
                    )));
                }
                let source = add::preflight_add(&args, global)
                    .await
                    .map_err(anyhow::Error::from)?;
                let svc = state.service_with(global, skills_dir).await?;
                add::execute_add_preflighted(&svc, args, global, source)
                    .await
                    .map_err(anyhow::Error::from)
            }
        })?
    };

    // ── analyze: fully migrated to typed API ─────────────────────────────────
    let builder = {
        use cli_framework::spec::command_tree::GroupMetadata;
        let state_analyze = Arc::clone(&state);
        builder
            .register_group(
                &path!["analysis"],
                GroupMetadata {
                    summary: "Diagnostic and analysis commands",
                    hidden: false,
                    category: Some("Quality"),
                    help_order: Some(10),
                },
            )?
            .register_out(path!["analysis", "matrix"], {
                let state = Arc::clone(&state_analyze);
                move |ctx, args: analyze::matrix::MatrixArgs| {
                    let global = ctx_global(ctx);
                    let skills_dir = ctx_skills_dir(ctx);
                    let state = Arc::clone(&state);
                    async move {
                        let svc = state.service_with(global, skills_dir).await?;
                        let Some(ctx) = analyze::load_analysis_context(&svc).await? else {
                            return Ok(());
                        };
                        analyze::matrix::execute_matrix(ctx, args)
                            .await
                            .map_err(anyhow::Error::from)
                    }
                }
            })?
            .register_out(path!["analysis", "cluster"], {
                let state = Arc::clone(&state_analyze);
                move |ctx, args: analyze::cluster::ClusterArgs| {
                    let global = ctx_global(ctx);
                    let skills_dir = ctx_skills_dir(ctx);
                    let state = Arc::clone(&state);
                    async move {
                        let svc = state.service_with(global, skills_dir).await?;
                        let Some(ctx) = analyze::load_analysis_context(&svc).await? else {
                            return Ok(());
                        };
                        analyze::cluster::execute_cluster(ctx, args)
                            .await
                            .map_err(anyhow::Error::from)
                    }
                }
            })?
            .register_out(path!["analysis", "duplicates"], {
                let state = Arc::clone(&state_analyze);
                move |ctx, args: analyze::duplicates::DuplicatesArgs| {
                    let global = ctx_global(ctx);
                    let skills_dir = ctx_skills_dir(ctx);
                    let state = Arc::clone(&state);
                    async move {
                        let svc = state.service_with(global, skills_dir).await?;
                        let Some(ctx) = analyze::load_analysis_context(&svc).await? else {
                            return Ok(());
                        };
                        analyze::duplicates::execute_duplicates(ctx, args)
                            .await
                            .map_err(anyhow::Error::from)
                    }
                }
            })?
    };

    // ── Typed commands migrated from register_cmd! (spec #89) ───────────────
    let builder = {
        let state_reindex = Arc::clone(&state);
        let state_remove = Arc::clone(&state);
        let state_search = Arc::clone(&state);
        let state_doctor = Arc::clone(&state);
        builder
            .register_group(
                &path!["index"],
                cli_framework::spec::command_tree::GroupMetadata {
                    summary: "Maintain the local search index",
                    hidden: false,
                    category: Some("Operations"),
                    help_order: Some(10),
                },
            )?
            .register_group(
                &path!["server"],
                cli_framework::spec::command_tree::GroupMetadata {
                    summary: "Run the HTTP API server",
                    hidden: false,
                    category: Some("Operations"),
                    help_order: Some(30),
                },
            )?
            .register_group(
                &path!["cli"],
                cli_framework::spec::command_tree::GroupMetadata {
                    summary: "Diagnose and integrate the command-line interface",
                    hidden: false,
                    category: Some("Operations"),
                    help_order: Some(50),
                },
            )?
            .register_out(
                path!["index", "rebuild"],
                move |ctx, args: reindex::ReindexArgs| {
                    let global = ctx_global(ctx);
                    let skills_dir = ctx_skills_dir(ctx);
                    let state = Arc::clone(&state_reindex);
                    async move {
                        let svc = state.service_with(global, skills_dir).await?;
                        reindex::execute_reindex(&svc, args)
                            .await
                            .map_err(anyhow::Error::from)
                    }
                },
            )?
            .register_out(
                path!["skill", "remove"],
                move |ctx, args: remove::RemoveArgs| {
                    let global = ctx_global(ctx);
                    let skills_dir = ctx_skills_dir(ctx);
                    let state = Arc::clone(&state_remove);
                    async move {
                        if global && skills_dir.is_some() {
                            return Err(anyhow::Error::from(crate::error::CliError::Validation(
                                "--global and --skills-dir cannot be combined".to_string(),
                            )));
                        }
                        let svc = state.service_with(global, skills_dir).await?;
                        remove::execute_remove(&svc, args, global)
                            .await
                            .map_err(anyhow::Error::from)
                    }
                },
            )?
            .register_out(
                path!["skill", "search"],
                move |ctx, args: search::SearchArgs| {
                    let global = ctx_global(ctx);
                    let skills_dir = ctx_skills_dir(ctx);
                    let state = Arc::clone(&state_search);
                    async move {
                        let svc = state.service_with(global, skills_dir).await?;
                        search::execute_search(&svc, args)
                            .await
                            .map_err(anyhow::Error::from)
                    }
                },
            )?
            .register_out_no_mcp(
                path!["server", "serve"],
                move |ctx, args: serve::ServeArgs| {
                    let global = ctx_global(ctx);
                    let skills_dir = ctx_skills_dir(ctx);
                    async move {
                        // `serve` builds its own service (rather than going through
                        // `FsState::service_with`) so it can inject the served
                        // project's root alongside the usual edge services; see
                        // `serve::execute_serve`.
                        serve::execute_serve(global, skills_dir, args)
                            .await
                            .map_err(anyhow::Error::from)
                    }
                },
            )?
            .register_out(
                path!["cli", "doctor"],
                move |ctx, args: doctor::DoctorArgs| {
                    let global = ctx_global(ctx);
                    let skills_dir = ctx_skills_dir(ctx);
                    let state = Arc::clone(&state_doctor);
                    async move {
                        let svc = state.service_with(global, skills_dir).await?;
                        doctor::execute_doctor(&svc, args, global)
                            .await
                            .map_err(anyhow::Error::from)
                    }
                },
            )?
            // `mcp serve` is registered here rather than left to cli-framework's
            // auto-registration, which has no write gate: it exported every
            // mutating command as a callable tool. Registering `mcp/serve`
            // ourselves suppresses that (see `AppBuilder::build`); `mcp install`
            // and `mcp list` still auto-register.
            .register_group(&path!["mcp"], mcp::group_metadata())?
            .register_out_no_mcp(path!["mcp", "serve"], |ctx, args: mcp::McpServeArgs| {
                let banner = mcp::banner_settings(ctx);
                async move { mcp::execute_mcp_serve(APP_NAME, args, banner).await }
            })?
    };

    Ok(builder)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fsstate_new_constructs() {
        // FsState::new() takes no args in the typed-API design;
        // global/skills_dir are read from env at dispatch time.
        let _state = FsState::new();
    }

    #[test]
    fn test_fsctx_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<FsCtx>();
    }
}
