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

        self.resolve_dependency_list(
            skill_id,
            dependencies,
            &mut resolved,
            &mut visited,
            &mut resolution_map,
            strategy,
        )?;

        Ok(resolved)
    }

    /// Resolve a single (non-transitive) list of dependencies across sources.
    ///
    /// Renamed from `resolve_dependencies_recursive` (PARTIAL-8): this method
    /// resolves only the directly supplied `dependencies` — it does not fetch
    /// resolved skills' transitive dependencies. Real transitive traversal lives
    /// in `DependencyResolver` (see `dependency_resolver.rs`).
    fn resolve_dependency_list(
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
#[allow(clippy::unwrap_used)]
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

    // ---- Helpers -------------------------------------------------------------

    fn empty_manager() -> Arc<SourcesManager> {
        let temp_file = NamedTempFile::new().unwrap();
        let mut sm = SourcesManager::new(temp_file.path().to_path_buf());
        sm.load().unwrap();
        Arc::new(sm)
    }

    fn candidate(id: &str, version: &str, source: &str) -> SkillCandidate {
        SkillCandidate {
            id: id.to_string(),
            name: id.to_string(),
            version: version.to_string(),
            description: "desc".to_string(),
            source_name: source.to_string(),
            source_config: SourceConfig::Local {
                path: PathBuf::from("./x"),
            },
            download_url: None,
            commit_hash: None,
        }
    }

    /// Build a resolver whose index is populated directly (bypassing sources).
    fn resolver_with(entries: Vec<(&str, Vec<SkillCandidate>)>) -> PackageResolver {
        let mut r = PackageResolver::new(empty_manager());
        for (id, cands) in entries {
            r.skill_index.insert(id.to_string(), cands);
        }
        r
    }

    // ---- resolve_skill -------------------------------------------------------

    #[test]
    fn test_resolve_skill_not_found() {
        let r = resolver_with(vec![]);
        let err = r
            .resolve_skill("missing", None, None, ConflictStrategy::Priority)
            .unwrap_err();
        assert!(matches!(err, ResolverError::NotFound(id) if id == "missing"));
    }

    #[test]
    fn test_resolve_skill_single_candidate() {
        let r = resolver_with(vec![("a", vec![candidate("a", "1.0.0", "src1")])]);
        let res = r
            .resolve_skill("a", None, None, ConflictStrategy::Priority)
            .unwrap();
        assert_eq!(res.candidate.version, "1.0.0");
        assert_eq!(res.source_name, "src1");
    }

    #[test]
    fn test_resolve_skill_source_filter_matches() {
        let r = resolver_with(vec![(
            "a",
            vec![
                candidate("a", "1.0.0", "src1"),
                candidate("a", "2.0.0", "src2"),
            ],
        )]);
        let res = r
            .resolve_skill("a", None, Some("src2"), ConflictStrategy::Priority)
            .unwrap();
        assert_eq!(res.source_name, "src2");
        assert_eq!(res.candidate.version, "2.0.0");
    }

    #[test]
    fn test_resolve_skill_source_filter_empty_is_not_found() {
        let r = resolver_with(vec![("a", vec![candidate("a", "1.0.0", "src1")])]);
        let err = r
            .resolve_skill("a", None, Some("nope"), ConflictStrategy::Priority)
            .unwrap_err();
        assert!(matches!(err, ResolverError::NotFound(_)));
    }

    #[test]
    fn test_resolve_skill_version_constraint_satisfied() {
        let r = resolver_with(vec![(
            "a",
            vec![
                candidate("a", "1.0.0", "src1"),
                candidate("a", "2.0.0", "src2"),
            ],
        )]);
        let constraint = VersionConstraint::parse("2.0.0").unwrap();
        let res = r
            .resolve_skill("a", Some(&constraint), None, ConflictStrategy::Priority)
            .unwrap();
        assert_eq!(res.candidate.version, "2.0.0");
    }

    #[test]
    fn test_resolve_skill_constraint_not_satisfied() {
        let r = resolver_with(vec![("a", vec![candidate("a", "1.0.0", "src1")])]);
        let constraint = VersionConstraint::parse(">=2.0.0").unwrap();
        let err = r
            .resolve_skill("a", Some(&constraint), None, ConflictStrategy::Priority)
            .unwrap_err();
        assert!(matches!(err, ResolverError::ConstraintNotSatisfied(_)));
    }

    #[test]
    fn test_resolve_skill_priority_takes_first() {
        // Index order acts as priority order (build_index sorts ascending).
        let r = resolver_with(vec![(
            "a",
            vec![
                candidate("a", "1.0.0", "high"),
                candidate("a", "9.9.9", "low"),
            ],
        )]);
        let res = r
            .resolve_skill("a", None, None, ConflictStrategy::Priority)
            .unwrap();
        assert_eq!(res.source_name, "high");
    }

    #[test]
    fn test_resolve_skill_highest_version() {
        let r = resolver_with(vec![(
            "a",
            vec![
                candidate("a", "1.0.0", "s1"),
                candidate("a", "3.2.1", "s2"),
                candidate("a", "2.0.0", "s3"),
            ],
        )]);
        let res = r
            .resolve_skill("a", None, None, ConflictStrategy::HighestVersion)
            .unwrap();
        assert_eq!(res.candidate.version, "3.2.1");
    }

    #[test]
    fn test_resolve_skill_explicit_multiple_errors() {
        let r = resolver_with(vec![(
            "a",
            vec![candidate("a", "1.0.0", "s1"), candidate("a", "2.0.0", "s2")],
        )]);
        let err = r
            .resolve_skill("a", None, None, ConflictStrategy::Explicit)
            .unwrap_err();
        assert!(matches!(err, ResolverError::MultipleCandidates));
    }

    // ---- introspection helpers ----------------------------------------------

    #[test]
    fn test_get_available_versions() {
        let r = resolver_with(vec![(
            "a",
            vec![candidate("a", "1.0.0", "s1"), candidate("a", "2.0.0", "s2")],
        )]);
        assert_eq!(r.get_available_versions("a").len(), 2);
        assert!(r.get_available_versions("missing").is_empty());
    }

    #[test]
    fn test_skill_exists_and_list_skills() {
        let r = resolver_with(vec![("a", vec![candidate("a", "1.0.0", "s1")])]);
        assert!(r.skill_exists("a"));
        assert!(!r.skill_exists("b"));
        assert_eq!(r.list_skills(), vec!["a".to_string()]);
    }

    // ---- resolve_dependencies -----------------------------------------------

    #[test]
    fn test_resolve_dependencies_simple() {
        let r = resolver_with(vec![
            ("a", vec![candidate("a", "1.0.0", "s1")]),
            ("b", vec![candidate("b", "1.0.0", "s1")]),
        ]);
        let deps = vec![
            Dependency {
                skill_id: "a".to_string(),
                version_constraint: None,
            },
            Dependency {
                skill_id: "b".to_string(),
                version_constraint: None,
            },
        ];
        let resolved = r
            .resolve_dependencies("root", &deps, ConflictStrategy::Priority)
            .unwrap();
        assert_eq!(resolved.len(), 2);
    }

    #[test]
    fn test_resolve_dependencies_skips_visited_duplicates() {
        let r = resolver_with(vec![("a", vec![candidate("a", "1.0.0", "s1")])]);
        let deps = vec![
            Dependency {
                skill_id: "a".to_string(),
                version_constraint: None,
            },
            Dependency {
                skill_id: "a".to_string(),
                version_constraint: None,
            },
        ];
        let resolved = r
            .resolve_dependencies("root", &deps, ConflictStrategy::Priority)
            .unwrap();
        // Second occurrence is skipped via the visited set.
        assert_eq!(resolved.len(), 1);
    }

    #[test]
    fn test_resolve_dependencies_missing_dep_errors() {
        let r = resolver_with(vec![("a", vec![candidate("a", "1.0.0", "s1")])]);
        let deps = vec![Dependency {
            skill_id: "ghost".to_string(),
            version_constraint: None,
        }];
        let err = r
            .resolve_dependencies("root", &deps, ConflictStrategy::Priority)
            .unwrap_err();
        assert!(matches!(err, ResolverError::NotFound(_)));
    }

    #[test]
    fn test_resolve_dependency_list_version_conflict() {
        // Two candidates for "a" from different sources with different versions;
        // Explicit strategy would error on the first resolve, so use HighestVersion
        // and manually seed the resolution_map to force the conflict branch.
        let r = resolver_with(vec![("a", vec![candidate("a", "2.0.0", "s1")])]);
        let mut resolved = Vec::new();
        let mut visited = HashSet::new();
        let mut resolution_map = HashMap::new();
        // Pre-seed a different resolved version for "a".
        resolution_map.insert(
            "a".to_string(),
            ResolutionResult {
                candidate: candidate("a", "1.0.0", "s0"),
                source_name: "s0".to_string(),
            },
        );
        let deps = vec![Dependency {
            skill_id: "a".to_string(),
            version_constraint: None,
        }];
        let err = r
            .resolve_dependency_list(
                "root",
                &deps,
                &mut resolved,
                &mut visited,
                &mut resolution_map,
                ConflictStrategy::Priority,
            )
            .unwrap_err();
        assert!(matches!(err, ResolverError::ConstraintNotSatisfied(_)));
    }

    // ---- build_index (via a real Local source) ------------------------------

    fn write_skill(dir: &std::path::Path, id: &str, version: &str) {
        let skill_dir = dir.join(id);
        std::fs::create_dir_all(&skill_dir).unwrap();
        let content = format!(
            "---\nname: {id}\ndescription: A test skill\nversion: {version}\n---\n# {id}\n"
        );
        std::fs::write(skill_dir.join("SKILL.md"), content).unwrap();
    }

    #[tokio::test]
    async fn test_build_index_from_local_sources() {
        let src1 = tempfile::TempDir::new().unwrap();
        let src2 = tempfile::TempDir::new().unwrap();
        write_skill(src1.path(), "alpha", "1.0.0");
        write_skill(src1.path(), "beta", "1.0.0");
        // "alpha" also exists in the lower-priority source with a different version.
        write_skill(src2.path(), "alpha", "2.0.0");

        let temp_file = NamedTempFile::new().unwrap();
        let mut sm = SourcesManager::new(temp_file.path().to_path_buf());
        sm.load().unwrap();
        sm.add_source_with_priority(
            "high".to_string(),
            SourceConfig::Local {
                path: src1.path().to_path_buf(),
            },
            1,
        )
        .unwrap();
        sm.add_source_with_priority(
            "low".to_string(),
            SourceConfig::Local {
                path: src2.path().to_path_buf(),
            },
            5,
        )
        .unwrap();

        let mut resolver = PackageResolver::new(Arc::new(sm));
        resolver.build_index().await.unwrap();

        assert!(resolver.skill_exists("alpha"));
        assert!(resolver.skill_exists("beta"));

        // "alpha" has two candidates, sorted so the higher-priority source is first.
        let versions = resolver.get_available_versions("alpha");
        assert_eq!(versions.len(), 2);
        assert_eq!(versions[0].source_name, "high");

        // Resolving with Priority picks the high-priority source's candidate.
        let res = resolver
            .resolve_skill("alpha", None, None, ConflictStrategy::Priority)
            .unwrap();
        assert_eq!(res.source_name, "high");

        // Rebuilding clears and repopulates without duplicating.
        resolver.build_index().await.unwrap();
        assert_eq!(resolver.get_available_versions("alpha").len(), 2);
    }

    // ---- error Display -------------------------------------------------------

    #[test]
    fn test_resolver_error_display() {
        assert!(ResolverError::NotFound("x".into())
            .to_string()
            .contains("Skill not found"));
        assert!(ResolverError::MultipleCandidates
            .to_string()
            .contains("source specification required"));
        assert!(ResolverError::SourceError("boom".into())
            .to_string()
            .contains("boom"));
        let ve: ResolverError = VersionError::InvalidConstraint("bad".into()).into();
        assert!(ve.to_string().contains("Version error"));
    }
}
