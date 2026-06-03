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
mod auth_config;
mod commands;
mod config;
mod config_file;
mod context;
mod error;
pub mod runtime_selector;
mod utils;

use cli_framework::app::context::AppContext;
use cli_framework::prelude::AppBuilder;
use cli_framework::spec::value::ArgValue;
use context::{FsCtx, FsState};
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

fn ctx_global(ctx: &dyn AppContext) -> bool {
    ctx.opt_global_args()
        .and_then(|m| m.get("global"))
        .and_then(|v| {
            if let ArgValue::Bool(b) = v {
                Some(*b)
            } else {
                None
            }
        })
        .unwrap_or(false)
}

fn ctx_skills_dir(ctx: &dyn AppContext) -> Option<std::path::PathBuf> {
    ctx.opt_global_args()
        .and_then(|m| m.get("skills-dir"))
        .and_then(|v| {
            if let ArgValue::Str(s) = v {
                Some(std::path::PathBuf::from(s))
            } else {
                None
            }
        })
}

use commands::{
    add, analyze, auth, disable, eval, init, install, list, marketplace, package, publish, read,
    reindex, remove, repos, resolve, search, serve, show, skillopt, sync, update,
};

#[tokio::main]
async fn main() {
    let raw: Vec<String> = std::env::args().collect();
    let verbose = raw.iter().any(|a| a == "--verbose" || a == "-v");
    fastskill_core::init_logging_with_verbose(verbose);

    let state = Arc::new(FsState::new());
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

    match app.run_with_args(raw).await {
        Ok(()) => std::process::exit(0),
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    }
}

