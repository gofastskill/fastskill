//! Update service for managing skill updates

use crate::core::lock::{LockedSkillEntry, SkillsLock};
use crate::core::resolver::{ConflictStrategy, PackageResolver, ResolutionResult};
use crate::core::version::{compare_versions, is_newer, VersionError};
use semver::Version;
use std::sync::Arc;
use thiserror::Error;

/// Update strategy
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateStrategy {
    /// Update to latest available version
    Latest,
    /// Update only patch versions (1.2.3 -> 1.2.4)
    Patch,
    /// Update minor and patch (1.2.3 -> 1.3.0)
    Minor,
    /// Update to latest major version (1.2.3 -> 2.0.0)
    Major,
    /// Update to specific version only
    Exact(String),
}

/// Update information
#[derive(Debug, Clone)]
pub struct UpdateInfo {
    pub skill_id: String,
    pub current_version: String,
    pub available_version: String,
    pub resolution: ResolutionResult,
}

/// Update service errors
#[derive(Debug, Error)]
pub enum UpdateError {
    #[error("Version error: {0}")]
    VersionError(#[from] VersionError),

    #[error("Resolver error: {0}")]
    ResolverError(String),

    #[error("No update available for skill: {0}")]
    NoUpdateAvailable(String),

    #[error("Update strategy not applicable: {0}")]
    StrategyNotApplicable(String),
}

/// Update service for checking and applying updates
pub struct UpdateService {
    resolver: Arc<PackageResolver>,
    lock: SkillsLock,
}

impl UpdateService {
    /// Create a new update service
    pub fn new(resolver: Arc<PackageResolver>, lock: SkillsLock) -> Self {
        Self { resolver, lock }
    }

    /// Check for available updates
    pub fn check_updates(
        &self,
        skill_id: Option<&str>,
        strategy: UpdateStrategy,
    ) -> Result<Vec<UpdateInfo>, UpdateError> {
        let skills_to_check: Vec<&LockedSkillEntry> = if let Some(id) = skill_id {
            self.lock.skills.iter().filter(|s| s.id == id).collect()
        } else {
            self.lock.skills.iter().collect()
        };

        let mut updates = Vec::new();

        for locked_skill in skills_to_check {
            if let Ok(update_info) = self.check_skill_update(locked_skill, &strategy) {
                updates.push(update_info);
            }
        }

        Ok(updates)
    }

    /// Check if a specific skill has an update available
    fn check_skill_update(
        &self,
        locked_skill: &LockedSkillEntry,
        strategy: &UpdateStrategy,
    ) -> Result<UpdateInfo, UpdateError> {
        // Get available versions
        let candidates = self.resolver.get_available_versions(&locked_skill.id);

        if candidates.is_empty() {
            return Err(UpdateError::NoUpdateAvailable(locked_skill.id.clone()));
        }

        // Find the best candidate based on strategy
        let target_version = match strategy {
            UpdateStrategy::Latest => {
                // Find highest version
                candidates
                    .iter()
                    .max_by(|a, b| {
                        compare_versions(&a.version, &b.version)
                            .unwrap_or(std::cmp::Ordering::Equal)
                    })
                    .ok_or_else(|| UpdateError::NoUpdateAvailable(locked_skill.id.clone()))?
            }
            UpdateStrategy::Patch => {
                // Find highest patch version in same minor
                let current = Version::parse(&locked_skill.version).map_err(|e| {
                    UpdateError::VersionError(VersionError::ParseError(e.to_string()))
                })?;

                candidates
                    .iter()
                    .filter(|c| {
                        if let Ok(candidate_ver) = Version::parse(&c.version) {
                            candidate_ver.major == current.major
                                && candidate_ver.minor == current.minor
                                && candidate_ver > current
                        } else {
                            false
                        }
                    })
                    .max_by(|a, b| {
                        compare_versions(&a.version, &b.version)
                            .unwrap_or(std::cmp::Ordering::Equal)
                    })
                    .ok_or_else(|| {
                        UpdateError::StrategyNotApplicable(format!(
                            "No patch update available for {}",
                            locked_skill.id
                        ))
                    })?
            }
            UpdateStrategy::Minor => {
                // Find highest version in same major
                let current = Version::parse(&locked_skill.version).map_err(|e| {
                    UpdateError::VersionError(VersionError::ParseError(e.to_string()))
                })?;

                candidates
                    .iter()
                    .filter(|c| {
                        if let Ok(candidate_ver) = Version::parse(&c.version) {
                            candidate_ver.major == current.major && candidate_ver > current
                        } else {
                            false
                        }
                    })
                    .max_by(|a, b| {
                        compare_versions(&a.version, &b.version)
                            .unwrap_or(std::cmp::Ordering::Equal)
                    })
                    .ok_or_else(|| {
                        UpdateError::StrategyNotApplicable(format!(
                            "No minor update available for {}",
                            locked_skill.id
                        ))
                    })?
            }
            UpdateStrategy::Major => {
                // Find highest version overall
                candidates
                    .iter()
                    .max_by(|a, b| {
                        compare_versions(&a.version, &b.version)
                            .unwrap_or(std::cmp::Ordering::Equal)
                    })
                    .ok_or_else(|| UpdateError::NoUpdateAvailable(locked_skill.id.clone()))?
            }
            UpdateStrategy::Exact(version) => candidates
                .iter()
                .find(|c| c.version == *version)
                .ok_or_else(|| {
                    UpdateError::StrategyNotApplicable(format!(
                        "Exact version {} not available for {}",
                        version, locked_skill.id
                    ))
                })?,
        };

        // Check if update is actually newer
        if !is_newer(&target_version.version, &locked_skill.version)? {
            return Err(UpdateError::NoUpdateAvailable(locked_skill.id.clone()));
        }

        // Resolve the skill to get full resolution info
        let resolution = self
            .resolver
            .resolve_skill(
                &locked_skill.id,
                None,
                Some(&target_version.source_name),
                ConflictStrategy::Priority,
            )
            .map_err(|e| UpdateError::ResolverError(e.to_string()))?;

        Ok(UpdateInfo {
            skill_id: locked_skill.id.clone(),
            current_version: locked_skill.version.clone(),
            available_version: target_version.version.clone(),
            resolution,
        })
    }

    /// Resolve updates for multiple skills
    pub fn resolve_updates(
        &self,
        skill_ids: &[String],
        strategy: UpdateStrategy,
    ) -> Result<Vec<UpdateInfo>, UpdateError> {
        let mut updates = Vec::new();

        for skill_id in skill_ids {
            if let Some(locked_skill) = self.lock.skills.iter().find(|s| s.id == *skill_id) {
                if let Ok(update_info) = self.check_skill_update(locked_skill, &strategy) {
                    updates.push(update_info);
                }
            }
        }

        Ok(updates)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn test_update_strategy_parsing() {
        // Test that strategies can be created
        let _latest = UpdateStrategy::Latest;
        let _patch = UpdateStrategy::Patch;
        let _minor = UpdateStrategy::Minor;
        let _major = UpdateStrategy::Major;
        let _exact = UpdateStrategy::Exact("1.2.3".to_string());
    }
}
