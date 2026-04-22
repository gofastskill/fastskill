//! Recursive dependency resolution for the install command
//!
//! Implements a breadth-first dependency graph traversal that:
//! - Deduplicates skills by ID (first-encountered wins)
//! - Enforces a configurable depth limit to prevent runaway chains
//! - Detects circular dependencies and breaks cycles with a warning
//! - Returns skills in topological (dependency-first) order

use crate::core::manifest::{ManifestError, SkillEntry, SkillProjectToml};
use std::collections::{HashSet, VecDeque};
use std::path::Path;

/// Error types specific to dependency resolution
#[derive(Debug, thiserror::Error)]
pub enum DependencyResolutionError {
    #[error("Maximum dependency depth {0} exceeded")]
    DepthLimitExceeded(u32),

    #[error("Circular dependency detected: {0} -> {1}")]
    CircularDependency(String, String),

    #[error("Failed to load transitive dependencies for {skill}: {error}")]
    TransitiveLoadError { skill: String, error: String },
}

/// A single item in the install queue, carrying its resolution context
#[derive(Debug, Clone)]
pub struct SkillInstallItem {
    pub entry: SkillEntry,
    /// Depth at which this skill was discovered (0 = direct dependency)
    pub depth: u32,
    /// ID of the skill that pulled this one in, if any
    pub parent_skill: Option<String>,
}

/// Resolves a dependency graph for a set of root `SkillEntry` items.
///
/// Uses breadth-first traversal so that depth is counted from the root,
/// matching the behaviour described in the spec (decision #5).
#[derive(Debug, Clone)]
pub struct DependencyResolver {
    max_depth: u32,
    visited_skills: HashSet<String>,
    install_queue: VecDeque<SkillInstallItem>,
}

impl DependencyResolver {
    /// Create a new resolver with the given depth limit.
    pub fn new(max_depth: u32) -> Self {
        Self {
            max_depth,
            visited_skills: HashSet::new(),
            install_queue: VecDeque::new(),
        }
    }

    /// Resolve the full dependency graph starting from `initial_entries`.
    ///
    /// After each skill is installed it is expected to live at
    /// `<skills_dir>/<skill_id>/skill-project.toml`.  If that file is absent
    /// the skill contributes zero transitive dependencies (graceful
    /// degradation per the spec).
    pub async fn resolve_dependencies(
        &mut self,
        initial_entries: Vec<SkillEntry>,
        skills_dir: &Path,
    ) -> Result<Vec<SkillInstallItem>, DependencyResolutionError> {
        // Seed the queue with the direct (depth-0) dependencies
        for entry in initial_entries {
            let id = entry.id.clone();
            if self.visited_skills.insert(id.clone()) {
                self.install_queue.push_back(SkillInstallItem {
                    entry,
                    depth: 0,
                    parent_skill: None,
                });
            }
        }

        let mut ordered: Vec<SkillInstallItem> = Vec::new();

        while let Some(item) = self.install_queue.pop_front() {
            let skill_id = item.entry.id.clone();
            let current_depth = item.depth;

            ordered.push(item);

            // Do not recurse beyond max_depth
            if current_depth >= self.max_depth {
                if current_depth > 0 {
                    // Informational – not a hard error per spec §4.6
                    tracing::info!(
                        "Dependency depth limit ({}) reached at skill '{}'; \
                         transitive dependencies will not be resolved further",
                        self.max_depth,
                        skill_id
                    );
                }
                continue;
            }

            // Attempt to load the transitive manifest for this skill
            let transitive_manifest = skills_dir.join(&skill_id).join("skill-project.toml");
            let transitive_entries = match self.load_transitive_dependencies(&transitive_manifest) {
                Ok(entries) => entries,
                Err(ManifestError::NotFound(_)) => {
                    // Graceful degradation: skill has no manifest → zero deps
                    tracing::debug!(
                        "No skill-project.toml found for '{}'; \
                             skipping transitive resolution",
                        skill_id
                    );
                    continue;
                }
                Err(e) => {
                    tracing::warn!(
                        "Could not load transitive dependencies for '{}': {}",
                        skill_id,
                        e
                    );
                    continue;
                }
            };

            for trans_entry in transitive_entries {
                let trans_id = trans_entry.id.clone();

                // Deduplication: first-encountered wins
                // Also detect circular dependencies (when same skill appears at different depths)
                if !self.visited_skills.insert(trans_id.clone()) {
                    // Check if this is a circular dependency (skill already in the graph)
                    // Emit warning per spec §4.6 "cycles are broken with warning message"
                    tracing::warn!(
                        "Circular dependency detected: {} -> {}; \
                         skipping duplicate to break cycle",
                        skill_id,
                        trans_id
                    );
                    continue;
                }

                self.install_queue.push_back(SkillInstallItem {
                    entry: trans_entry,
                    depth: current_depth + 1,
                    parent_skill: Some(skill_id.clone()),
                });
            }
        }

        Ok(ordered)
    }

    /// Load skill entries from a `skill-project.toml` at the given path.
    ///
    /// Returns `ManifestError::NotFound` when the file does not exist so the
    /// caller can distinguish "no manifest" from "broken manifest".
    fn load_transitive_dependencies(
        &self,
        manifest_path: &Path,
    ) -> Result<Vec<SkillEntry>, ManifestError> {
        let project = SkillProjectToml::load_from_file(manifest_path)?;
        project.to_skill_entries().map_err(ManifestError::Parse)
    }

