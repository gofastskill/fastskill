use super::{DependencySpec, SkillEntry, SkillProjectToml};
use crate::core::origin::Origin;
use crate::core::version::VersionConstraint;
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

impl SkillProjectToml {
    /// Convert local and composed Manifest dependencies to install roots.
    /// Relative origins stay anchored to the Manifest that declares them.
    pub fn to_skill_entries(&self, manifest_dir: &Path) -> Result<Vec<SkillEntry>, String> {
        let root_path = manifest_dir.join("skill-project.toml");
        let root_path = root_path.canonicalize().unwrap_or(root_path);
        let mut stack = vec![root_path.clone()];
        let mut visited = HashSet::from([root_path]);
        let mut entries = BTreeMap::new();
        self.collect_skill_entries(manifest_dir, &mut stack, &mut visited, &mut entries)?;
        Ok(entries.into_values().collect())
    }

    fn collect_skill_entries(
        &self,
        manifest_dir: &Path,
        stack: &mut Vec<PathBuf>,
        visited: &mut HashSet<PathBuf>,
        entries: &mut BTreeMap<String, SkillEntry>,
    ) -> Result<(), String> {
        for entry in self.direct_skill_entries(manifest_dir)? {
            if let Some(existing) = entries.get(&entry.id) {
                let mut existing_groups = existing.groups.clone();
                let mut incoming_groups = entry.groups.clone();
                existing_groups.sort();
                incoming_groups.sort();
                if existing.origin != entry.origin || existing_groups != incoming_groups {
                    return Err(format!(
                        "Conflicting dependency '{}' is declared by composed Manifests with different origins or groups",
                        entry.id
                    ));
                }
            } else {
                entries.insert(entry.id.clone(), entry);
            }
        }

        let manifests = self
            .tool
            .as_ref()
            .and_then(|tool| tool.fastskill.as_ref())
            .map(|fastskill| &fastskill.manifests);
        for (name, declared_path) in manifests.into_iter().flatten() {
            let mut path = if declared_path.is_absolute() {
                declared_path.clone()
            } else {
                manifest_dir.join(declared_path)
            };
            if path.is_dir() {
                path.push("skill-project.toml");
            }
            let canonical = path.canonicalize().map_err(|error| {
                format!(
                    "Failed to load composed Manifest '{name}' at {}: {error}",
                    path.display()
                )
            })?;
            if let Some(position) = stack.iter().position(|candidate| candidate == &canonical) {
                let mut cycle: Vec<_> = stack[position..]
                    .iter()
                    .map(|item| item.display().to_string())
                    .collect();
                cycle.push(canonical.display().to_string());
                return Err(format!(
                    "Manifest composition cycle: {}",
                    cycle.join(" -> ")
                ));
            }
            if !visited.insert(canonical.clone()) {
                continue;
            }
            let referenced = Self::load_from_file(&canonical)
                .map_err(|error| format!("Failed to load composed Manifest '{name}': {error}"))?;
            let referenced_dir = canonical.parent().map(Path::to_path_buf).ok_or_else(|| {
                format!(
                    "Composed Manifest '{}' has no parent directory",
                    canonical.display()
                )
            })?;
            stack.push(canonical);
            referenced.collect_skill_entries(&referenced_dir, stack, visited, entries)?;
            stack.pop();
        }
        Ok(())
    }

    fn direct_skill_entries(&self, manifest_dir: &Path) -> Result<Vec<SkillEntry>, String> {
        let mut entries = Vec::new();
        if let Some(ref dependencies) = self.dependencies {
            for (skill_id, dependency) in &dependencies.dependencies {
                let (origin, groups) = match dependency {
                    DependencySpec::Version(version) => {
                        let constraint = VersionConstraint::parse(version).map_err(|error| {
                            format!("Invalid version '{version}' for {skill_id}: {error}")
                        })?;
                        (
                            Origin::Repository {
                                repo: "default".to_string(),
                                skill: skill_id.clone(),
                                version: Some(constraint),
                            },
                            Vec::new(),
                        )
                    }
                    DependencySpec::Inline { origin, groups } => {
                        (origin.clone(), groups.clone().unwrap_or_default())
                    }
                };
                entries.push(SkillEntry {
                    id: skill_id.clone(),
                    origin: origin.resolved_against(manifest_dir),
                    groups,
                });
            }
        }
        Ok(entries)
    }
}
