use super::{
    DependencySpec, RepositoryConnection, RepositoryDefinition, SkillEntry, SkillProjectToml,
};
use crate::core::origin::Origin;
use crate::core::version::VersionConstraint;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Called once per Manifest in a composition: the Manifest, the directory its
/// relative paths are anchored to, and the file it was read from.
type Visit<'a> = dyn FnMut(&SkillProjectToml, &Path, &Path) -> Result<(), String> + 'a;

impl SkillProjectToml {
    /// Read dependencies of an installed or fetched skill without following host paths.
    /// Manifest composition is a project-authoring feature, not package metadata.
    pub(crate) fn to_package_skill_entries(
        &self,
        manifest_dir: &Path,
    ) -> Result<Vec<SkillEntry>, String> {
        if self
            .tool
            .as_ref()
            .and_then(|tool| tool.fastskill.as_ref())
            .is_some_and(|config| !config.manifests.is_empty())
        {
            return Err(
                "Manifest composition is only supported in project manifests, not skill packages"
                    .into(),
            );
        }
        self.direct_skill_entries(manifest_dir)
    }

    /// Convert local and composed Manifest dependencies to install roots.
    /// Relative origins stay anchored to the Manifest that declares them.
    pub fn to_skill_entries(&self, manifest_dir: &Path) -> Result<Vec<SkillEntry>, String> {
        let mut entries: BTreeMap<String, SkillEntry> = BTreeMap::new();
        self.visit_composed(manifest_dir, &mut |manifest, dir, _| {
            for entry in manifest.direct_skill_entries(dir)? {
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
            Ok(())
        })?;
        Ok(entries.into_values().collect())
    }

    /// Repositories declared by this Manifest and every Manifest it composes,
    /// so a composed repository origin resolves through the catalog its own
    /// Manifest names. A composed Manifest's relative local path is anchored to
    /// that Manifest; this Manifest's own entries are returned as declared.
    /// One name defined two different ways is an error, never a silent pick.
    pub fn composed_repositories(
        &self,
        manifest_dir: &Path,
    ) -> Result<Vec<RepositoryDefinition>, String> {
        let root_path = manifest_dir.join("skill-project.toml");
        let mut repositories = Vec::new();
        let mut declared: HashMap<String, (toml::Value, PathBuf)> = HashMap::new();
        self.visit_composed(manifest_dir, &mut |manifest, dir, path| {
            let declared_here = manifest
                .tool
                .as_ref()
                .and_then(|tool| tool.fastskill.as_ref())
                .and_then(|fastskill| fastskill.repositories.as_ref());
            for repository in declared_here.into_iter().flatten() {
                let anchored = anchored_to(repository, dir);
                let identity = toml::Value::try_from(&anchored).map_err(|error| {
                    format!(
                        "Repository '{}' cannot be compared: {error}",
                        repository.name
                    )
                })?;
                match declared.get(&repository.name) {
                    Some((existing, _)) if *existing == identity => {}
                    Some((_, first)) => {
                        return Err(format!(
                            "Repository '{}' is defined differently by {} and {}; \
                             give one of them another name",
                            repository.name,
                            first.display(),
                            path.display()
                        ));
                    }
                    None => {
                        declared.insert(repository.name.clone(), (identity, path.to_path_buf()));
                        repositories.push(if path == root_path {
                            repository.clone()
                        } else {
                            anchored
                        });
                    }
                }
            }
            Ok(())
        })?;
        Ok(repositories)
    }

    /// Visit this Manifest, then every Manifest it composes, depth first and
    /// each once. A reference cycle is an error.
    fn visit_composed(&self, manifest_dir: &Path, visit: &mut Visit<'_>) -> Result<(), String> {
        let root_path = manifest_dir.join("skill-project.toml");
        let canonical_root = root_path
            .canonicalize()
            .unwrap_or_else(|_| root_path.clone());
        let mut stack = vec![canonical_root.clone()];
        let mut visited = HashSet::from([canonical_root]);
        self.walk_composed(manifest_dir, &root_path, &mut stack, &mut visited, visit)
    }

    fn walk_composed(
        &self,
        manifest_dir: &Path,
        manifest_path: &Path,
        stack: &mut Vec<PathBuf>,
        visited: &mut HashSet<PathBuf>,
        visit: &mut Visit<'_>,
    ) -> Result<(), String> {
        visit(self, manifest_dir, manifest_path)?;

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
            let mut canonical = path.canonicalize().map_err(|error| {
                format!(
                    "Failed to load composed Manifest '{name}' at {}: {error}",
                    path.display()
                )
            })?;
            if canonical.is_dir() {
                path.push("skill-project.toml");
                canonical = canonical
                    .join("skill-project.toml")
                    .canonicalize()
                    .map_err(|error| {
                        format!("Failed to load composed Manifest '{name}': {error}")
                    })?;
            }
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
            // Canonical paths identify cycles, but origins retain the caller's path
            // spelling, including Windows non-verbatim prefixes and symlink paths.
            let referenced_dir = path.parent().map(Path::to_path_buf).ok_or_else(|| {
                format!(
                    "Composed Manifest '{}' has no parent directory",
                    canonical.display()
                )
            })?;
            stack.push(canonical);
            referenced.walk_composed(&referenced_dir, &path, stack, visited, visit)?;
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

/// The repository with a relative local path resolved against the directory
/// of the Manifest that declares it, canonical when the directory exists so
/// two spellings of one catalog compare equal.
fn anchored_to(repository: &RepositoryDefinition, manifest_dir: &Path) -> RepositoryDefinition {
    let mut anchored = repository.clone();
    if let RepositoryConnection::Local { path } = &mut anchored.connection {
        let joined = manifest_dir.join(&*path);
        let resolved = joined.canonicalize().unwrap_or(joined);
        *path = resolved.to_string_lossy().into_owned();
    }
    anchored
}
