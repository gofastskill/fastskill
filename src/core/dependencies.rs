//! Dependency resolution and graph management

use crate::core::version::{VersionConstraint, VersionError};
use std::collections::{HashMap, HashSet, VecDeque};
use thiserror::Error;

/// A skill dependency with optional version constraint
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dependency {
    pub skill_id: String,
    pub version_constraint: Option<VersionConstraint>,
}

impl Dependency {
    /// Parse dependency from string format (e.g., "skill-id" or "skill-id@^1.2.3")
    pub fn parse(dep_str: &str) -> Result<Self, DependencyError> {
        if let Some(at_pos) = dep_str.find('@') {
            let skill_id = dep_str[..at_pos].trim().to_string();
            let constraint_str = dep_str[at_pos + 1..].trim();

            if constraint_str.is_empty() {
                return Err(DependencyError::InvalidFormat(
                    "Empty version constraint after @".to_string(),
                ));
            }

            let constraint =
                VersionConstraint::parse(constraint_str).map_err(DependencyError::VersionError)?;

            Ok(Dependency {
                skill_id,
                version_constraint: Some(constraint),
            })
        } else {
            Ok(Dependency {
                skill_id: dep_str.trim().to_string(),
                version_constraint: None,
            })
        }
    }

    /// Parse multiple dependencies from a list of strings
    pub fn parse_multiple(dep_strings: &[String]) -> Result<Vec<Self>, DependencyError> {
        dep_strings.iter().map(|s| Self::parse(s)).collect()
    }
}

/// Dependency graph errors
#[derive(Debug, Error)]
pub enum DependencyError {
    #[error("Circular dependency detected: {0}")]
    CircularDependency(String),

    #[error("Dependency not found: {0}")]
    NotFound(String),

    #[error("Version error: {0}")]
    VersionError(#[from] VersionError),

    #[error("Invalid dependency format: {0}")]
    InvalidFormat(String),
}

/// Dependency graph for managing skill dependencies
pub struct DependencyGraph {
    /// Adjacency list: skill_id -> list of dependencies
    graph: HashMap<String, Vec<Dependency>>,
    /// Reverse graph for finding dependents
    reverse_graph: HashMap<String, Vec<String>>,
}

impl DependencyGraph {
    /// Create a new empty dependency graph
    pub fn new() -> Self {
        Self {
            graph: HashMap::new(),
            reverse_graph: HashMap::new(),
        }
    }

    /// Add a skill with its dependencies
    pub fn add_skill(&mut self, skill_id: String, dependencies: Vec<Dependency>) {
        // Update forward graph
        self.graph.insert(skill_id.clone(), dependencies.clone());

        // Update reverse graph
        for dep in dependencies {
            self.reverse_graph
                .entry(dep.skill_id.clone())
                .or_default()
                .push(skill_id.clone());
        }
    }

    /// Build graph from a list of (skill_id, dependencies) pairs
    pub fn build_graph(skills: Vec<(String, Vec<Dependency>)>) -> Self {
        let mut graph = Self::new();
        for (skill_id, deps) in skills {
            graph.add_skill(skill_id, deps);
        }
        graph
    }

    /// Detect circular dependencies
    pub fn detect_cycles(&self) -> Result<(), DependencyError> {
        let mut visited = HashSet::new();
        let mut rec_stack = HashSet::new();

        for skill_id in self.graph.keys() {
            if !visited.contains(skill_id)
                && self.dfs_cycle_detect(skill_id, &mut visited, &mut rec_stack)
            {
                return Err(DependencyError::CircularDependency(
                    "Circular dependency detected in graph".to_string(),
                ));
            }
        }

        Ok(())
    }

    /// DFS for cycle detection (public interface)
    fn dfs_cycle_detect(
        &self,
        skill_id: &str,
        visited: &mut HashSet<String>,
        rec_stack: &mut HashSet<String>,
    ) -> bool {
        self.dfs_cycle_detect_internal(skill_id, visited, rec_stack)
    }

