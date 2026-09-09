use crate::error::{CliError, CliResult};

pub(super) fn emit_bundle_removal(
    preview: &fastskill_core::core::bundle::BundleRemovalPreview,
    dry_run: bool,
    json: bool,
) -> CliResult<()> {
    let mut changes = preview
        .delete_members
        .iter()
        .map(|id| format!("delete {id}"))
        .collect::<Vec<_>>();
    changes.extend(
        preview
            .promoted_overrides
            .iter()
            .map(|id| format!("promote override {id}")),
    );
    changes.push("manifest".to_string());
    changes.push("lock".to_string());
    if json {
        return emit_json_result(
            "project",
            dry_run,
            vec![serde_json::json!({
                "id": preview.id,
                "outcome": "changed",
                "current_revision": preview.current_revision,
                "target_revision": serde_json::Value::Null,
                "changes": changes,
                "retained": preview.retained_members,
            })],
            "changed",
        );
    }
    crate::outln!(
        "Would remove bundle {}@{}; no changes were applied",
        preview.id,
        preview.current_revision
    );
    if !preview.retained_members.is_empty() {
        crate::outln!("Retained members: {}", preview.retained_members.join(", "));
    }
    Ok(())
}

pub(super) fn emit_project_removal(
    requested: &[String],
    plan: &fastskill_core::core::ownership::ProjectRemovalPlan,
    dry_run: bool,
    json: bool,
) -> CliResult<()> {
    let changed = !plan.remove_manifest_dependencies.is_empty()
        || !plan.remove_lock_entries.is_empty()
        || !plan.delete_files.is_empty();
    let outcome = if changed { "changed" } else { "unchanged" };
    if json {
        let targets = requested
            .iter()
            .map(|id| {
                let target_outcome = if plan.unchanged.contains(id) {
                    "unchanged"
                } else {
                    "changed"
                };
                let mut changes = Vec::new();
                if plan.remove_manifest_dependencies.contains(id) {
                    changes.push("manifest");
                }
                if plan.remove_lock_entries.contains(id) {
                    changes.push("lock");
                }
                if plan.delete_files.contains(id) {
                    changes.push("delete files");
                }
                serde_json::json!({
                    "id": id,
                    "outcome": target_outcome,
                    "current_revision": serde_json::Value::Null,
                    "target_revision": serde_json::Value::Null,
                    "changes": changes,
                    "retained": plan.retained_files,
                })
            })
            .collect();
        return emit_json_result("project", dry_run, targets, outcome);
    }
    if changed {
        crate::outln!(
            "Would remove {}; no changes were applied",
            requested.join(", ")
        );
        if !plan.retained_files.is_empty() {
            crate::outln!("Retained skills: {}", plan.retained_files.join(", "));
        }
    } else {
        crate::outln!("No requested skills are installed; no changes would be made");
    }
    Ok(())
}

pub(super) fn emit_global_removal_plan(
    requested: &[String],
    plan: &fastskill_core::core::global_ownership::GlobalRemovalPlan,
    dry_run: bool,
    json: bool,
) -> CliResult<()> {
    let changed = !plan.remove_roots.is_empty() || !plan.remove_lock_entries.is_empty();
    let outcome = if changed { "changed" } else { "unchanged" };
    if json {
        let targets = requested
            .iter()
            .map(|id| {
                let unchanged = plan.unchanged.contains(id);
                let mut changes = Vec::new();
                if plan.remove_roots.contains(id) {
                    changes.push("global root");
                }
                if plan.remove_lock_entries.contains(id) {
                    changes.push("global lock");
                }
                if plan.delete_files.contains(id) {
                    changes.push("delete files");
                }
                serde_json::json!({
                    "id": id,
                    "outcome": if unchanged { "unchanged" } else { "changed" },
                    "current_revision": serde_json::Value::Null,
                    "target_revision": serde_json::Value::Null,
                    "changes": changes,
                    "retained": plan.retained_files,
                })
            })
            .collect();
        return emit_json_result("global", dry_run, targets, outcome);
    }
    if changed {
        crate::outln!(
            "Would remove {}; no changes were applied",
            requested.join(", ")
        );
        if !plan.retained_files.is_empty() {
            crate::outln!("Retained skills: {}", plan.retained_files.join(", "));
        }
    } else {
        crate::outln!("No requested global skills are installed; no changes would be made");
    }
    Ok(())
}

