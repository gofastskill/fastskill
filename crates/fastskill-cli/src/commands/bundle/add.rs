//! Explicit bundle installation for `fastskill bundle add`.

use crate::error::{manifest_required_message, CliError, CliResult};
use crate::utils::SkillSource;
use clap::Args;
use cli_framework::command::{FromArgValueMap, IntoCommandSpec};
use cli_framework::spec::arg_spec::{ArgKind, ArgSpec, ArgValueType, Cardinality};
use cli_framework::spec::command_tree::CommandSpec;
use cli_framework::spec::value::ArgValue;
use fastskill_core::core::bundle::BundleService;
use fastskill_core::core::project::resolve_project_file;
use fastskill_core::FastSkillService;
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

#[derive(Debug, Clone, Args)]
pub struct AddArgs {
    /// Local bundle ZIP or HTTPS artifact URL
    pub artifact: String,

    /// Trigger reindex after adding the bundle
    #[arg(long)]
    pub reindex: bool,

    /// Skip reindex after adding the bundle
    #[arg(long)]
    pub no_reindex: bool,

    /// Use a local artifact without provider or index access
    #[arg(long)]
    pub offline: bool,

    /// Validate and preview without changing project state
    #[arg(long)]
    pub dry_run: bool,

    /// Emit one machine-readable lifecycle result
    #[arg(long)]
    pub json: bool,
}

impl IntoCommandSpec for AddArgs {
    fn command_spec() -> CommandSpec {
        CommandSpec {
            summary: "Add and install a bundle artifact",
            syntax: Some("bundle add <ARTIFACT> [OPTIONS]"),
            category: Some("skills-projects"),
            help_order: Some(20),
            examples: vec![
                "fastskill bundle add ./team.fastskill.zip",
                "fastskill bundle add https://example.com/team.fastskill.zip --dry-run",
            ],
            args: vec![
                ArgSpec {
                    name: "artifact",
                    kind: ArgKind::Positional,
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Required,
                    help: "Local bundle ZIP or HTTPS artifact URL",
                    ..Default::default()
                },
                flag("reindex", "Trigger reindex after adding the bundle"),
                flag("no-reindex", "Skip reindex after adding the bundle"),
                flag(
                    "offline",
                    "Use a local artifact without provider or index access",
                ),
                flag(
                    "dry-run",
                    "Validate and preview without changing project state",
                ),
                flag("json", "Emit one machine-readable lifecycle result"),
            ],
            ..Default::default()
        }
    }
}

fn flag(name: &'static str, help: &'static str) -> ArgSpec {
    ArgSpec {
        name,
        kind: ArgKind::Flag,
        long: Some(name),
        value_type: ArgValueType::Bool,
        cardinality: Cardinality::Optional,
        help,
        ..Default::default()
    }
}

impl FromArgValueMap for AddArgs {
    fn from_arg_value_map(map: &HashMap<String, ArgValue>) -> Self {
        Self {
            artifact: string_value(map, "artifact").unwrap_or_default(),
            reindex: bool_value(map, "reindex"),
            no_reindex: bool_value(map, "no-reindex"),
            offline: bool_value(map, "offline"),
            dry_run: bool_value(map, "dry-run"),
            json: bool_value(map, "json"),
        }
    }
}

fn string_value(map: &HashMap<String, ArgValue>, name: &str) -> Option<String> {
    match map.get(name) {
        Some(ArgValue::Str(value)) => Some(value.clone()),
        _ => None,
    }
}

fn bool_value(map: &HashMap<String, ArgValue>, name: &str) -> bool {
    matches!(map.get(name), Some(ArgValue::Bool(true)))
}

fn is_remote_artifact(value: &str) -> bool {
    value.starts_with("https://") || (cfg!(test) && value.starts_with("http://127.0.0.1"))
}

pub(crate) struct PreparedArtifact {
    path: PathBuf,
    _downloaded: Option<TempDir>,
}

