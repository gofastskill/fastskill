use crate::error::{CliError, CliResult};

pub(super) fn emit_bundle_update_result(
    preview: &fastskill_core::core::bundle::BundleUpdatePreview,
    dry_run: bool,
    indexing: &crate::utils::reindex_utils::LifecycleIndexResult,
) -> CliResult<()> {
    let outcome = if preview.changes.is_empty() {
        "unchanged"
    } else {
        "changed"
    };
    let rendered = serde_json::to_string_pretty(&serde_json::json!({
        "scope": "project",
        "outcome": outcome,
        "dry_run": dry_run,
        "targets": [{
            "id": preview.id,
            "outcome": outcome,
            "current_revision": preview.current_revision,
            "target_revision": preview.target_revision,
            "changes": preview.changes,
            "retained": preview.retained,
        }],
        "diagnostics": Vec::<String>::new(),
        "indexing": indexing,
    }))
    .map_err(|error| CliError::Config(format!("Failed to serialize bundle update: {error}")))?;
    crate::outln!("{rendered}");
    Ok(())
}

pub(super) fn print_update_json(
    previews: &[crate::commands::install::change::ChangePreview],
    dry_run: bool,
    refreshed_repositories: &[String],
    indexing: &crate::utils::reindex_utils::LifecycleIndexResult,
) -> CliResult<()> {
    let targets = previews
        .iter()
        .map(|preview| {
            let changed = !preview.changes.is_empty();
            serde_json::json!({
                "id": preview.id,
                "outcome": if changed { "changed" } else { "unchanged" },
                "current_revision": preview.previous_version,
                "target_revision": preview.resolved_version,
                "changes": preview.changes,
                "retained": Vec::<String>::new()
            })
        })
        .collect::<Vec<_>>();
    let changed = targets.iter().any(|target| target["outcome"] == "changed");
    let diagnostics = previews
        .iter()
        .flat_map(|preview| {
            preview
                .warnings
                .iter()
                .map(move |warning| format!("{}: {warning}", preview.id))
        })
        .collect::<Vec<_>>();
    crate::outln!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "scope": "project",
            "outcome": if changed { "changed" } else { "unchanged" },
            "dry_run": dry_run,
            "targets": targets,
            "diagnostics": diagnostics,
            "resolution": {
                "source": if refreshed_repositories.is_empty() { "cached" } else { "refreshed" },
                "refreshed_repositories": refreshed_repositories
            },
            "indexing": indexing
        }))
        .map_err(|error| CliError::Config(format!("Failed to serialize update result: {error}")))?
    );
    Ok(())
}

pub(super) fn render_update_previews(previews: &[crate::commands::install::change::ChangePreview]) {
    for preview in previews {
        let current = preview
            .previous_version
            .as_deref()
            .unwrap_or("not installed");
        let direction = match preview.previous_version.as_deref() {
            Some(previous) if previous == preview.resolved_version && preview.origin_changed => {
                "provenance change"
            }
            Some(previous) if previous == preview.resolved_version && preview.resolved_changed => {
                "content change"
            }
            Some(previous) if previous == preview.resolved_version => "unchanged",
            Some(previous)
                if semver::Version::parse(previous).ok()
                    > semver::Version::parse(&preview.resolved_version).ok() =>
            {
                "downgrade"
            }
            Some(_) => "upgrade",
            None => "install",
        };
        crate::outln!(
            "  • {}: {} -> {} ({})",
            preview.id,
            current,
            preview.resolved_version,
            direction
        );
        for warning in &preview.warnings {
            eprintln!("  {}", crate::utils::messages::warning(warning));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::install::change::ChangePreview;
    use crate::utils::reindex_utils::LifecycleIndexResult;

    fn preview(
        previous: Option<&str>,
        resolved: &str,
        origin_changed: bool,
        resolved_changed: bool,
    ) -> ChangePreview {
        ChangePreview {
            id: "demo".to_string(),
            previous_version: previous.map(str::to_string),
            resolved_version: resolved.to_string(),
            origin_changed,
            resolved_changed,
            changes: if previous == Some(resolved) && !origin_changed && !resolved_changed {
                Vec::new()
            } else {
                vec!["planned".to_string()]
            },
            warnings: vec!["portable path warning".to_string()],
        }
    }

    fn skipped_indexing() -> LifecycleIndexResult {
        LifecycleIndexResult {
            outcome: "skipped",
            count: 0,
            diagnostic: Some("test".to_string()),
        }
    }

    #[tokio::test]
    async fn json_outputs_cover_unchanged_and_refreshed_results() {
        let bundle = fastskill_core::core::bundle::BundleUpdatePreview {
            id: "bundle".to_string(),
            current_revision: "1.0.0".to_string(),
            target_revision: "1.0.0".to_string(),
            changes: Vec::new(),
            retained: vec!["demo".to_string()],
        };
        let (_, bundle_json) = crate::output::capture(async {
            emit_bundle_update_result(&bundle, true, &skipped_indexing()).unwrap();
        })
        .await;
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&bundle_json).unwrap()["outcome"],
            "unchanged"
        );

        let previews = [preview(Some("1.0.0"), "1.0.0", false, false)];
        let (_, update_json) = crate::output::capture(async {
            print_update_json(&previews, false, &["team".to_string()], &skipped_indexing())
                .unwrap();
        })
        .await;
        let value: serde_json::Value = serde_json::from_str(&update_json).unwrap();
        assert_eq!(value["outcome"], "unchanged");
        assert_eq!(value["resolution"]["source"], "refreshed");
        assert!(value["diagnostics"][0]
            .as_str()
            .unwrap()
            .contains("portable path warning"));
    }

    #[tokio::test]
    async fn human_preview_describes_each_change_direction() {
        let previews = [
            preview(Some("1.0.0"), "1.0.0", true, false),
            preview(Some("1.0.0"), "1.0.0", false, true),
            preview(Some("1.0.0"), "1.0.0", false, false),
            preview(Some("2.0.0"), "1.0.0", false, true),
            preview(Some("1.0.0"), "2.0.0", false, true),
            preview(None, "1.0.0", false, true),
        ];
        let (_, rendered) = crate::output::capture(async {
            render_update_previews(&previews);
        })
        .await;
        for direction in [
            "provenance change",
            "content change",
            "unchanged",
            "downgrade",
            "upgrade",
            "install",
        ] {
            assert!(
                rendered.contains(direction),
                "missing {direction}: {rendered}"
            );
        }
    }
}