fn build_app(builder: AppBuilder, state: Arc<FsState>) -> anyhow::Result<AppBuilder> {
    use cli_framework::path;
    use cli_framework::spec::arg_spec::{ArgKind, ArgSpec, ArgValueType, Cardinality};

    // ── Global flags ─────────────────────────────────────────────────────────
    let builder = builder
        .global_flag(ArgSpec {
            name: "skills-dir",
            kind: ArgKind::Option,
            long: Some("skills-dir"),
            value_type: ArgValueType::String,
            cardinality: Cardinality::Optional,
            help: "Override the skills directory path",
            ..Default::default()
        })
        .global_flag(ArgSpec {
            name: "global",
            kind: ArgKind::Flag,
            long: Some("global"),
            value_type: ArgValueType::Bool,
            cardinality: Cardinality::Optional,
            help: "Use global skills directory (~/.config/fastskill/skills)",
            ..Default::default()
        })
        .global_flag(ArgSpec {
            name: "verbose",
            kind: ArgKind::Flag,
            long: Some("verbose"),
            short: Some('v'),
            value_type: ArgValueType::Bool,
            cardinality: Cardinality::Optional,
            help: "Enable verbose output",
            ..Default::default()
        });

    // ── Typed commands (no service) ──────────────────────────────────────────
    let builder = builder
        .register(path!["init"], |_ctx, args: init::InitArgs| async move {
            init::execute_init(args).await.map_err(anyhow::Error::from)
        })?
        .register(
            path!["install"],
            |_ctx, args: install::InstallArgs| async move {
                install::execute_install(args)
                    .await
                    .map_err(anyhow::Error::from)
            },
        )?
        .register(path!["update"], |ctx, args: update::UpdateArgs| {
            let global = ctx_global(ctx);
            async move {
                update::execute_update(args, global)
                    .await
                    .map_err(anyhow::Error::from)
            }
        })?
        .register(path!["show"], |ctx, args: show::ShowArgs| {
            let global = ctx_global(ctx);
            async move {
                show::execute_show(args, global)
                    .await
                    .map_err(anyhow::Error::from)
            }
        })?
        .register(
            path!["package"],
            |_ctx, args: package::PackageArgs| async move {
                package::execute_package(args)
                    .await
                    .map_err(anyhow::Error::from)
            },
        )?
        .register(
            path!["publish"],
            |_ctx, args: publish::PublishArgs| async move {
                publish::execute_publish(args)
                    .await
                    .map_err(anyhow::Error::from)
            },
        )?;

    // ── Typed commands that need FsState (service injection) ─────────────────
    let builder = {
        let state_disable = Arc::clone(&state);
        let state_list = Arc::clone(&state);
        let state_read = Arc::clone(&state);
        builder
            .register(path!["disable"], move |ctx, args: disable::DisableArgs| {
                let global = ctx_global(ctx);
                let skills_dir = ctx_skills_dir(ctx);
                let state = Arc::clone(&state_disable);
                async move {
                    let svc = state.service_with(global, skills_dir).await?;
                    disable::execute_disable(&svc, args)
                        .await
                        .map_err(anyhow::Error::from)
                }
            })?
            .register(path!["list"], move |ctx, args: list::ListArgs| {
                let global = ctx_global(ctx);
                let skills_dir = ctx_skills_dir(ctx);
                let state = Arc::clone(&state_list);
                async move {
                    let svc = state.service_with(global, skills_dir).await?;
                    list::execute_list(&svc, args, global)
                        .await
                        .map_err(anyhow::Error::from)
                }
            })?
            .register(path!["read"], move |ctx, args: read::ReadArgs| {
                let global = ctx_global(ctx);
                let skills_dir = ctx_skills_dir(ctx);
                let state = Arc::clone(&state_read);
                async move {
                    let svc = state.service_with(global, skills_dir).await?;
                    read::execute_read(svc, args)
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
                &path!["repos"],
                GroupMetadata {
                    summary: "Manage repository list and browse remote skill catalog",
                    hidden: false,
                },
            )?
            .register(
                path!["repos", "list"],
                |_ctx, args: repos::ReposListArgs| async move {
                    repos::execute_repos_list(args)
                        .await
                        .map_err(anyhow::Error::from)
                },
            )?
            .register(
                path!["repos", "add"],
                |_ctx, args: repos::ReposAddArgs| async move {
                    repos::execute_repos_add(args)
                        .await
                        .map_err(anyhow::Error::from)
                },
            )?
            .register(
                path!["repos", "remove"],
                |_ctx, args: repos::ReposRemoveArgs| async move {
                    repos::execute_repos_remove(args)
                        .await
                        .map_err(anyhow::Error::from)
                },
            )?
            .register(
                path!["repos", "info"],
                |_ctx, args: repos::ReposInfoArgs| async move {
                    repos::execute_repos_info(args)
                        .await
                        .map_err(anyhow::Error::from)
                },
            )?
            .register(
                path!["repos", "update"],
                |_ctx, args: repos::ReposUpdateArgs| async move {
                    repos::execute_repos_update(args)
                        .await
                        .map_err(anyhow::Error::from)
                },
            )?
            .register(
                path!["repos", "test"],
                |_ctx, args: repos::ReposTestArgs| async move {
                    repos::execute_repos_test(args)
                        .await
                        .map_err(anyhow::Error::from)
                },
            )?
            .register(
                path!["repos", "refresh"],
                |_ctx, args: repos::ReposRefreshArgs| async move {
                    repos::execute_repos_refresh(args)
                        .await
                        .map_err(anyhow::Error::from)
                },
            )?
            .register(
                path!["repos", "skills"],
                |_ctx, args: repos::ReposSkillsArgs| async move {
                    repos::execute_repos_skills(args)
                        .await
                        .map_err(anyhow::Error::from)
                },
            )?
            .register(
                path!["repos", "show"],
                |_ctx, args: repos::ReposShowArgs| async move {
                    repos::execute_repos_show(args)
                        .await
                        .map_err(anyhow::Error::from)
                },
            )?
            .register(
                path!["repos", "versions"],
                |_ctx, args: repos::ReposVersionsArgs| async move {
                    repos::execute_repos_versions(args)
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
                },
            )?
            .register(
                path!["marketplace", "create"],
                |_ctx, args: marketplace::MarketplaceCreateArgs| async move {
                    marketplace::execute_marketplace_create(args)
                        .await
                        .map_err(anyhow::Error::from)
                },
            )?
    };

    // ── auth: fully migrated to typed API ────────────────────────────────────
    let builder = {
        use cli_framework::spec::command_tree::GroupMetadata;
        builder
            .register_group(
                &path!["auth"],
                GroupMetadata {
                    summary: "Manage authentication for registries",
                    hidden: false,
                },
            )?
            .register(
                path!["auth", "login"],
                |_ctx, args: auth::LoginArgs| async move {
                    auth::execute_login(args).await.map_err(anyhow::Error::from)
                },
            )?
            .register(
                path!["auth", "logout"],
                |_ctx, args: auth::LogoutArgs| async move {
                    auth::execute_logout(args).map_err(anyhow::Error::from)
                },
            )?
            .register(
                path!["auth", "whoami"],
                |_ctx, args: auth::WhoamiArgs| async move {
                    auth::execute_whoami(args).map_err(anyhow::Error::from)
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
                },
            )?
            .register(
                path!["eval", "validate"],
                |_ctx, args: eval::validate::ValidateArgs| async move {
                    eval::validate::execute_validate(args)
                        .await
                        .map_err(anyhow::Error::from)
                },
            )?
            .register(
                path!["eval", "run"],
                |_ctx, args: eval::run::RunArgs| async move {
                    eval::run::execute_run(args)
                        .await
                        .map_err(anyhow::Error::from)
                },
            )?
            .register(
                path!["eval", "report"],
                |_ctx, args: eval::report::ReportArgs| async move {
                    eval::report::execute_report(args)
                        .await
                        .map_err(anyhow::Error::from)
                },
            )?
            .register(
                path!["eval", "score"],
                |_ctx, args: eval::score::ScoreArgs| async move {
                    eval::score::execute_score(args)
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
                &path!["optimize"],
                GroupMetadata {
                    summary: "Iterative skill-document optimization via text-gradient",
                    hidden: false,
                },
            )?
            .register(
                path!["optimize", "run"],
                |_ctx, args: skillopt::run::RunArgs| async move {
                    skillopt::run::execute_run(args)
                        .await
                        .map_err(anyhow::Error::from)
                },
            )?
            .register(
                path!["optimize", "resume"],
                |_ctx, args: skillopt::resume::ResumeArgs| async move {
                    skillopt::resume::execute_resume(args)
                        .await
                        .map_err(anyhow::Error::from)
                },
            )?
            .register(
                path!["optimize", "status"],
                |_ctx, args: skillopt::status::StatusArgs| async move {
                    skillopt::status::execute_status(args)
                        .await
                        .map_err(anyhow::Error::from)
                },
            )?
            .register(
                path!["optimize", "inspect"],
                |_ctx, args: skillopt::inspect::InspectArgs| async move {
                    skillopt::inspect::execute_inspect(args)
                        .await
                        .map_err(anyhow::Error::from)
                },
            )?
            .register(
                path!["optimize", "export"],
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
        builder.register(path!["add"], move |ctx, args: add::AddArgs| {
            let global = ctx_global(ctx);
            let skills_dir = ctx_skills_dir(ctx);
            let state = Arc::clone(&state_add);
            async move {
                let svc = state.service_with(global, skills_dir).await?;
                add::execute_add(&svc, args, global)
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
                &path!["analyze"],
                GroupMetadata {
                    summary: "Diagnostic and analysis commands",
                    hidden: false,
                },
            )?
            .register(path!["analyze", "matrix"], {
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
            .register(path!["analyze", "cluster"], {
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
            .register(path!["analyze", "duplicates"], {
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
        let state_sync = Arc::clone(&state);
        let state_reindex = Arc::clone(&state);
        let state_remove = Arc::clone(&state);
        let state_resolve = Arc::clone(&state);
        let state_search = Arc::clone(&state);
        let state_serve = Arc::clone(&state);
        builder
            .register(path!["sync"], move |ctx, args: sync::SyncArgs| {
                let global = ctx_global(ctx);
                let skills_dir = ctx_skills_dir(ctx);
                let state = Arc::clone(&state_sync);
                async move {
                    let svc = state.service_with(global, skills_dir).await?;
                    sync::execute_sync(&svc, args)
                        .await
                        .map_err(anyhow::Error::from)
                }
            })?
            .register(path!["reindex"], move |ctx, args: reindex::ReindexArgs| {
                let global = ctx_global(ctx);
                let skills_dir = ctx_skills_dir(ctx);
                let state = Arc::clone(&state_reindex);
                async move {
                    let svc = state.service_with(global, skills_dir).await?;
                    reindex::execute_reindex(&svc, args)
                        .await
                        .map_err(anyhow::Error::from)
                }
            })?
            .register(path!["remove"], move |ctx, args: remove::RemoveArgs| {
                let global = ctx_global(ctx);
                let skills_dir = ctx_skills_dir(ctx);
                let state = Arc::clone(&state_remove);
                async move {
                    let svc = state.service_with(global, skills_dir).await?;
                    remove::execute_remove(&svc, args, global)
                        .await
                        .map_err(anyhow::Error::from)
                }
            })?
            .register(path!["resolve"], move |ctx, args: resolve::ResolveArgs| {
                let global = ctx_global(ctx);
                let skills_dir = ctx_skills_dir(ctx);
                let state = Arc::clone(&state_resolve);
                async move {
                    let svc = state.service_with(global, skills_dir).await?;
                    resolve::execute_resolve(&svc, args)
                        .await
                        .map_err(anyhow::Error::from)
                }
            })?
            .register(path!["search"], move |ctx, args: search::SearchArgs| {
                let global = ctx_global(ctx);
                let skills_dir = ctx_skills_dir(ctx);
                let state = Arc::clone(&state_search);
                async move {
                    let svc = state.service_with(global, skills_dir).await?;
                    search::execute_search(&svc, args)
                        .await
                        .map_err(anyhow::Error::from)
                }
            })?
            .register(path!["serve"], move |ctx, args: serve::ServeArgs| {
                let global = ctx_global(ctx);
                let skills_dir = ctx_skills_dir(ctx);
                let state = Arc::clone(&state_serve);
                async move {
                    let svc = state.service_with(global, skills_dir).await?;
                    serve::execute_serve(svc, args)
                        .await
                        .map_err(anyhow::Error::from)
                }
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
