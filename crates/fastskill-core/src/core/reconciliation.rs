//! Reconciliation types for comparing installed skills with project/lock files
//!
//! These types are used by both the CLI and tests to reconcile installed skills
//! against skills-project.toml and skills-lock.toml

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Installed skill information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledSkillInfo {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub source: Option<String>,
    pub installed_path: PathBuf,
    pub installed_at: Option<chrono::DateTime<chrono::Utc>>,
    pub status: ReconciliationStatus,
}

/// Reconciliation status for installed skills
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReconciliationStatus {
    Ok,
    Missing,    // In project.toml but not installed
    Extraneous, // Installed but not in project.toml
    Mismatch,   // Version mismatch with lock.toml
}

/// Reconciliation report
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReconciliationReport {
    pub installed: Vec<InstalledSkillInfo>,
    pub missing: Vec<DesiredEntry>,
    pub extraneous: Vec<InstalledSkillInfo>,
    pub version_mismatches: Vec<VersionMismatch>,
}

/// Desired entry from skills-project.toml
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DesiredEntry {
    pub id: String,
    pub version: Option<String>, // Version constraint
}

/// Version mismatch information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionMismatch {
    pub id: String,
    pub installed_version: String,
    pub locked_version: String,
}

use crate::core::origin::Origin;
use crate::core::skill_manager::SkillDefinition;
use crate::core::version::VersionConstraint;
use std::collections::HashMap;
use std::path::Path;

/// A short human-readable description of where a skill came from, derived from
/// its `Origin`. Used only for display in the reconciliation report.
fn origin_display(origin: &Origin) -> String {
    match origin {
        Origin::Git { url, .. } => url.clone(),
        Origin::Local { path, .. } => path.display().to_string(),
        Origin::ZipUrl { url } => url.clone(),
        Origin::Repository { repo, skill, .. } => format!("{repo}/{skill}"),
    }
}

