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

use crate::core::skill_manager::SkillDefinition;
use std::collections::HashMap;
use std::path::Path;

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

        // Determine status
        let status = if !is_in_project {
            ReconciliationStatus::Extraneous
        } else if let Some(locked_ver) = locked_version {
            if skill.version != *locked_ver {
                ReconciliationStatus::Mismatch
            } else {
                ReconciliationStatus::Ok
            }
        } else {
            ReconciliationStatus::Ok
        };

        // Collect extraneous and mismatches
        match &status {
            ReconciliationStatus::Extraneous => {
                extraneous.push(InstalledSkillInfo {
                    id: skill_id.clone(),
                    name: skill.name.clone(),
                    version: skill.version.clone(),
                    description: skill.description.clone(),
                    source: skill.source_url.clone(),
                    installed_path: skill.skill_file.clone(),
                    installed_at: Some(skill.updated_at),
                    status: status.clone(),
                });
            }
            ReconciliationStatus::Mismatch => {
                if let Some(locked_ver) = locked_version {
                    version_mismatches.push(VersionMismatch {
                        id: skill_id.clone(),
                        installed_version: skill.version.clone(),
                        locked_version: locked_ver.clone(),
                    });
                }
            }
            _ => {}
        }

        // Add to installed list
        installed_info.push(InstalledSkillInfo {
            id: skill_id,
            name: skill.name.clone(),
            version: skill.version.clone(),
            description: skill.description.clone(),
            source: skill.source_url.clone(),
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