async fn download_artifact(url: &str) -> CliResult<TempDir> {
    let response = reqwest::get(url)
        .await
        .map_err(|error| CliError::InvalidSource(format!("Failed to download '{url}': {error}")))?
        .error_for_status()
        .map_err(|error| CliError::InvalidSource(format!("Failed to download '{url}': {error}")))?;
    let bytes = response
        .bytes()
        .await
        .map_err(|error| CliError::InvalidSource(format!("Failed to read '{url}': {error}")))?;
    let temporary = TempDir::new().map_err(CliError::Io)?;
    fs::write(temporary.path().join("bundle.zip"), bytes).map_err(CliError::Io)?;
    Ok(temporary)
}

/// Reject a bundle passed through `skill add` before managed state changes.
pub(crate) async fn reject_skill_source(source: &SkillSource, offline: bool) -> CliResult<()> {
    let mut downloaded = None;
    let artifact = match source {
        SkillSource::ZipFile(path) => Some(path.clone()),
        SkillSource::RemoteZipUrl(url) if !offline && is_remote_artifact(url) => {
            let temporary = download_artifact(url).await?;
            let path = temporary.path().join("bundle.zip");
            downloaded = Some(temporary);
            Some(path)
        }
        _ => None,
    };
    if artifact
        .as_deref()
        .map(BundleService::is_bundle_artifact)
        .transpose()
        .map_err(CliError::Service)?
        .unwrap_or(false)
    {
        return Err(CliError::Validation(
            "Bundle artifacts must be added with 'fastskill bundle add <ARTIFACT>'".to_string(),
        ));
    }
    drop(downloaded);
    Ok(())
}

pub(crate) async fn preflight_add(args: &AddArgs, global: bool) -> CliResult<PreparedArtifact> {
    if global {
        return Err(CliError::Validation(
            "Bundle operations require a project Manifest and do not support --global".to_string(),
        ));
    }
    if args.reindex && args.no_reindex {
        return Err(CliError::Validation(
            "--reindex and --no-reindex cannot be used together".to_string(),
        ));
    }
    if args.offline && args.reindex {
        return Err(CliError::Validation(
            "--offline and --reindex cannot be used together".to_string(),
        ));
    }

    let mut downloaded = None;
    let artifact = if is_remote_artifact(&args.artifact) {
        if args.offline {
            return Err(CliError::Validation(
                "--offline requires a local bundle artifact".to_string(),
            ));
        }
        let temporary = download_artifact(&args.artifact).await?;
        let artifact = temporary.path().join("bundle.zip");
        downloaded = Some(temporary);
        artifact
    } else if args.artifact.contains("://") {
        return Err(CliError::Validation(
            "Bundle URLs must use HTTPS. Download private artifacts with your authenticated tool, then pass the local ZIP."
                .to_string(),
        ));
    } else {
        PathBuf::from(&args.artifact)
    };

    if !BundleService::is_bundle_artifact(&artifact).map_err(CliError::Service)? {
        return Err(CliError::Validation(
            "The artifact is not a fastskill bundle; add an individual skill with 'fastskill skill add <SOURCE>'"
                .to_string(),
        ));
    }

    Ok(PreparedArtifact {
        path: artifact,
        _downloaded: downloaded,
    })
}

#[cfg(test)]
pub async fn execute_add(service: &FastSkillService, args: AddArgs, global: bool) -> CliResult<()> {
    let artifact = preflight_add(&args, global).await?;
    execute_add_preflighted(service, args, artifact).await
}

pub(crate) async fn execute_add_preflighted(
    service: &FastSkillService,
    args: AddArgs,
    artifact: PreparedArtifact,
) -> CliResult<()> {
    let current = env::current_dir().map_err(|error| {
        CliError::Config(format!("Failed to determine current directory: {error}"))
    })?;
    let project = resolve_project_file(&current);
    if !project.found {
        return Err(CliError::Config(manifest_required_message().to_string()));
    }
    let root = project.path.parent().ok_or_else(|| {
        CliError::Config("skill-project.toml has no project directory".to_string())
    })?;
    install_local(service, root, &artifact.path, &args).await
}