fn emit_json_result(
    scope: &str,
    dry_run: bool,
    targets: Vec<serde_json::Value>,
    outcome: &str,
) -> CliResult<()> {
    let rendered = serde_json::to_string_pretty(&serde_json::json!({
        "scope": scope,
        "outcome": outcome,
        "dry_run": dry_run,
        "targets": targets,
        "diagnostics": Vec::<String>::new(),
    }))
    .map_err(|error| CliError::Config(format!("Failed to serialize removal result: {error}")))?;
    crate::outln!("{rendered}");
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use fastskill_core::core::bundle::BundleRemovalPreview;
    use fastskill_core::core::global_ownership::GlobalRemovalPlan;
    use fastskill_core::core::ownership::ProjectRemovalPlan;

    #[tokio::test]
    async fn bundle_output_includes_changes_and_retained_members() {
        let preview = BundleRemovalPreview {
            id: "team".to_string(),
            current_revision: "release".to_string(),
            delete_members: vec!["removed".to_string()],
            retained_members: vec!["shared".to_string()],
            promoted_overrides: vec!["personal".to_string()],
        };
        let (result, json) =
            crate::output::capture(async { emit_bundle_removal(&preview, true, true) }).await;
        result.unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["scope"], "project");
        assert_eq!(value["targets"][0]["retained"][0], "shared");
        assert_eq!(value["targets"][0]["changes"][0], "delete removed");

        let (result, human) =
            crate::output::capture(async { emit_bundle_removal(&preview, true, false) }).await;
        result.unwrap();
        assert!(human.contains("Would remove bundle team@release"));
        assert!(human.contains("Retained members: shared"));
    }

    #[tokio::test]
    async fn project_output_distinguishes_changed_retained_and_unchanged_targets() {
        let changed = ProjectRemovalPlan {
            remove_manifest_dependencies: vec!["root".to_string()],
            remove_lock_entries: vec!["root".to_string()],
            delete_files: vec!["root".to_string()],
            retained_files: vec!["shared".to_string()],
            unchanged: vec!["absent".to_string()],
            remaining_individual_roots: Vec::new(),
        };
        let requested = vec!["root".to_string(), "absent".to_string()];
        let (result, json) = crate::output::capture(async {
            emit_project_removal(&requested, &changed, false, true)
        })
        .await;
        result.unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["outcome"], "changed");
        assert_eq!(value["targets"][0]["changes"].as_array().unwrap().len(), 3);
        assert_eq!(value["targets"][1]["outcome"], "unchanged");

        let (result, human) = crate::output::capture(async {
            emit_project_removal(&requested, &changed, true, false)
        })
        .await;
        result.unwrap();
        assert!(human.contains("Would remove root, absent"));
        assert!(human.contains("Retained skills: shared"));

        let unchanged = ProjectRemovalPlan {
            unchanged: requested.clone(),
            ..changed.clone()
        };
        let mut unchanged = unchanged;
        unchanged.remove_manifest_dependencies.clear();
        unchanged.remove_lock_entries.clear();
        unchanged.delete_files.clear();
        let (result, human) = crate::output::capture(async {
            emit_project_removal(&requested, &unchanged, true, false)
        })
        .await;
        result.unwrap();
        assert!(human.contains("No requested skills are installed"));
    }

    #[tokio::test]
    async fn global_output_covers_changed_and_explicit_noop_results() {
        let changed = GlobalRemovalPlan {
            remove_roots: vec!["root".to_string()],
            remove_lock_entries: vec!["root".to_string()],
            delete_files: vec!["root".to_string()],
            retained_files: vec!["shared".to_string()],
            unchanged: vec!["absent".to_string()],
            remaining_roots: vec!["other".to_string()],
        };
        let requested = vec!["root".to_string(), "absent".to_string()];
        let (result, json) = crate::output::capture(async {
            emit_global_removal_plan(&requested, &changed, false, true)
        })
        .await;
        result.unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["scope"], "global");
        assert_eq!(value["targets"][0]["changes"].as_array().unwrap().len(), 3);
        assert_eq!(value["targets"][1]["outcome"], "unchanged");

        let (result, human) = crate::output::capture(async {
            emit_global_removal_plan(&requested, &changed, true, false)
        })
        .await;
        result.unwrap();
        assert!(human.contains("Retained skills: shared"));

        let unchanged = GlobalRemovalPlan {
            remove_roots: Vec::new(),
            remove_lock_entries: Vec::new(),
            delete_files: Vec::new(),
            retained_files: Vec::new(),
            unchanged: requested.clone(),
            remaining_roots: Vec::new(),
        };
        let (result, human) = crate::output::capture(async {
            emit_global_removal_plan(&requested, &unchanged, true, false)
        })
        .await;
        result.unwrap();
        assert!(human.contains("No requested global skills are installed"));
    }
}
