//! Package resolver for unified skill resolution across multiple sources

use crate::core::dependencies::Dependency;
use crate::core::sources::{SourceConfig, SourcesManager};
use crate::core::version::{VersionConstraint, VersionError};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use thiserror::Error;

/// Skill candidate from a source
#[derive(Debug, Clone)]
pub struct SkillCandidate {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub source_name: String,
    pub source_config: SourceConfig,
    pub download_url: Option<String>,
    pub commit_hash: Option<String>,
}

/// Conflict resolution strategy
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictStrategy {
    /// Use highest priority source
    Priority,
    /// Use highest version
    HighestVersion,
    /// Require explicit source specification
    Explicit,
}

/// Resolution result
#[derive(Debug, Clone)]
pub struct ResolutionResult {
    pub candidate: SkillCandidate,
    pub source_name: String,
}

/// Package resolver errors
#[derive(Debug, Error)]
pub enum ResolverError {
    #[error("Skill not found: {0}")]
    NotFound(String),

    #[error("Version constraint not satisfied: {0}")]
    ConstraintNotSatisfied(String),

    #[error("Multiple candidates found, source specification required")]
    MultipleCandidates,

    #[error("Version error: {0}")]
    VersionError(#[from] VersionError),

    #[error("Source error: {0}")]
    SourceError(String),
}

/// Package resolver for unified skill resolution
pub struct PackageResolver {
    sources_manager: Arc<SourcesManager>,
    skill_index: HashMap<String, Vec<SkillCandidate>>,
}

impl PackageResolver {
    /// Create a new package resolver
    pub fn new(sources_manager: Arc<SourcesManager>) -> Self {
        Self {
            sources_manager,
            skill_index: HashMap::new(),
        }
    }

    /// Build the unified skill index from all sources
    pub async fn build_index(&mut self) -> Result<(), ResolverError> {
        self.skill_index.clear();

        // Get all skills from all sources (already sorted by priority)
        let all_skills = self
            .sources_manager
            .get_available_skills()
            .await
            .map_err(|e| ResolverError::SourceError(e.to_string()))?;

        // Group skills by ID
        for skill_info in all_skills {
            // Get source config for this skill
            let source_def = self
                .sources_manager
                .get_source(&skill_info.source_name)
                .ok_or_else(|| {
                    ResolverError::SourceError(format!(
                        "Source '{}' not found",
                        skill_info.source_name
                    ))
                })?;

            let candidate = SkillCandidate {
                id: skill_info.id.clone(),
                name: skill_info.name.clone(),
                version: skill_info.version.unwrap_or_else(|| "1.0.0".to_string()),
                description: skill_info.description.clone(),
                source_name: skill_info.source_name.clone(),
                source_config: source_def.source.clone(),
                download_url: None,
                commit_hash: None,
            };

            self.skill_index
                .entry(skill_info.id)
                .or_default()
                .push(candidate);
        }

        // Sort candidates by source priority (lower priority number = higher priority)
        for candidates in self.skill_index.values_mut() {
            candidates.sort_by(|a, b| {
                let a_priority = self
                    .sources_manager
                    .get_source(&a.source_name)
                    .map(|s| s.priority)
                    .unwrap_or(u32::MAX);
                let b_priority = self
                    .sources_manager
                    .get_source(&b.source_name)
                    .map(|s| s.priority)
                    .unwrap_or(u32::MAX);
                a_priority.cmp(&b_priority)
            });
        }

        Ok(())
    }

    /// Resolve a skill by ID
    pub fn resolve_skill(
        &self,
        skill_id: &str,
        version_constraint: Option<&VersionConstraint>,
        source_name: Option<&str>,
        strategy: ConflictStrategy,
    ) -> Result<ResolutionResult, ResolverError> {
        let candidates = self
            .skill_index
            .get(skill_id)
            .ok_or_else(|| ResolverError::NotFound(skill_id.to_string()))?;

        // Filter by source if specified
        let filtered_candidates: Vec<&SkillCandidate> = if let Some(source) = source_name {
            candidates
                .iter()
                .filter(|c| c.source_name == source)
                .collect()
        } else {
            candidates.iter().collect()
        };

        if filtered_candidates.is_empty() {
            return Err(ResolverError::NotFound(skill_id.to_string()));
        }

        // Filter by version constraint if specified
        let constraint_filtered: Vec<&SkillCandidate> = if let Some(constraint) = version_constraint
        {
            filtered_candidates
                .iter()
                .filter(|c| constraint.satisfies(&c.version).unwrap_or(false))
                .copied()
                .collect()
        } else {
            filtered_candidates
        };

        if constraint_filtered.is_empty() {
            return Err(ResolverError::ConstraintNotSatisfied(format!(
                "No version satisfies constraint for skill '{}'",
                skill_id
            )));
        }

        // Resolve conflict if multiple candidates
        let selected = if constraint_filtered.len() == 1 {
            constraint_filtered[0]
        } else {
            match strategy {
                ConflictStrategy::Priority => {
                    // Already sorted by priority, take first
                    constraint_filtered[0]
                }
                ConflictStrategy::HighestVersion => {
                    // Find highest version
                    constraint_filtered
                        .iter()
                        .max_by(|a, b| {
                            crate::core::version::compare_versions(&a.version, &b.version)
                                .unwrap_or(std::cmp::Ordering::Equal)
                        })
                        .ok_or_else(|| {
                            ResolverError::ConstraintNotSatisfied(
                                "No candidates available after filtering".to_string(),
                            )
                        })?
                }
                ConflictStrategy::Explicit => {
                    return Err(ResolverError::MultipleCandidates);
                }
            }
        };

        Ok(ResolutionResult {
            candidate: selected.clone(),
            source_name: selected.source_name.clone(),
        })
    }

