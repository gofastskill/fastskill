//! Add command implementation

pub mod install;
pub mod skill_def;
pub mod sources;

// Re-export public API consumed by install_utils.rs
use crate::error::{manifest_required_message, CliError, CliResult};
use crate::utils::{detect_skill_source, validate_skill_structure, SkillSource};
use chrono::Utc;
use clap::Args;
use cli_framework::command::{FromArgValueMap, IntoCommandSpec};
use cli_framework::spec::arg_spec::{ArgKind, ArgSpec, ArgValueType, Cardinality};
use cli_framework::spec::command_tree::CommandSpec;
use cli_framework::spec::value::ArgValue;
use fastskill_core::core::origin::{GitRef, Origin};
use fastskill_core::core::project::resolve_project_file;
use fastskill_core::core::repository::RepositoryManager;
use fastskill_core::core::version::VersionConstraint;
use fastskill_core::core::AddMode;
use fastskill_core::{FastSkillService, SkillDefinition};
pub use install::copy_dir_recursive;
pub use skill_def::create_skill_from_path;
use std::collections::HashMap;
use std::env;
use std::path::{Path, PathBuf};

/// Bundled options for add operations (reduces argument count)
struct AddContext<'a> {
    service: &'a FastSkillService,
    force: bool,
    editable: bool,
    groups: Vec<String>,
    global: bool,
}

/// Source metadata to record after installing a skill.
///
/// `Origin` (install intent) is now the single source of truth for provenance,
/// so this is a thin wrapper rather than a flat bag of source_* fields.
struct SourceMeta {
    origin: Origin,
}

/// Target for install: where to copy and what to record
struct InstallTarget {
    storage_dir: PathBuf,
    meta: SourceMeta,
    version_display: String,
}

fn update_project_files(skill_def: &SkillDefinition, groups: Vec<String>) -> CliResult<()> {
    use crate::utils::manifest_utils;
    manifest_utils::add_skill_to_project_toml(skill_def, groups.clone())
        .map_err(|e| CliError::Config(format!("Failed to update skill-project.toml: {}", e)))?;

    let current_dir = env::current_dir()
        .map_err(|e| CliError::Config(format!("Failed to get current directory: {}", e)))?;
    let project_file_result = resolve_project_file(&current_dir);
    let lock_path = if let Some(parent) = project_file_result.path.parent() {
        parent.join("skills.lock")
    } else {
        PathBuf::from("skills.lock")
    };
    manifest_utils::update_lock_file(&lock_path, skill_def, groups)
        .map_err(|e| CliError::Config(format!("Failed to update lock file: {}", e)))?;

    Ok(())
}

/// Helper to update global-skills.lock (global scope only)
fn update_global_files(skill_def: &SkillDefinition) -> CliResult<()> {
    use crate::utils::manifest_utils;
    manifest_utils::update_global_lock_file(skill_def, Utc::now())
        .map_err(|e| CliError::Config(format!("Failed to update global lock file: {}", e)))?;
    Ok(())
}

async fn register_skill_once(ctx: &AddContext<'_>, skill_def: &SkillDefinition) -> CliResult<()> {
    if ctx.force {
        ctx.service
            .skill_manager()
            .force_register_skill(skill_def.clone())
            .await
            .map_err(CliError::Service)?;
    } else {
        if ctx
            .service
            .skill_manager()
            .get_skill(&skill_def.id)
            .await
            .map_err(CliError::Service)?
            .is_some()
        {
            return Err(CliError::Config(format!(
                "Skill '{}' already exists. Use --force to overwrite.",
                skill_def.id
            )));
        }
        ctx.service
            .skill_manager()
            .register_skill(skill_def.clone())
            .await
            .map_err(CliError::Service)?;
    }
    Ok(())
}

/// Add a skill (two operational modes)
///
/// Mode 1: Manifest-managed (when skill-project.toml exists)
/// Mode 2: Local-only (when no skill-project.toml or --global flag)
#[derive(Debug, Args)]
pub struct AddArgs {
    /// Source: path to zip file, folder, git URL, or skill ID (e.g., pptx@1.2.3)
    pub source: String,