    /// Internal DFS for cycle detection
    fn dfs_cycle_detect_internal(
        &self,
        skill_id: &str,
        visited: &mut HashSet<String>,
        rec_stack: &mut HashSet<String>,
    ) -> bool {
        if rec_stack.contains(skill_id) {
            return true; // Found a cycle
        }

        if visited.contains(skill_id) {
            return false; // Already processed this node
        }

        visited.insert(skill_id.to_string());
        rec_stack.insert(skill_id.to_string());

        if let Some(deps) = self.graph.get(skill_id) {
            for dep in deps {
                // Only check dependencies that are in the graph
                if !self.graph.contains_key(&dep.skill_id) {
                    continue;
                }

                if self.dfs_cycle_detect_internal(&dep.skill_id, visited, rec_stack) {
                    return true;
                }
            }
        }

        rec_stack.remove(skill_id);
        false
    }

    /// Topological sort for installation order
    /// Returns skills in dependency-first order (dependencies before dependents)
    pub fn topological_sort(&self) -> Result<Vec<String>, DependencyError> {
        // Check for cycles first using the existing method
        self.detect_cycles()?;

        // Calculate in-degree: number of dependencies each skill has (that are in the graph)
        // For installation order, we want to install skills with no dependencies first
        let mut in_degree: HashMap<String, usize> = HashMap::new();

        // Initialize in-degree for all skills in the graph
        for skill_id in self.graph.keys() {
            // Count how many dependencies this skill has that are also in the graph
            let dep_count = self
                .graph
                .get(skill_id)
                .map(|deps| {
                    deps.iter()
                        .filter(|dep| self.graph.contains_key(&dep.skill_id))
                        .count()
                })
                .unwrap_or(0);
            in_degree.insert(skill_id.clone(), dep_count);
        }

        // Find all skills with no dependencies (in-degree 0)
        let mut queue: VecDeque<String> = in_degree
            .iter()
            .filter(|(_, &degree)| degree == 0)
            .map(|(skill_id, _)| skill_id.clone())
            .collect();

        let mut result = Vec::new();

        // Process queue using Kahn's algorithm
        while let Some(skill_id) = queue.pop_front() {
            result.push(skill_id.clone());

            // When we process a skill, we can now install skills that depend on it
            // Use reverse graph to find dependents and reduce their in-degree
            if let Some(dependents) = self.reverse_graph.get(&skill_id) {
                for dependent in dependents {
                    // Only process dependents that are in the graph
                    if self.graph.contains_key(dependent) {
                        if let Some(degree) = in_degree.get_mut(dependent) {
                            *degree -= 1;
                            if *degree == 0 {
                                queue.push_back(dependent.clone());
                            }
                        }
                    }
                }
            }
        }

        // Verify all skills were processed
        if result.len() != self.graph.len() {
            // This should not happen if cycle detection worked correctly
            // But handle gracefully for external dependencies
            return Err(DependencyError::CircularDependency(format!(
                "Not all skills could be sorted ({} of {} processed)",
                result.len(),
                self.graph.len()
            )));
        }

        Ok(result)
    }

    /// Get dependencies for a skill
    pub fn get_dependencies(&self, skill_id: &str) -> Option<&Vec<Dependency>> {
        self.graph.get(skill_id)
    }

    /// Get all skills that depend on a given skill
    pub fn get_dependents(&self, skill_id: &str) -> Vec<String> {
        self.reverse_graph
            .get(skill_id)
            .cloned()
            .unwrap_or_default()
    }
}

impl Default for DependencyGraph {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_dependency() {
        let dep = Dependency::parse("my-skill").unwrap();
        assert_eq!(dep.skill_id, "my-skill");
        assert_eq!(dep.version_constraint, None);

        let dep = Dependency::parse("my-skill@^1.2.3").unwrap();
        assert_eq!(dep.skill_id, "my-skill");
        assert!(dep.version_constraint.is_some());
    }

