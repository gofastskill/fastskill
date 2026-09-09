use crate::error::{CliError, CliResult};
use fastskill_core::core::manifest::SkillProjectToml;
use fastskill_core::core::origin::Origin;
use fastskill_core::core::project::resolve_project_file;
use fastskill_core::FastSkillService;
use std::env;
use std::path::Path;

pub(super) struct ProjectAddOutcome {
    pub id: String,
    pub current_version: Option<String>,
    pub version: String,
    pub changes: Vec<String>,
    pub warnings: Vec<String>,
    pub refreshed_repositories: Vec<String>,
}

pub(super) async fn add_project_skill(
    service: &FastSkillService,
    origin: Origin,
    groups: Vec<String>,
    force: bool,
    offline: bool,
    dry_run: bool,
) -> CliResult<ProjectAddOutcome> {
    let mut outcomes =
        add_project_skills(service, vec![origin], groups, force, offline, dry_run).await?;
    outcomes
        .pop()
        .ok_or_else(|| CliError::Config("Add planning produced no selected skill".to_string()))
}

pub(super) async fn add_project_skills(
    service: &FastSkillService,
    origins: Vec<Origin>,
    groups: Vec<String>,
    force: bool,
    offline: bool,
    dry_run: bool,
) -> CliResult<Vec<ProjectAddOutcome>> {
    let current_dir = env::current_dir()
        .map_err(|error| CliError::Config(format!("Failed to get current directory: {error}")))?;
    let project_file = resolve_project_file(&current_dir).path;
    let manifest_dir = project_file.parent().unwrap_or(Path::new("."));
    let lock_path = manifest_dir.join("skills.lock");
    let project = SkillProjectToml::load_from_file(&project_file)
        .map_err(|error| CliError::Config(format!("Failed to load skill-project.toml: {error}")))?;
    let max_levels = project
        .tool
        .as_ref()
        .and_then(|tool| tool.fastskill.as_ref())
        .map_or(5, |config| config.install_depth);
    let declared = project
        .dependencies
        .as_ref()
        .map(|dependencies| &dependencies.dependencies);
    if !force {
        for origin in &origins {
            if let Origin::Repository { skill, .. } = origin {
                let unscoped = skill.rsplit('/').next().unwrap_or(skill);
                if declared.is_some_and(|dependencies| {
                    dependencies.contains_key(skill) || dependencies.contains_key(unscoped)
                }) {
                    return Err(CliError::Config(format!(
                        "Skill '{skill}' is already declared. Use --force to replace it."
                    )));
                }
            }
        }
    }
    // For sources whose identity is only known after acquisition, resolve into
    // disposable storage first. A duplicate rejection must not update the real
    // repository index or cache.
    let isolated_preflight = !force
        && declared.is_some_and(|dependencies| !dependencies.is_empty())
        && origins
            .iter()
            .any(|origin| !matches!(origin, Origin::Repository { .. }));
    let roots = || {
        origins
            .iter()
            .cloned()
            .map(|origin| crate::commands::install::change::ChangeRoot {
                expected_id: match &origin {
                    Origin::Repository { skill, .. } => Some(skill.clone()),
                    _ => None,
                },
                origin,
                groups: groups.clone(),
                locked: None,
            })
            .collect()
    };
    let (mut prepared, mut preview) = crate::commands::install::change::prepare_changes(
        service,
        &lock_path,
        manifest_dir,
        roots(),
        max_levels,
        offline,
        dry_run || isolated_preflight,
    )
    .await?;
    for selected in &preview {
        let already_declared = project
            .dependencies
            .as_ref()
            .is_some_and(|dependencies| dependencies.dependencies.contains_key(&selected.id));
        if already_declared && !force {
            return Err(CliError::Config(format!(
                "Skill '{}' is already declared. Use --force to replace it.",
                selected.id
            )));
        }
    }
    if isolated_preflight && !dry_run {
        (prepared, preview) = crate::commands::install::change::prepare_changes(
            service,
            &lock_path,
            manifest_dir,
            roots(),
            max_levels,
            offline,
            false,
        )
        .await?;
    }
    let planned_refreshes = prepared.refreshed_repositories().to_vec();
    let refreshed_repositories = if dry_run {
        crate::commands::install::plan::validate_prepared(service, manifest_dir, prepared)?;
        planned_refreshes
    } else {
        crate::commands::install::plan::apply_prepared(service, &lock_path, manifest_dir, prepared)
            .await?
            .refreshed_repositories
    };
    Ok(preview
        .into_iter()
        .map(|selected| ProjectAddOutcome {
            id: selected.id,
            current_version: selected.previous_version,
            version: selected.resolved_version,
            changes: selected.changes,
            warnings: selected.warnings,
            refreshed_repositories: refreshed_repositories.clone(),
        })
        .collect())
}