async fn install_local(
    service: &FastSkillService,
    project_root: &Path,
    artifact: &Path,
    args: &AddArgs,
) -> CliResult<()> {
    let bundles = BundleService::new(project_root, service.config().skill_storage_path.clone());
    let preview = bundles.plan_install(artifact).map_err(CliError::Service)?;
    if args.dry_run {
        if args.json {
            emit_add_json(&preview, true, None)?;
        } else if preview.changes.is_empty() {
            crate::outln!(
                "Bundle {}@{} is already installed; no changes were applied",
                preview.id,
                preview.target_revision
            );
        } else {
            crate::outln!(
                "Would install bundle {}@{}:",
                preview.id,
                preview.target_revision
            );
            for change in &preview.changes {
                crate::outln!("  {change}");
            }
            crate::outln!("No changes were applied");
        }
        return Ok(());
    }

    let outcome = bundles.install(artifact).map_err(CliError::Service)?;
    if !args.json {
        if outcome.unchanged {
            crate::outln!(
                "Bundle {}@{} is already installed",
                outcome.id,
                outcome.version
            );
        } else {
            crate::outln!("Installed bundle {}@{}", outcome.id, outcome.version);
            crate::outln!(
                "{}",
                crate::utils::messages::ok("Updated skill-project.toml and skills.lock")
            );
        }
    }

    let indexing = crate::utils::reindex_utils::lifecycle_reindex_result(
        service,
        "bundle add",
        args.reindex,
        args.no_reindex || args.offline,
        crate::config_file::load_auto_reindex_config(),
    )
    .await;
    if args.json {
        emit_add_json(&preview, false, Some(&indexing))?;
    } else {
        crate::utils::reindex_utils::report_lifecycle_index_result(&indexing);
    }
    Ok(())
}

fn emit_add_json(
    preview: &fastskill_core::core::bundle::BundleInstallPreview,
    dry_run: bool,
    indexing: Option<&crate::utils::reindex_utils::LifecycleIndexResult>,
) -> CliResult<()> {
    let changed = !preview.changes.is_empty();
    crate::outln!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "scope": "project",
            "outcome": if changed { "changed" } else { "unchanged" },
            "dry_run": dry_run,
            "targets": [{
                "id": preview.id,
                "outcome": if changed { "changed" } else { "unchanged" },
                "current_revision": preview.current_revision,
                "target_revision": preview.target_revision,
                "changes": preview.changes,
                "retained": preview.retained,
            }],
            "diagnostics": Vec::<String>::new(),
            "indexing": indexing,
        }))
        .map_err(|error| CliError::Config(format!(
            "Failed to serialize bundle add result: {error}"
        )))?
    );
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};
    use zip::write::SimpleFileOptions;

    fn ordinary_skill_zip() -> Vec<u8> {
        let cursor = Cursor::new(Vec::new());
        let mut writer = zip::ZipWriter::new(cursor);
        writer
            .start_file("SKILL.md", SimpleFileOptions::default())
            .unwrap();
        writer
            .write_all(b"---\nname: ordinary\nversion: 1.0.0\n---\n# ordinary\n")
            .unwrap();
        writer.finish().unwrap().into_inner()
    }

    #[tokio::test]
    async fn bundle_add_rejects_individual_skill_artifact() {
        let root = tempfile::tempdir().unwrap();
        let artifact = root.path().join("skill.zip");
        fs::write(&artifact, ordinary_skill_zip()).unwrap();
        let service = FastSkillService::new(fastskill_core::ServiceConfig {
            skill_storage_path: root.path().join("skills"),
            ..Default::default()
        })
        .await
        .unwrap();
        let error = execute_add(
            &service,
            AddArgs {
                artifact: artifact.display().to_string(),
                reindex: false,
                no_reindex: true,
                offline: true,
                dry_run: true,
                json: false,
            },
            false,
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("fastskill skill add"));
    }

    #[test]
    fn argument_map_uses_explicit_bundle_schema() {
        let args = AddArgs::from_arg_value_map(&HashMap::from([
            (
                "artifact".to_string(),
                ArgValue::Str("team.zip".to_string()),
            ),
            ("offline".to_string(), ArgValue::Bool(true)),
            ("dry-run".to_string(), ArgValue::Bool(true)),
        ]));
        assert_eq!(args.artifact, "team.zip");
        assert!(args.offline && args.dry_run);
    }
}