    /// Get all available versions of a skill
    pub fn get_available_versions(&self, skill_id: &str) -> Vec<&SkillCandidate> {
        self.skill_index
            .get(skill_id)
            .map(|candidates| candidates.iter().collect())
            .unwrap_or_default()
    }

    /// Check if a skill exists in any source
    pub fn skill_exists(&self, skill_id: &str) -> bool {
        self.skill_index.contains_key(skill_id)
    }

    /// Get all skill IDs available
    pub fn list_skills(&self) -> Vec<String> {
        self.skill_index.keys().cloned().collect()
    }

    /// Resolve dependencies for a skill
    ///
    /// Note: This method resolves dependencies across sources (finding which source
    /// provides each dependency). For graph structure management and topological sorting,
    /// see `DependencyGraph` in the `dependencies` module. The two serve different purposes:
    /// - PackageResolver: Source resolution (which source has the skill/version)
    /// - DependencyGraph: Graph structure and installation order
    pub fn resolve_dependencies(
        &self,
        skill_id: &str,
        dependencies: &[Dependency],
        strategy: ConflictStrategy,
    ) -> Result<Vec<ResolutionResult>, ResolverError> {
        let mut resolved = Vec::new();
        let mut visited = HashSet::new();
        let mut resolution_map = HashMap::new();

        self.resolve_dependencies_recursive(
            skill_id,
            dependencies,
            &mut resolved,
            &mut visited,
            &mut resolution_map,
            strategy,
        )?;

        Ok(resolved)
    }

    /// Recursively resolve dependencies
    fn resolve_dependencies_recursive(
        &self,
        _skill_id: &str,
        dependencies: &[Dependency],
        resolved: &mut Vec<ResolutionResult>,
        visited: &mut HashSet<String>,
        resolution_map: &mut HashMap<String, ResolutionResult>,
        strategy: ConflictStrategy,
    ) -> Result<(), ResolverError> {
        for dep in dependencies {
            if visited.contains(&dep.skill_id) {
                continue;
            }

            // Resolve the dependency
            let constraint = dep.version_constraint.as_ref();
            let resolution = self.resolve_skill(&dep.skill_id, constraint, None, strategy)?;

            // Check for version conflicts
            if let Some(existing) = resolution_map.get(&dep.skill_id) {
                if existing.candidate.version != resolution.candidate.version {
                    return Err(ResolverError::ConstraintNotSatisfied(format!(
                        "Version conflict for '{}': {} vs {}",
                        dep.skill_id, existing.candidate.version, resolution.candidate.version
                    )));
                }
            } else {
                resolution_map.insert(dep.skill_id.clone(), resolution.clone());
            }

            visited.insert(dep.skill_id.clone());
            resolved.push(resolution);
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::sources::{SourceConfig, SourcesManager};
    use std::path::PathBuf;
    use tempfile::NamedTempFile;

    #[tokio::test]
    async fn test_resolver_priority() {
        let temp_file = NamedTempFile::new().unwrap();
        let config_path = temp_file.path().to_path_buf();

        let mut sources_manager = SourcesManager::new(config_path);
        sources_manager.load().unwrap();

        // Add sources with different priorities
        sources_manager
            .add_source_with_priority(
                "high-priority".to_string(),
                SourceConfig::Local {
                    path: PathBuf::from("./test"),
                },
                1,
            )
            .unwrap();

        sources_manager
            .add_source_with_priority(
                "low-priority".to_string(),
                SourceConfig::Local {
                    path: PathBuf::from("./test2"),
                },
                2,
            )
            .unwrap();

        let _resolver = PackageResolver::new(Arc::new(sources_manager));
        // Note: build_index would need actual skills to test fully
        // This is a basic structure test
    }
}