    /// Override source type (registry, github/git, local)
    #[arg(long)]
    pub source_type: Option<String>,

    /// Git branch to checkout (only for git URLs)
    #[arg(long)]
    pub branch: Option<String>,

    /// Git tag to checkout (only for git URLs)
    #[arg(long)]
    pub tag: Option<String>,

    /// Force registration even if skill already exists
    #[arg(short, long)]
    pub force: bool,

    /// Install skill in editable mode (for local development, like poetry add -e)
    #[arg(short = 'e', long)]
    pub editable: bool,

    /// Add skill to a specific group (like poetry add --group dev)
    #[arg(long)]
    pub group: Option<String>,

    /// Add all skills found under the directory (only for local folders)
    #[arg(short = 'r', long)]
    pub recursive: bool,

    /// Trigger reindex after adding (overrides config)
    #[arg(long)]
    pub reindex: bool,

    /// Skip reindex after adding
    #[arg(long)]
    pub no_reindex: bool,
}

impl IntoCommandSpec for AddArgs {
    fn command_spec() -> CommandSpec {
        CommandSpec {
            summary: "Add a skill (from local path, zip, git URL, or registry ID)",
            syntax: Some("add <SOURCE> [OPTIONS]"),
            category: Some("packages"),
            args: vec![
                ArgSpec {
                    name: "source",
                    kind: ArgKind::Positional,
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Required,
                    help:
                        "Source: path to zip file, folder, git URL, or skill ID (e.g., pptx@1.2.3)",
                    ..Default::default()
                },
                ArgSpec {
                    name: "source-type",
                    kind: ArgKind::Option,
                    long: Some("source-type"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    help: "Override source type (registry, github/git, local)",
                    ..Default::default()
                },
                ArgSpec {
                    name: "branch",
                    kind: ArgKind::Option,
                    long: Some("branch"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    help: "Git branch to checkout (only for git URLs)",
                    ..Default::default()
                },
                ArgSpec {
                    name: "tag",
                    kind: ArgKind::Option,
                    long: Some("tag"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    help: "Git tag to checkout (only for git URLs)",
                    ..Default::default()
                },
                ArgSpec {
                    name: "force",
                    kind: ArgKind::Flag,
                    short: Some('f'),
                    long: Some("force"),
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    help: "Force registration even if skill already exists",
                    ..Default::default()
                },
                ArgSpec {
                    name: "editable",
                    kind: ArgKind::Flag,
                    short: Some('e'),
                    long: Some("editable"),
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    help: "Install skill in editable mode (for local development)",
                    ..Default::default()
                },
                ArgSpec {
                    name: "group",
                    kind: ArgKind::Option,
                    long: Some("group"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    help: "Add skill to a specific group",
                    ..Default::default()
                },
                ArgSpec {
                    name: "recursive",
                    kind: ArgKind::Flag,
                    short: Some('r'),
                    long: Some("recursive"),
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    help: "Add all skills found under the directory (only for local folders)",
                    ..Default::default()
                },
                ArgSpec {
                    name: "reindex",
                    kind: ArgKind::Flag,
                    long: Some("reindex"),
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    help: "Trigger reindex after adding (overrides config)",
                    ..Default::default()
                },
                ArgSpec {
                    name: "no-reindex",
                    kind: ArgKind::Flag,
                    long: Some("no-reindex"),
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    help: "Skip reindex after adding",
                    ..Default::default()
                },
            ],
            ..Default::default()
        }
    }
}

impl FromArgValueMap for AddArgs {
    fn from_arg_value_map(map: &HashMap<String, ArgValue>) -> Self {
        Self {
            source: map
                .get("source")
                .and_then(|v| {
                    if let ArgValue::Str(s) = v {
                        Some(s.clone())
                    } else {
                        None
                    }
                })
                .unwrap_or_default(),
            source_type: map.get("source-type").and_then(|v| {
                if let ArgValue::Str(s) = v {
                    Some(s.clone())
                } else {
                    None
                }
            }),
            branch: map.get("branch").and_then(|v| {
                if let ArgValue::Str(s) = v {
                    Some(s.clone())
                } else {
                    None
                }
            }),
            tag: map.get("tag").and_then(|v| {
                if let ArgValue::Str(s) = v {
                    Some(s.clone())
                } else {
                    None
                }
            }),
            force: matches!(map.get("force"), Some(ArgValue::Bool(true))),
            editable: matches!(map.get("editable"), Some(ArgValue::Bool(true))),
            group: map.get("group").and_then(|v| {
                if let ArgValue::Str(s) = v {
                    Some(s.clone())
                } else {
                    None
                }
            }),
            recursive: matches!(map.get("recursive"), Some(ArgValue::Bool(true))),
            reindex: matches!(map.get("reindex"), Some(ArgValue::Bool(true))),
            no_reindex: matches!(map.get("no-reindex"), Some(ArgValue::Bool(true))),
        }
    }
}

fn resolve_source(args: &AddArgs) -> SkillSource {
    let Some(ref source_type) = args.source_type else {
        return detect_skill_source(&args.source);
    };
    match source_type.as_str() {
        "registry" => SkillSource::SkillId(args.source.clone()),
        "github" | "git" => SkillSource::GitUrl(args.source.clone()),
        "local" => {
            let path = PathBuf::from(&args.source);
            if path.extension().and_then(|s| s.to_str()) == Some("zip") {
                SkillSource::ZipFile(path)
            } else {
                SkillSource::Folder(path)
            }
        }
        _ => detect_skill_source(&args.source),
    }
}

fn ensure_manifest() -> CliResult<()> {
    let current_dir = env::current_dir()
        .map_err(|e| CliError::Config(format!("Failed to get current directory: {}", e)))?;
    let project_file_result = resolve_project_file(&current_dir);
    if !project_file_result.found {
        return Err(CliError::Config(manifest_required_message().to_string()));
    }
    Ok(())
}

fn validate_folder_has_skill(path: &Path) -> CliResult<()> {
    if path.join("SKILL.md").exists() {
        return Ok(());
    }
    if let Ok(dirs) = install::get_skill_dirs_recursive(path) {
        if !dirs.is_empty() {
            return Err(CliError::Validation(format!(
                "This directory has no SKILL.md at the root but contains {} skill(s) in subdirectories. Add them all with: fastskill add {} --recursive",
                dirs.len(),
                path.display()
            )));
        }
    }
    Err(CliError::Validation(format!(
        "No SKILL.md found in {}. A skill directory must contain a SKILL.md file. If this folder has multiple skills in subdirectories, use --recursive.",
        path.display()
    )))
}

/// Build the install-intent [`Origin`] for the common (single-skill,
/// project-level) add path from the classified `source` and parsed args
/// (ADR-0005: `detect_skill_source` → `Origin`, feeding `add_from_origin`).
fn build_origin(source: &SkillSource, args: &AddArgs) -> CliResult<Origin> {
    match source {
        SkillSource::ZipFile(path) => {
            let canonical = path.canonicalize().map_err(|e| {
                CliError::InvalidSource(format!(
                    "Failed to resolve absolute path for '{}': {}",
                    path.display(),
                    e
                ))
            })?;
            Ok(Origin::Local {
                path: canonical,
                editable: false,
            })
        }
        SkillSource::Folder(path) => {
            let canonical = path.canonicalize().map_err(|e| {
                CliError::InvalidSource(format!(
                    "Failed to resolve absolute path for '{}': {}",
                    path.display(),
                    e
                ))
            })?;
            Ok(Origin::Local {
                path: canonical,
                editable: args.editable,
            })
        }
        SkillSource::GitUrl(url) => {
            let git_info = crate::utils::parse_git_url(url)?;
            let branch = args.branch.clone().or_else(|| git_info.branch.clone());
            let r#ref = match (&branch, &args.tag) {
                (Some(b), _) => GitRef::Branch(b.clone()),
                (None, Some(t)) => GitRef::Tag(t.clone()),
                (None, None) => GitRef::Default,
            };
            Ok(Origin::Git {
                url: url.clone(),
                r#ref,
                subdir: git_info.subdir,
            })
        }
        SkillSource::RemoteZipUrl(url) => Ok(Origin::ZipUrl { url: url.clone() }),
        SkillSource::SkillId(skill_id_input) => {
            let (skill_id_full, _scope, _expected_id, version_opt) =
                sources::parse_registry_scope_id(skill_id_input)?;
            let version = version_opt
                .as_deref()
                .map(VersionConstraint::parse)
                .transpose()
                .map_err(|e| CliError::Config(format!("Invalid version constraint: {}", e)))?;

            // Resolve "default" to the concrete configured repository's name now
            // (mirrors the pre-seam `add_from_registry`), so a missing repository
            // config fails fast with a clear message rather than surfacing later
            // from inside the seam.
            let repositories = crate::config::load_repositories_from_project()?;
            let repo_manager = RepositoryManager::from_definitions(repositories);
            let default_repo = repo_manager.get_default_repository().ok_or_else(|| {
                CliError::Config(
                    "No default repository configured. Use 'fastskill repos add' to add a \
                     repository."
                        .to_string(),
                )
            })?;

            Ok(Origin::Repository {
                repo: default_repo.name.clone(),
                skill: skill_id_full,
                version,
            })
        }
    }
}

pub async fn execute_add(service: &FastSkillService, args: AddArgs, global: bool) -> CliResult<()> {
    if args.reindex && args.no_reindex {
        return Err(CliError::Validation(
            "--reindex and --no-reindex cannot be used together".to_string(),
        ));
    }
    let reindex = args.reindex;
    let no_reindex = args.no_reindex;

    let source = resolve_source(&args);

    if args.editable {
        match &source {
            SkillSource::Folder(_) => {}
            SkillSource::ZipFile(_)
            | SkillSource::GitUrl(_)
            | SkillSource::RemoteZipUrl(_)
            | SkillSource::SkillId(_) => {
                return Err(CliError::Config(
                    "--editable (-e) is only supported for local folder sources. \
                     Remove -e or use a local path."
                        .to_string(),
                ));
            }
        }
    }

    if !global {
        ensure_manifest()?;
    }

    // `--global` and `--recursive` are not (yet) expressible through the core
    // `add_from_origin` seam — it is single-skill + project-level only (no
    // global-lock concept, no directory-of-skills fan-out) — so both keep the
    // pre-seam per-source-type install path below.
    if global || args.recursive {
        let groups = args.group.clone().map(|g| vec![g]).unwrap_or_default();
        let ctx = AddContext {
            service,
            force: args.force,
            editable: args.editable,
            groups,
            global,
        };

        if args.recursive {
            let path = match &source {
                SkillSource::Folder(p) => p,
                _ => {
                    return Err(CliError::Config(
                        "Recursive add is only valid when source is a local directory".to_string(),
                    ));
                }
            };
            install::handle_recursive_add(&ctx, path).await?;
        } else {
            match source {
                SkillSource::ZipFile(path) => sources::add_from_zip(&ctx, &path).await?,
                SkillSource::Folder(path) => {
                    validate_folder_has_skill(&path)?;
                    sources::add_from_folder(&ctx, &path).await?
                }
                SkillSource::GitUrl(url) => {
                    sources::add_from_git(&ctx, &url, args.branch.as_deref(), args.tag.as_deref())
                        .await?
                }
                SkillSource::RemoteZipUrl(url) => sources::add_from_zip_url(&ctx, &url).await?,
                SkillSource::SkillId(skill_id) => {
                    sources::add_from_registry(&ctx, &skill_id).await?
                }
            }
        }

        let auto_reindex = crate::config_file::load_auto_reindex_config();
        return crate::utils::reindex_utils::maybe_auto_reindex(
            service,
            "add",
            reindex,
            no_reindex,
            auto_reindex,
            false,
        )
        .await;
    }

    // Common path: single skill, project-level → core install seam (ADR-0005).
    // Local-folder sources keep the CLI's own pre-checks (the "did you mean
    // --recursive?" hint and the StandardValidator compatibility warnings) since
    // core's own `validate_cloned_skill` is a narrower, warning-free check.
    if let SkillSource::Folder(path) = &source {
        validate_folder_has_skill(path)?;
        validate_skill_structure(path)?;
    }

    let origin = build_origin(&source, &args)?;
    let mode = if args.force {
        AddMode::Update
    } else {
        AddMode::Fresh
    };
    let outcome = service
        .add_from_origin(origin, mode)
        .await
        .map_err(CliError::Service)?;

    // Core-seam gap: `add_from_origin`'s manifest/lock upsert has no `groups`
    // parameter (always writes `groups: None` / `Vec::new()`), so `--group` is
    // reapplied here, same as the update path.
    if let Some(group) = &args.group {
        let current_dir = env::current_dir()
            .map_err(|e| CliError::Config(format!("Failed to get current directory: {}", e)))?;
        let project_file_result = resolve_project_file(&current_dir);
        let lock_path = project_file_result
            .path
            .parent()
            .map(|p| p.join("skills.lock"))
            .unwrap_or_else(|| PathBuf::from("skills.lock"));
        if let Err(e) = crate::utils::manifest_utils::reapply_groups_after_seam(
            &project_file_result.path,
            &lock_path,
            &outcome.id,
            vec![group.clone()],
        ) {
            eprintln!(
                "{}",
                crate::utils::messages::error(&format!(
                    "Added {} but failed to record its group: {}",
                    outcome.id, e
                ))
            );
        }
    }

    // `AddOutcome` only carries the skill `id`, not its display `name`; look the
    // freshly-registered skill back up for a nicer message, falling back to the
    // id (which is always a valid, if less friendly, thing to print).
    let display_name = match fastskill_core::SkillId::new(outcome.id.clone()) {
        Ok(id) => service
            .skill_manager()
            .get_skill(&id)
            .await
            .ok()
            .flatten()
            .map(|s| s.name)
            .unwrap_or_else(|| outcome.id.clone()),
        Err(_) => outcome.id.clone(),
    };

    println!(
        "Successfully added skill: {} (v{})",
        display_name, outcome.resolved.version
    );
    println!(
        "{}",
        crate::utils::messages::ok("Updated skill-project.toml and skills.lock")
    );

    let auto_reindex = crate::config_file::load_auto_reindex_config();
    crate::utils::reindex_utils::maybe_auto_reindex(
        service,
        "add",
        reindex,
        no_reindex,
        auto_reindex,
        false,
    )
    .await
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::expect_used,
    clippy::await_holding_lock
)]
mod tests {
    use super::*;
    use fastskill_core::{FastSkillService, ServiceConfig};
    use tempfile::TempDir;

    async fn run_add_expect_err(source: &str, source_type: Option<&str>, force: bool) {
        let temp_dir = TempDir::new().unwrap();
        let config = ServiceConfig {
            skill_storage_path: temp_dir.path().to_path_buf(),
            ..Default::default()
        };
        let mut service = FastSkillService::new(config).await.unwrap();
        service.initialize().await.unwrap();
        let args = AddArgs {
            source: source.to_string(),
            source_type: source_type.map(String::from),
            branch: None,
            tag: None,
            force,
            editable: false,
            group: None,
            recursive: false,
            reindex: false,
            no_reindex: false,
        };
        let result = execute_add(&service, args, false).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_execute_add_nonexistent_source() {
        run_add_expect_err("/nonexistent/path", Some("local"), false).await;
    }

    #[tokio::test]
    async fn test_execute_add_invalid_skill_id() {
        run_add_expect_err("invalid@skill@id", Some("registry"), false).await;
    }

    #[tokio::test]
    async fn test_execute_add_with_force_flag() {
        run_add_expect_err("/nonexistent/path", Some("local"), true).await;
    }

    #[tokio::test]
    async fn test_execute_add_no_manifest_fails_with_instructions() {
        let _lock = fastskill_core::test_utils::DIR_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());

        let temp_dir = TempDir::new().unwrap();
        let config = ServiceConfig {
            skill_storage_path: temp_dir.path().to_path_buf(),
            ..Default::default()
        };
        let mut service = FastSkillService::new(config).await.unwrap();
        service.initialize().await.unwrap();

        struct DirGuard(Option<std::path::PathBuf>);
        impl Drop for DirGuard {
            fn drop(&mut self) {
                if let Some(dir) = &self.0 {
                    let _ = std::env::set_current_dir(dir);
                }
            }
        }
        let original_dir = std::env::current_dir().ok();
        let _guard = DirGuard(original_dir);
        std::env::set_current_dir(temp_dir.path()).unwrap();

        let args = AddArgs {
            source: "scope/some-skill".to_string(),
            source_type: Some("registry".to_string()),
            branch: None,
            tag: None,
            force: false,
            editable: false,
            group: None,
            recursive: false,
            reindex: false,
            no_reindex: false,
        };

        let result = execute_add(&service, args, false).await;
        assert!(
            result.is_err(),
            "Expected error when no manifest: {:?}",
            result
        );
        if let Err(CliError::Config(msg)) = result {
            assert!(
                msg.contains("skill-project.toml not found") && msg.contains("fastskill init"),
                "Error must mention skill-project.toml and fastskill init: '{}'",
                msg
            );
        } else {
            panic!("Expected Config error");
        }
    }

    // ── Common path (project, non-recursive) → core `add_from_origin` seam ────

    fn write_valid_skill_md(dir: &std::path::Path, name: &str) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(
            dir.join("SKILL.md"),
            format!(
                "---\nname: {name}\nversion: \"1.0.0\"\ndescription: A test skill\n---\nBody\n"
            ),
        )
        .unwrap();
    }

    struct DirGuard(Option<PathBuf>);
    impl Drop for DirGuard {
        fn drop(&mut self) {
            if let Some(dir) = &self.0 {
                let _ = std::env::set_current_dir(dir);
            }
        }
    }

    fn setup_common_path_project() -> (TempDir, DirGuard, PathBuf) {
        let tmp = TempDir::new().unwrap();
        let guard = DirGuard(std::env::current_dir().ok());
        std::env::set_current_dir(tmp.path()).unwrap();
        std::fs::write(
            tmp.path().join("skill-project.toml"),
            "[tool.fastskill]\nskills_directory = \".claude/skills\"\n\n[dependencies]\n",
        )
        .unwrap();
        let skills_dir = tmp.path().join(".claude/skills");
        std::fs::create_dir_all(&skills_dir).unwrap();
        (tmp, guard, skills_dir)
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn test_execute_add_local_folder_common_path_end_to_end() {
        let _lock = fastskill_core::test_utils::DIR_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let (tmp, _guard, skills_dir) = setup_common_path_project();

        let src = tmp.path().join("src-skill");
        write_valid_skill_md(&src, "common-path-skill");

        let config = ServiceConfig {
            skill_storage_path: skills_dir.clone(),
            ..Default::default()
        };
        let mut service = FastSkillService::new(config).await.unwrap();
        service.initialize().await.unwrap();

        let args = AddArgs {
            source: src.display().to_string(),
            source_type: None,
            branch: None,
            tag: None,
            force: false,
            editable: false,
            group: Some("dev".to_string()),
            recursive: false,
            reindex: false,
            no_reindex: false,
        };

        let result = execute_add(&service, args, false).await;
        assert!(result.is_ok(), "add should succeed: {:?}", result);
        assert!(skills_dir.join("common-path-skill/SKILL.md").exists());

        // --group was reapplied onto both the manifest dependency and the lock
        // entry (core-seam gap workaround: add_from_origin's own upsert has no
        // groups parameter).
        let mut project = fastskill_core::core::manifest::SkillProjectToml::load_from_file(
            &tmp.path().join("skill-project.toml"),
        )
        .unwrap();
        let dep = project
            .dependencies
            .as_mut()
            .unwrap()
            .dependencies
            .remove("common-path-skill")
            .unwrap();
        match dep {
            fastskill_core::core::manifest::DependencySpec::Inline { groups, .. } => {
                assert_eq!(groups, Some(vec!["dev".to_string()]));
            }
            other => panic!("expected Inline dependency spec, got {:?}", other),
        }

        let lock = fastskill_core::core::lock::ProjectSkillsLock::load_from_file(
            &tmp.path().join("skills.lock"),
        )
        .unwrap();
        let entry = lock
            .skills
            .iter()
            .find(|s| s.id == "common-path-skill")
            .unwrap();
        assert_eq!(entry.groups, vec!["dev".to_string()]);
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn test_execute_add_force_overwrites_via_common_path() {
        let _lock = fastskill_core::test_utils::DIR_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let (tmp, _guard, skills_dir) = setup_common_path_project();

        let src = tmp.path().join("src-skill");
        write_valid_skill_md(&src, "force-skill");

        let config = ServiceConfig {
            skill_storage_path: skills_dir.clone(),
            ..Default::default()
        };
        let mut service = FastSkillService::new(config).await.unwrap();
        service.initialize().await.unwrap();

        let make_args = |force: bool| AddArgs {
            source: src.display().to_string(),
            source_type: None,
            branch: None,
            tag: None,
            force,
            editable: false,
            group: None,
            recursive: false,
            reindex: false,
            no_reindex: false,
        };

        execute_add(&service, make_args(false), false)
            .await
            .expect("first add should succeed");

        // Without --force, re-adding the same skill must fail (AddMode::Fresh ->
        // AlreadyIndexed).
        let err = execute_add(&service, make_args(false), false).await;
        assert!(err.is_err(), "re-add without --force must fail");

        // With --force, it must succeed (mapped onto AddMode::Update).
        execute_add(&service, make_args(true), false)
            .await
            .expect("re-add with --force should succeed");
    }

    // ── ADR-0005 zip-url fix: a URL ending in `.zip` now installs via
    // Origin::ZipUrl instead of being misclassified as a git URL. ─────────────

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn test_execute_add_remote_zip_url_common_path_end_to_end() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let _lock = fastskill_core::test_utils::DIR_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let (_tmp, _guard, skills_dir) = setup_common_path_project();

        let server = MockServer::start().await;
        let zip_bytes = {
            use std::io::Write;
            use zip::write::FileOptions;
            let mut buf = Vec::new();
            {
                let cursor = std::io::Cursor::new(&mut buf);
                let mut writer = zip::ZipWriter::new(cursor);
                let opts =
                    FileOptions::default().compression_method(zip::CompressionMethod::Stored);
                writer.start_file("zip-url-skill/SKILL.md", opts).unwrap();
                writer
                    .write_all(
                        b"---\nname: zip-url-skill\nversion: \"1.0.0\"\ndescription: d\n---\nBody\n",
                    )
                    .unwrap();
                writer.finish().unwrap();
            }
            buf
        };
        Mock::given(method("GET"))
            .and(path("/pkg.zip"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(zip_bytes))
            .mount(&server)
            .await;

        let config = ServiceConfig {
            skill_storage_path: skills_dir.clone(),
            ..Default::default()
        };
        let mut service = FastSkillService::new(config).await.unwrap();
        service.initialize().await.unwrap();

        let args = AddArgs {
            source: format!("{}/pkg.zip", server.uri()),
            source_type: None,
            branch: None,
            tag: None,
            force: false,
            editable: false,
            group: None,
            recursive: false,
            reindex: false,
            no_reindex: false,
        };

        // Before the fix, `detect_skill_source` classified this as `GitUrl` and
        // the add would fail with a git-clone error; it must now install
        // successfully via `Origin::ZipUrl`.
        let result = execute_add(&service, args, false).await;
        assert!(result.is_ok(), "zip-url add should succeed: {:?}", result);
        assert!(skills_dir.join("zip-url-skill/SKILL.md").exists());
    }
}