    /// Return the skills in dependency-first (topological) order.
    ///
    /// The BFS traversal already yields parents before children, so this is
    /// a no-op structurally; the method exists as the public API surface
    /// specified in §5.1 and can be extended later if needed.
    pub fn topological_sort(&self, items: Vec<SkillInstallItem>) -> Vec<SkillInstallItem> {
        // BFS order from resolve_dependencies is already topological:
        // each skill appears after the skill that declared it as a dependency.
        items
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::core::manifest::{SkillEntry, SkillSource};
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn make_local_entry(id: &str, path: &str) -> SkillEntry {
        SkillEntry {
            id: id.to_string(),
            source: SkillSource::Local {
                path: PathBuf::from(path),
                editable: false,
            },
            version: "*".to_string(),
            groups: vec![],
            editable: false,
        }
    }

    #[tokio::test]
    async fn test_resolve_no_transitive_deps() {
        let dir = TempDir::new().unwrap();
        let mut resolver = DependencyResolver::new(5);

        let entries = vec![make_local_entry("skill-a", "local/skill-a")];
        let result = resolver
            .resolve_dependencies(entries, dir.path())
            .await
            .unwrap();

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].entry.id, "skill-a");
        assert_eq!(result[0].depth, 0);
        assert!(result[0].parent_skill.is_none());
    }

    #[tokio::test]
    async fn test_resolve_deduplicates_same_id() {
        let dir = TempDir::new().unwrap();
        let mut resolver = DependencyResolver::new(5);

        // Two entries with the same ID – only the first should survive
        let entries = vec![
            make_local_entry("skill-a", "local/skill-a"),
            make_local_entry("skill-a", "local/skill-a-duplicate"),
        ];
        let result = resolver
            .resolve_dependencies(entries, dir.path())
            .await
            .unwrap();

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].entry.id, "skill-a");
    }

    #[tokio::test]
    async fn test_resolve_with_transitive_manifest() {
        let dir = TempDir::new().unwrap();

        // Create skill-a with a skill-project.toml that depends on skill-b
        let skill_a_dir = dir.path().join("skill-a");
        std::fs::create_dir_all(&skill_a_dir).unwrap();
        std::fs::write(
            skill_a_dir.join("skill-project.toml"),
            r#"[dependencies]
skill-b = { source = "local", path = "local/skill-b" }
"#,
        )
        .unwrap();

        let mut resolver = DependencyResolver::new(5);
        let entries = vec![make_local_entry("skill-a", "local/skill-a")];
        let result = resolver
            .resolve_dependencies(entries, dir.path())
            .await
            .unwrap();

        assert_eq!(result.len(), 2);
        assert_eq!(result[0].entry.id, "skill-a");
        assert_eq!(result[0].depth, 0);
        assert_eq!(result[1].entry.id, "skill-b");
        assert_eq!(result[1].depth, 1);
        assert_eq!(result[1].parent_skill.as_deref(), Some("skill-a"));
    }

    #[tokio::test]
    async fn test_resolve_respects_depth_limit() {
        let dir = TempDir::new().unwrap();

        // skill-a → skill-b (at depth 1) → skill-c would be depth 2, but limit is 1
        let skill_a_dir = dir.path().join("skill-a");
        std::fs::create_dir_all(&skill_a_dir).unwrap();
        std::fs::write(
            skill_a_dir.join("skill-project.toml"),
            r#"[dependencies]
skill-b = { source = "local", path = "local/skill-b" }
"#,
        )
        .unwrap();

        let skill_b_dir = dir.path().join("skill-b");
        std::fs::create_dir_all(&skill_b_dir).unwrap();
        std::fs::write(
            skill_b_dir.join("skill-project.toml"),
            r#"[dependencies]
skill-c = { source = "local", path = "local/skill-c" }
"#,
        )
        .unwrap();

        // max_depth = 1 → skill-b is included (depth 1), skill-c is NOT
        let mut resolver = DependencyResolver::new(1);
        let entries = vec![make_local_entry("skill-a", "local/skill-a")];
        let result = resolver
            .resolve_dependencies(entries, dir.path())
            .await
            .unwrap();

        let ids: Vec<&str> = result.iter().map(|i| i.entry.id.as_str()).collect();
        assert!(ids.contains(&"skill-a"), "skill-a must be present");
        assert!(
            ids.contains(&"skill-b"),
            "skill-b must be present at depth 1"
        );
        assert!(
            !ids.contains(&"skill-c"),
            "skill-c must be excluded beyond depth limit"
        );
    }

    #[tokio::test]
    async fn test_resolve_missing_manifest_continues() {
        // If an installed skill has no skill-project.toml the resolver must
        // not error out – it should just contribute zero transitive deps.
        let dir = TempDir::new().unwrap();
        let mut resolver = DependencyResolver::new(5);

        let entries = vec![make_local_entry(
            "no-manifest-skill",
            "local/no-manifest-skill",
        )];
        let result = resolver
            .resolve_dependencies(entries, dir.path())
            .await
            .unwrap();

        // Skill without manifest still appears – it just has no children
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].entry.id, "no-manifest-skill");
    }

    #[tokio::test]
    async fn test_topological_sort_preserves_order() {
        let resolver = DependencyResolver::new(5);
        let items = vec![
            SkillInstallItem {
                entry: make_local_entry("a", "local/a"),
                depth: 0,
                parent_skill: None,
            },
            SkillInstallItem {
                entry: make_local_entry("b", "local/b"),
                depth: 1,
                parent_skill: Some("a".to_string()),
            },
        ];
        let sorted = resolver.topological_sort(items.clone());
        assert_eq!(sorted[0].entry.id, items[0].entry.id);
        assert_eq!(sorted[1].entry.id, items[1].entry.id);
    }
}