    #[test]
    fn test_cycle_detection() {
        let mut graph = DependencyGraph::new();
        graph.add_skill(
            "a".to_string(),
            vec![Dependency {
                skill_id: "b".to_string(),
                version_constraint: None,
            }],
        );
        graph.add_skill(
            "b".to_string(),
            vec![Dependency {
                skill_id: "a".to_string(),
                version_constraint: None,
            }],
        );

        assert!(graph.detect_cycles().is_err());
    }

    #[test]
    fn test_topological_sort() {
        let mut graph = DependencyGraph::new();
        // a depends on b, b depends on c, c has no dependencies
        // Installation order should be: c, b, a
        graph.add_skill(
            "a".to_string(),
            vec![Dependency {
                skill_id: "b".to_string(),
                version_constraint: None,
            }],
        );
        graph.add_skill(
            "b".to_string(),
            vec![Dependency {
                skill_id: "c".to_string(),
                version_constraint: None,
            }],
        );
        graph.add_skill("c".to_string(), vec![]);

        let order = graph.topological_sort().unwrap();
        // Verify all skills are present
        assert_eq!(order.len(), 3);
        assert!(order.contains(&"a".to_string()));
        assert!(order.contains(&"b".to_string()));
        assert!(order.contains(&"c".to_string()));

        // Verify dependencies: c must come before b, b must come before a
        let c_pos = order.iter().position(|x| x == "c").unwrap();
        let b_pos = order.iter().position(|x| x == "b").unwrap();
        let a_pos = order.iter().position(|x| x == "a").unwrap();

        assert!(
            c_pos < b_pos,
            "c (pos {}) should come before b (pos {})",
            c_pos,
            b_pos
        );
        assert!(
            b_pos < a_pos,
            "b (pos {}) should come before a (pos {})",
            b_pos,
            a_pos
        );
    }

    #[test]
    fn test_topological_sort_branching() {
        let mut graph = DependencyGraph::new();
        // a depends on b and c, b depends on d
        // Installation order: d, b, c, a (or d, c, b, a)
        graph.add_skill(
            "a".to_string(),
            vec![
                Dependency {
                    skill_id: "b".to_string(),
                    version_constraint: None,
                },
                Dependency {
                    skill_id: "c".to_string(),
                    version_constraint: None,
                },
            ],
        );
        graph.add_skill(
            "b".to_string(),
            vec![Dependency {
                skill_id: "d".to_string(),
                version_constraint: None,
            }],
        );
        graph.add_skill("c".to_string(), vec![]);
        graph.add_skill("d".to_string(), vec![]);

        let order = graph.topological_sort().unwrap();
        assert_eq!(order.len(), 4);

        // d must come before b
        let d_pos = order.iter().position(|x| x == "d").unwrap();
        let b_pos = order.iter().position(|x| x == "b").unwrap();
        let a_pos = order.iter().position(|x| x == "a").unwrap();

        assert!(d_pos < b_pos, "d should come before b");
        assert!(b_pos < a_pos, "b should come before a");
        // c can be anywhere before a
        let c_pos = order.iter().position(|x| x == "c").unwrap();
        assert!(c_pos < a_pos, "c should come before a");
    }

    #[test]
    fn test_topological_sort_multiple_roots() {
        let mut graph = DependencyGraph::new();
        // b and c have no dependencies, a depends on b
        // Installation order: b and c first (any order), then a
        graph.add_skill(
            "a".to_string(),
            vec![Dependency {
                skill_id: "b".to_string(),
                version_constraint: None,
            }],
        );
        graph.add_skill("b".to_string(), vec![]);
        graph.add_skill("c".to_string(), vec![]);

        let order = graph.topological_sort().unwrap();
        assert_eq!(order.len(), 3);

        // b must come before a
        let b_pos = order.iter().position(|x| x == "b").unwrap();
        let a_pos = order.iter().position(|x| x == "a").unwrap();
        assert!(b_pos < a_pos, "b should come before a");

        // c can be anywhere
        assert!(order.contains(&"c".to_string()));
    }
}