/// Build reconciliation report
pub fn build_reconciliation_report(
    installed_skills: &[SkillDefinition],
    project_deps: &HashMap<String, Option<String>>,
    lock_deps: &HashMap<String, String>,
    _skills_dir: &Path,
) -> Result<ReconciliationReport, crate::core::service::ServiceError> {
    let mut installed_info = Vec::new();
    let mut missing = Vec::new();
    let mut extraneous = Vec::new();
    let mut version_mismatches = Vec::new();

    // Build map of installed skills by ID
    let installed_map: HashMap<String, &SkillDefinition> = installed_skills
        .iter()
        .map(|s| (s.id.to_string(), s))
        .collect();

    // Check for missing dependencies (in project but not installed)
    for id in project_deps.keys() {
        if !installed_map.contains_key(id) {
            missing.push(DesiredEntry {
                id: id.clone(),
                version: None, // Version constraint not used for missing detection
            });
        }
    }

    // Process installed skills
    for skill in installed_skills {
        let skill_id = skill.id.to_string();
        let is_in_project = project_deps.contains_key(&skill_id);
        let locked_version = lock_deps.get(&skill_id);
        // The declared project version constraint, if any (Some(Some(constraint))).
        let project_constraint = project_deps.get(&skill_id).and_then(|c| c.as_ref());

        // Determine status. Both the project version constraint (BUG-9) and the
        // lock-equality check can drive a Mismatch. The decision carries the
        // expected-version string alongside the status so the reporting branch
        // doesn't have to re-derive it — `expected` is always `Some` whenever the
        // status is `Mismatch`.
        let (status, expected): (ReconciliationStatus, Option<String>) = if !is_in_project {
            (ReconciliationStatus::Extraneous, None)
        } else {
            // First: does the installed version satisfy the declared constraint?
            let violates_constraint = match project_constraint {
                Some(constraint_str) => match VersionConstraint::parse(constraint_str) {
                    // An installed version outside its declared constraint is a Mismatch.
                    Ok(constraint) => !constraint.satisfies(&skill.version).unwrap_or(true),
                    // Unparseable constraint: don't spuriously flag as mismatch.
                    Err(_) => false,
                },
                None => false,
            };

            if violates_constraint {
                // Report the constraint the installed version failed to satisfy.
                (ReconciliationStatus::Mismatch, project_constraint.cloned())
            } else if let Some(locked_ver) = locked_version {
                // Additional signal: lock-file version equality.
                if skill.version != *locked_ver {
                    (ReconciliationStatus::Mismatch, Some(locked_ver.clone()))
                } else {
                    (ReconciliationStatus::Ok, None)
                }
            } else {
                (ReconciliationStatus::Ok, None)
            }
        };

        // Collect extraneous and mismatches
        match &status {
            ReconciliationStatus::Extraneous => {
                extraneous.push(InstalledSkillInfo {
                    id: skill_id.clone(),
                    name: skill.name.clone(),
                    version: skill.version.clone(),
                    description: skill.description.clone(),
                    source: Some(origin_display(&skill.origin)),
                    installed_path: skill.skill_file.clone(),
                    installed_at: Some(skill.updated_at),
                    status: status.clone(),
                });
            }
            ReconciliationStatus::Mismatch => {
                version_mismatches.push(VersionMismatch {
                    id: skill_id.clone(),
                    installed_version: skill.version.clone(),
                    // The violated constraint or the differing locked version;
                    // always populated on the Mismatch paths above.
                    locked_version: expected.clone().unwrap_or_default(),
                });
            }
            _ => {}
        }

        // Add to installed list
        installed_info.push(InstalledSkillInfo {
            id: skill_id,
            name: skill.name.clone(),
            version: skill.version.clone(),
            description: skill.description.clone(),
            source: Some(origin_display(&skill.origin)),
            installed_path: skill.skill_file.clone(),
            installed_at: Some(skill.updated_at),
            status,
        });
    }

    Ok(ReconciliationReport {
        installed: installed_info,
        missing,
        extraneous,
        version_mismatches,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::core::service::SkillId;
    use crate::core::skill_manager::SkillDefinition;

    fn skill(id: &str, version: &str) -> SkillDefinition {
        SkillDefinition::new(
            SkillId::new(id.to_string()).unwrap(),
            id.to_string(),
            "desc".to_string(),
            version.to_string(),
            Origin::Local {
                path: std::path::PathBuf::from(format!("./skills/{id}")),
                editable: false,
            },
        )
    }

    fn status_of<'a>(report: &'a ReconciliationReport, id: &str) -> &'a ReconciliationStatus {
        &report
            .installed
            .iter()
            .find(|s| s.id == id)
            .expect("skill present in report")
            .status
    }

    #[test]
    fn test_constraint_satisfied_is_ok() {
        let installed = vec![skill("a", "1.2.3")];
        let mut project = HashMap::new();
        project.insert("a".to_string(), Some(">=1.0.0,<2.0.0".to_string()));
        let lock = HashMap::new();

        let report =
            build_reconciliation_report(&installed, &project, &lock, Path::new(".")).unwrap();

        assert!(matches!(status_of(&report, "a"), ReconciliationStatus::Ok));
        assert!(report.version_mismatches.is_empty());
    }

    #[test]
    fn test_constraint_violated_is_mismatch() {
        // BUG-9: installed 2.0.0 violates <2.0.0 and must be flagged.
        let installed = vec![skill("a", "2.0.0")];
        let mut project = HashMap::new();
        project.insert("a".to_string(), Some(">=1.0.0,<2.0.0".to_string()));
        let lock = HashMap::new();

        let report =
            build_reconciliation_report(&installed, &project, &lock, Path::new(".")).unwrap();

        assert!(matches!(
            status_of(&report, "a"),
            ReconciliationStatus::Mismatch
        ));
        assert_eq!(report.version_mismatches.len(), 1);
        let mm = &report.version_mismatches[0];
        assert_eq!(mm.id, "a");
        assert_eq!(mm.installed_version, "2.0.0");
        // The reported "expected" is the violated constraint.
        assert_eq!(mm.locked_version, ">=1.0.0,<2.0.0");
    }

    #[test]
    fn test_bare_constraint_exact_pin_violation() {
        // ADR-0004: a bare pin "1.2.3" means exact — 1.2.4 violates it.
        let installed = vec![skill("a", "1.2.4")];
        let mut project = HashMap::new();
        project.insert("a".to_string(), Some("1.2.3".to_string()));
        let lock = HashMap::new();

        let report =
            build_reconciliation_report(&installed, &project, &lock, Path::new(".")).unwrap();

        assert!(matches!(
            status_of(&report, "a"),
            ReconciliationStatus::Mismatch
        ));
    }

    #[test]
    fn test_no_constraint_lock_equal_is_ok() {
        let installed = vec![skill("a", "1.0.0")];
        let mut project = HashMap::new();
        project.insert("a".to_string(), None);
        let mut lock = HashMap::new();
        lock.insert("a".to_string(), "1.0.0".to_string());

        let report =
            build_reconciliation_report(&installed, &project, &lock, Path::new(".")).unwrap();

        assert!(matches!(status_of(&report, "a"), ReconciliationStatus::Ok));
    }

    #[test]
    fn test_no_constraint_lock_differs_is_mismatch() {
        // Existing lock-equality behavior is preserved when no constraint is present.
        let installed = vec![skill("a", "1.0.1")];
        let mut project = HashMap::new();
        project.insert("a".to_string(), None);
        let mut lock = HashMap::new();
        lock.insert("a".to_string(), "1.0.0".to_string());

        let report =
            build_reconciliation_report(&installed, &project, &lock, Path::new(".")).unwrap();

        assert!(matches!(
            status_of(&report, "a"),
            ReconciliationStatus::Mismatch
        ));
        assert_eq!(report.version_mismatches[0].locked_version, "1.0.0");
    }

    #[test]
    fn test_extraneous_when_not_in_project() {
        let installed = vec![skill("a", "1.0.0")];
        let project = HashMap::new(); // empty → a is extraneous
        let lock = HashMap::new();

        let report =
            build_reconciliation_report(&installed, &project, &lock, Path::new(".")).unwrap();

        assert!(matches!(
            status_of(&report, "a"),
            ReconciliationStatus::Extraneous
        ));
        assert_eq!(report.extraneous.len(), 1);
    }

    #[test]
    fn test_missing_when_in_project_not_installed() {
        let installed: Vec<SkillDefinition> = vec![];
        let mut project = HashMap::new();
        project.insert("a".to_string(), Some("1.0.0".to_string()));
        let lock = HashMap::new();

        let report =
            build_reconciliation_report(&installed, &project, &lock, Path::new(".")).unwrap();

        assert_eq!(report.missing.len(), 1);
        assert_eq!(report.missing[0].id, "a");
    }

    #[test]
    fn test_unparseable_constraint_does_not_flag_mismatch() {
        // A garbage constraint must not spuriously mark the skill as mismatched;
        // fall through to the lock check (here Ok since lock matches).
        let installed = vec![skill("a", "1.0.0")];
        let mut project = HashMap::new();
        project.insert("a".to_string(), Some("not-a-constraint".to_string()));
        let mut lock = HashMap::new();
        lock.insert("a".to_string(), "1.0.0".to_string());

        let report =
            build_reconciliation_report(&installed, &project, &lock, Path::new(".")).unwrap();

        assert!(matches!(status_of(&report, "a"), ReconciliationStatus::Ok));
    }
}