pub(super) async fn emit_project_add(
    service: &FastSkillService,
    outcome: &ProjectAddOutcome,
    dry_run: bool,
    json: bool,
    indexing: Option<&crate::utils::reindex_utils::LifecycleIndexResult>,
) -> CliResult<bool> {
    let changed = !outcome.changes.is_empty();
    if json {
        crate::outln!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "scope": "project",
                "outcome": if changed { "changed" } else { "unchanged" },
                "dry_run": dry_run,
                "targets": [{
                    "id": outcome.id,
                    "outcome": if changed { "changed" } else { "unchanged" },
                    "current_revision": outcome.current_version,
                    "target_revision": outcome.version,
                    "changes": outcome.changes,
                    "retained": Vec::<String>::new()
                }],
                "diagnostics": outcome.warnings,
                "resolution": {
                    "source": if outcome.refreshed_repositories.is_empty() {
                        "cached"
                    } else {
                        "refreshed"
                    },
                    "refreshed_repositories": outcome.refreshed_repositories
                },
                "indexing": indexing
            }))
            .map_err(|error| CliError::Config(format!(
                "Failed to serialize add result: {error}"
            )))?
        );
        return Ok(true);
    }
    if dry_run {
        if changed {
            crate::outln!(
                "Would add {} at revision {}; no changes were applied",
                outcome.id,
                outcome.version
            );
        } else {
            crate::outln!(
                "{} at revision {} is already satisfied; no changes were applied",
                outcome.id,
                outcome.version
            );
        }
        for repository in &outcome.refreshed_repositories {
            crate::outln!("Resolved from refreshed repository metadata: {repository}");
        }
        return Ok(true);
    }
    let display_name = match fastskill_core::SkillId::new(outcome.id.clone()) {
        Ok(id) => service
            .skill_manager()
            .get_skill(&id)
            .await
            .ok()
            .flatten()
            .map(|skill| skill.name)
            .unwrap_or_else(|| outcome.id.clone()),
        Err(_) => outcome.id.clone(),
    };
    crate::outln!(
        "Successfully added skill: {} (v{})",
        display_name,
        outcome.version
    );
    crate::outln!(
        "{}",
        crate::utils::messages::ok("Updated skill-project.toml and skills.lock")
    );
    for warning in &outcome.warnings {
        eprintln!("{}", crate::utils::messages::warning(warning));
    }
    for repository in &outcome.refreshed_repositories {
        crate::outln!("   Refreshed repository metadata: {repository}");
    }
    Ok(false)
}

pub(super) fn emit_project_adds(
    outcomes: &[ProjectAddOutcome],
    dry_run: bool,
    json: bool,
    indexing: Option<&crate::utils::reindex_utils::LifecycleIndexResult>,
) -> CliResult<()> {
    if json {
        let targets = outcomes
            .iter()
            .map(|outcome| {
                let changed = !outcome.changes.is_empty();
                serde_json::json!({
                    "id": outcome.id,
                    "outcome": if changed { "changed" } else { "unchanged" },
                    "current_revision": outcome.current_version,
                    "target_revision": outcome.version,
                    "changes": outcome.changes,
                    "retained": Vec::<String>::new()
                })
            })
            .collect::<Vec<_>>();
        let changed = targets.iter().any(|target| target["outcome"] == "changed");
        let diagnostics = outcomes
            .iter()
            .flat_map(|outcome| outcome.warnings.iter().cloned())
            .collect::<Vec<_>>();
        let refreshed = outcomes
            .first()
            .map(|outcome| outcome.refreshed_repositories.as_slice())
            .unwrap_or_default();
        crate::outln!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "scope": "project",
                "outcome": if changed { "changed" } else { "unchanged" },
                "dry_run": dry_run,
                "targets": targets,
                "diagnostics": diagnostics,
                "resolution": {
                    "source": if refreshed.is_empty() { "cached" } else { "refreshed" },
                    "refreshed_repositories": refreshed
                },
                "indexing": indexing
            }))
            .map_err(|error| CliError::Config(format!(
                "Failed to serialize recursive add result: {error}"
            )))?
        );
        return Ok(());
    }
    for outcome in outcomes {
        if dry_run {
            if outcome.changes.is_empty() {
                crate::outln!("{} is already satisfied", outcome.id);
            } else {
                crate::outln!("Would add {} at revision {}", outcome.id, outcome.version);
            }
        } else if outcome.changes.is_empty() {
            crate::outln!("{} is already satisfied", outcome.id);
        } else {
            crate::outln!(
                "Successfully added skill: {} (v{})",
                outcome.id,
                outcome.version
            );
        }
    }
    if dry_run {
        crate::outln!("No changes were applied");
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use fastskill_core::ServiceConfig;
    use tempfile::TempDir;

    fn outcome(changed: bool) -> ProjectAddOutcome {
        ProjectAddOutcome {
            id: "demo".to_string(),
            current_version: Some("1.0.0".to_string()),
            version: "2.0.0".to_string(),
            changes: changed
                .then(|| "resolved content changed".to_string())
                .into_iter()
                .collect(),
            warnings: vec!["portable warning".to_string()],
            refreshed_repositories: vec!["community".to_string()],
        }
    }

    #[tokio::test]
    async fn single_add_output_is_truthful_for_changed_and_unchanged_previews() {
        let temp = TempDir::new().unwrap();
        let mut service = FastSkillService::new(ServiceConfig {
            skill_storage_path: temp.path().to_path_buf(),
            ..Default::default()
        })
        .await
        .unwrap();
        service.initialize().await.unwrap();
        for (changed, json) in [(true, true), (true, false), (false, false)] {
            assert!(
                emit_project_add(&service, &outcome(changed), true, json, None)
                    .await
                    .unwrap()
            );
        }
        assert!(
            !emit_project_add(&service, &outcome(true), false, false, None)
                .await
                .unwrap()
        );
    }

    #[test]
    fn recursive_add_output_covers_json_and_human_outcomes() {
        let outcomes = [outcome(true), outcome(false)];
        assert!(emit_project_adds(&outcomes, true, true, None).is_ok());
        assert!(emit_project_adds(&outcomes, true, false, None).is_ok());
        assert!(emit_project_adds(&outcomes, false, false, None).is_ok());
        assert!(emit_project_adds(&[], false, true, None).is_ok());
    }
}
