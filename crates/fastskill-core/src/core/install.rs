//! The core install seam (ADR-0005): `add_from_origin(origin, mode)`.
//!
//! `add_from_origin = commit(fetch(origin), origin, mode)`. `fetch` is the only
//! per-variant part (match-dispatched, produces a temp dir + the [`Resolved`]
//! facts); `commit` is the shared pipeline (validate → atomic-move into the
//! skills dir → upsert Manifest → write Lock → reindex-if-provider). `mode` only
//! governs the id-conflict policy. `add`/`update` are one operation.

use crate::core::lock::{project_lock_path, ProjectSkillsLock};
use crate::core::manifest::{
    DependenciesSection, DependencySpec, ProjectContext, SkillProjectToml,
};
use crate::core::metadata::{parse_yaml_frontmatter, SkillFrontmatter};
use crate::core::origin::{GitRef, Origin, Resolved};
use crate::core::project::{detect_context_from_content, resolve_project_file};
use crate::core::repository::RepositoryManager;
use crate::core::service::{FastSkillService, ServiceError, SkillId};
use crate::core::skill_manager::SkillDefinition;
use crate::core::version::{is_newer, newest_version, VersionConstraint};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

/// Whether an install is a fresh add (409 on an existing id) or an update
/// (overwrite the recorded skill). See ADR-0005.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddMode {
    /// Adding a new skill: fail if the resolved id is already installed.
    Fresh,
    /// Re-installing an already-recorded skill from its origin: overwrite.
    Update,
}

/// Result of a successful `add_from_origin`.
#[derive(Debug, Clone)]
pub struct AddOutcome {
    pub id: String,
    pub origin: Origin,
    pub resolved: Resolved,
    /// Whether the auto-reindex ran (false = skipped, e.g. no embedding provider).
    pub reindexed: bool,
}

/// The outcome of the update preflight (ADR-0005 §Q6). Only `Updatable` proceeds
/// to a re-fetch; the others are honest no-ops with a reason.
#[derive(Debug, Clone)]
pub enum UpdatePreflight {
    UpToDate,
    Immutable { reason: String },
    Updatable,
}

/// A fetched skill sitting in a temp dir, plus the facts the fetch resolved.
/// The `TempDir` guards the extracted contents until `commit` moves them.
pub struct Fetched {
    pub temp_dir: TempDir,
    pub skill_path: PathBuf,
    pub resolved: Resolved,
}

impl FastSkillService {
    /// Install a single skill from an [`Origin`] (ADR-0005). `Fresh` adds a new
    /// skill (409-style error if the id already exists); `Update` re-resolves the
    /// origin and overwrites. Equivalent to `commit(fetch(origin), origin, mode)`.
    pub async fn add_from_origin(
        &self,
        origin: Origin,
        mode: AddMode,
    ) -> Result<AddOutcome, ServiceError> {
        let fetched = self.fetch(&origin).await?;
        self.commit(fetched, origin, mode).await
    }

    /// Fetch a skill described by `origin` into a temp dir, capturing the resolved
    /// facts. The only per-variant step (git clone / local copy-or-unzip / remote
    /// zip download / registry download).
    async fn fetch(&self, origin: &Origin) -> Result<Fetched, ServiceError> {
        match origin {
            Origin::Git { url, r#ref, subdir } => {
                self.fetch_git(url, r#ref, subdir.as_deref()).await
            }
            Origin::Local { path, editable } => self.fetch_local(path, *editable).await,
            Origin::ZipUrl { url } => self.fetch_zip_url(url).await,
            Origin::Repository {
                repo,
                skill,
                version,
            } => self.fetch_repository(repo, skill, version.as_ref()).await,
        }
    }

    async fn fetch_git(
        &self,
        url: &str,
        git_ref: &GitRef,
        subdir: Option<&Path>,
    ) -> Result<Fetched, ServiceError> {
        let (branch, tag) = match git_ref {
            GitRef::Default => (None, None),
            GitRef::Branch(b) => (Some(b.as_str()), None),
            GitRef::Tag(t) => (None, Some(t.as_str())),
            GitRef::Commit(_) => {
                // No clone-by-commit primitive exists yet (`clone_repository` only
                // takes branch/tag). Surface a clear error rather than mishandling it.
                return Err(ServiceError::InvalidOperation(
                    "Installing a skill pinned to a git commit is not yet supported (no \
                     clone-by-commit primitive)"
                        .to_string(),
                ));
            }
        };

        let temp_dir = crate::storage::git::clone_repository(url, branch, tag, None).await?;

        let skill_base = if let Some(subdir) = subdir {
            let joined = safe_subdir_join(temp_dir.path(), subdir)?;
            if !joined.exists() {
                return Err(ServiceError::InvalidOperation(format!(
                    "Specified subdirectory '{}' does not exist in cloned repository",
                    subdir.display()
                )));
            }
            joined
        } else {
            temp_dir.path().to_path_buf()
        };
        let skill_path = crate::storage::git::validate_cloned_skill(&skill_base)?;

        let commit_hash = git_head_commit(temp_dir.path()).await?;
        let frontmatter = read_skill_frontmatter(&skill_path).await?;
        let (_, version) = derive_skill_id_and_version(&skill_path, &frontmatter)?;

        Ok(Fetched {
            temp_dir,
            skill_path,
            resolved: Resolved {
                version,
                commit_hash: Some(commit_hash),
                checksum: None,
            },
        })
    }

    async fn fetch_local(&self, path: &Path, editable: bool) -> Result<Fetched, ServiceError> {
        let resolved_path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir()?.join(path)
        };
        if !resolved_path.exists() {
            return Err(ServiceError::InvalidOperation(format!(
                "Local path does not exist: {}",
                resolved_path.display()
            )));
        }

        let temp_dir = TempDir::new()?;
        let is_zip = resolved_path.is_file()
            && resolved_path.extension().and_then(|e| e.to_str()) == Some("zip");

        let skill_path = if is_zip {
            let extract_path = temp_dir.path().join("extracted");
            tokio::fs::create_dir_all(&extract_path).await?;
            let zip_handler = crate::storage::zip::ZipHandler::new()?;
            zip_handler.extract_to_dir(&resolved_path, &extract_path)?;
            crate::storage::git::validate_cloned_skill(&extract_path)?
        } else if editable {
            // `commit` will symlink this path in place; the original directory
            // must survive untouched (it stays the live, user-owned source), so
            // `skill_path` points straight at it rather than a temp-dir copy.
            crate::storage::git::validate_cloned_skill(&resolved_path)?
        } else {
            // Non-editable directory: copy into the (throwaway) temp dir so
            // `commit`'s atomic-move step consumes the copy, never the caller's
            // original source directory.
            let validated_source = crate::storage::git::validate_cloned_skill(&resolved_path)?;
            let copy_dest = temp_dir.path().join("copied");
            copy_dir_recursive(&validated_source, &copy_dest).await?;
            copy_dest
        };

        let frontmatter = read_skill_frontmatter(&skill_path).await?;
        let (_, version) = derive_skill_id_and_version(&skill_path, &frontmatter)?;

        Ok(Fetched {
            temp_dir,
            skill_path,
            resolved: Resolved {
                version,
                commit_hash: None,
                checksum: None,
            },
        })
    }

    async fn fetch_zip_url(&self, url: &str) -> Result<Fetched, ServiceError> {
        let response = reqwest::get(url)
            .await
            .map_err(|e| {
                ServiceError::InvalidOperation(format!("Failed to download '{url}': {e}"))
            })?
            .error_for_status()
            .map_err(|e| {
                ServiceError::InvalidOperation(format!("Failed to download '{url}': {e}"))
            })?;
        let bytes = response
            .bytes()
            .await
            .map_err(|e| ServiceError::InvalidOperation(format!("Failed to read '{url}': {e}")))?;

        let temp_dir = TempDir::new()?;
        let zip_path = temp_dir.path().join("package.zip");
        let extract_path = temp_dir.path().join("extracted");
        tokio::fs::write(&zip_path, &bytes).await?;
        tokio::fs::create_dir_all(&extract_path).await?;

        let zip_handler = crate::storage::zip::ZipHandler::new()?;
        zip_handler.extract_to_dir(&zip_path, &extract_path)?;
        let skill_path = crate::storage::git::validate_cloned_skill(&extract_path)?;

        let frontmatter = read_skill_frontmatter(&skill_path).await?;
        let (_, version) = derive_skill_id_and_version(&skill_path, &frontmatter)?;

        Ok(Fetched {
            temp_dir,
            skill_path,
            resolved: Resolved {
                version,
                commit_hash: None,
                checksum: None,
            },
        })
    }

    async fn fetch_repository(
        &self,
        repo: &str,
        skill: &str,
        version: Option<&VersionConstraint>,
    ) -> Result<Fetched, ServiceError> {
        let repo_manager = self.repository_manager().ok_or_else(|| {
            ServiceError::Config(
                "No repositories configured; cannot fetch an Origin::Repository skill".to_string(),
            )
        })?;
        let repo_name = resolve_repo_name(repo_manager, repo)?;
        let client = repo_manager.get_client(&repo_name).await?;

        let available = client
            .get_versions(skill)
            .await
            .map_err(|e| ServiceError::Config(format!("Failed to get versions: {e}")))?;
        let candidates: Vec<String> = match version {
            Some(constraint) => available
                .into_iter()
                .filter(|v| constraint.satisfies(v).unwrap_or(false))
                .collect(),
            None => available,
        };
        let resolved_version = newest_version(&candidates).ok_or_else(|| {
            ServiceError::Config(format!(
                "No version of '{skill}' satisfies the requested constraint in repository \
                 '{repo_name}'"
            ))
        })?;

        let zip_data = client
            .download(skill, &resolved_version)
            .await
            .map_err(|e| ServiceError::Config(format!("Failed to download package: {e}")))?;

        let temp_dir = TempDir::new()?;
        let extract_path = temp_dir.path().join("extracted");
        tokio::fs::create_dir_all(&extract_path).await?;
        let zip_path = temp_dir
            .path()
            .join(format!("package-{resolved_version}.zip"));
        tokio::fs::write(&zip_path, &zip_data).await?;

        let zip_handler = crate::storage::zip::ZipHandler::new()?;
        zip_handler.extract_to_dir(&zip_path, &extract_path)?;
        let skill_path = crate::storage::git::validate_cloned_skill(&extract_path)?;

        let frontmatter = read_skill_frontmatter(&skill_path).await?;
        // The registry-resolved version selected the package to download; the
        // *recorded* resolved version is whatever the downloaded SKILL.md /
        // skill-project.toml declares (matching the pre-seam CLI behavior, where
        // the lock's `Resolved.version` always came from the installed skill's
        // own metadata, not the registry's version string).
        let (_, version_from_skill) = derive_skill_id_and_version(&skill_path, &frontmatter)?;

        Ok(Fetched {
            temp_dir,
            skill_path,
            resolved: Resolved {
                version: version_from_skill,
                commit_hash: None,
                checksum: None,
            },
        })
    }

    /// The shared post-fetch pipeline: validate → atomic-move into the skills dir →
    /// upsert Manifest → write Lock (origin + resolved) → reindex-if-provider.
    /// Ordering is skills-dir → manifest → lock → reindex (never reference a skill
    /// before it exists); each store write is atomic; recovery is idempotent
    /// re-run + reconcile (ADR-0005 §Q3).
    async fn commit(
        &self,
        fetched: Fetched,
        origin: Origin,
        mode: AddMode,
    ) -> Result<AddOutcome, ServiceError> {
        let Fetched {
            temp_dir,
            skill_path,
            resolved,
        } = fetched;

        let frontmatter = read_skill_frontmatter(&skill_path).await?;
        let (id, _version) = derive_skill_id_and_version(&skill_path, &frontmatter)?;

        let existing = self.skill_manager().get_skill(&id).await?;
        if mode == AddMode::Fresh && existing.is_some() {
            return Err(ServiceError::AlreadyIndexed(id.into_string()));
        }

        let storage_dir = self.config().skill_storage_path.join(id.as_str());
        let editable = matches!(&origin, Origin::Local { editable: true, .. });
        if editable {
            symlink_into_storage(&skill_path, &storage_dir).await?;
        } else {
            move_or_copy_into_storage(&skill_path, &storage_dir).await?;
        }
        // The fetched contents now live at `storage_dir` (moved, copied, or
        // symlinked-to); the temp dir (if anything of it remains) can go.
        drop(temp_dir);

        let fetched_at = chrono::Utc::now();
        let mut skill_def = SkillDefinition::new(
            id.clone(),
            frontmatter.name,
            frontmatter.description,
            resolved.version.clone(),
            origin.clone(),
        );
        skill_def.skill_file = storage_dir.join("SKILL.md");
        skill_def.author = frontmatter.author;
        skill_def.commit_hash = resolved.commit_hash.clone();
        skill_def.fetched_at = Some(fetched_at);

        self.skill_manager()
            .force_register_skill(skill_def.clone())
            .await?;

        self.upsert_manifest_and_lock(&skill_def)?;

        let reindexed = match self.reindex(None, None).await {
            Ok(outcome) => outcome.reindexed,
            Err(e) => {
                // A reindex failure must not fail the commit (ADR-0005 §Q3): the
                // skill is already installed and recorded; reindexing is retried
                // by any subsequent `reindex` call.
                tracing::warn!("post-install reindex failed (non-fatal): {}", e);
                false
            }
        };

        Ok(AddOutcome {
            id: id.into_string(),
            origin,
            resolved,
            reindexed,
        })
    }

    /// Upsert the skill-project.toml `[dependencies]` entry and the project
    /// `skills.lock` entry for a just-installed skill. Resolves the project file
    /// from the current working directory (mirrors the CLI's
    /// `manifest_utils::add_skill_to_project_toml` / `update_lock_file`).
    fn upsert_manifest_and_lock(&self, skill_def: &SkillDefinition) -> Result<(), ServiceError> {
        let current_dir = std::env::current_dir()?;
        let project_file_result = resolve_project_file(&current_dir);
        if !project_file_result.found {
            return Err(ServiceError::Config(
                "skill-project.toml not found in this directory or any parent. Run \
                 `fastskill init` at the project root before adding skills."
                    .to_string(),
            ));
        }
        let project_file_path = project_file_result.path;

        let mut project = SkillProjectToml::load_from_file(&project_file_path)
            .map_err(|e| ServiceError::Config(format!("Failed to load skill-project.toml: {e}")))?;

        let mut context = project_file_result.context;
        if context == ProjectContext::Ambiguous {
            context = detect_context_from_content(&project);
        }
        if context == ProjectContext::Skill {
            return Err(ServiceError::Config(
                "Cannot add dependencies to a skill-level skill-project.toml (this directory \
                 contains SKILL.md); run the add from the project root instead."
                    .to_string(),
            ));
        }
        project.validate_for_context(context).map_err(|e| {
            ServiceError::Config(format!("skill-project.toml validation failed: {e}"))
        })?;

        if project.dependencies.is_none() {
            project.dependencies = Some(DependenciesSection {
                dependencies: HashMap::new(),
            });
        }
        if let Some(deps) = project.dependencies.as_mut() {
            // Safety net: re-canonicalize a local path to an absolute path before
            // persisting it, in case the caller passed a relative one.
            let origin_for_manifest = match &skill_def.origin {
                Origin::Local { path, editable } => {
                    let canonical = path.canonicalize()?;
                    Origin::Local {
                        path: canonical,
                        editable: *editable,
                    }
                }
                other => other.clone(),
            };
            deps.dependencies.insert(
                skill_def.id.to_string(),
                DependencySpec::Inline {
                    origin: origin_for_manifest,
                    groups: None,
                },
            );
        }

        project
            .save_to_file(&project_file_path)
            .map_err(|e| ServiceError::Config(format!("Failed to save skill-project.toml: {e}")))?;

        let lock_path = project_lock_path(&project_file_path);
        let mut lock = if lock_path.exists() {
            ProjectSkillsLock::load_from_file(&lock_path)
                .map_err(|e| ServiceError::Config(format!("Failed to load skills.lock: {e}")))?
        } else {
            ProjectSkillsLock::new_empty()
        };
        lock.update_skill(skill_def);
        lock.save_to_file(&lock_path)
            .map_err(|e| ServiceError::Config(format!("Failed to save skills.lock: {e}")))?;

        Ok(())
    }

    /// Update preflight (ADR-0005 §Q6): decide whether the recorded origin has
    /// anything to update before doing any fetch. `repository` → newest allowed
    /// via the repository client; immutable git tag/commit + editable local →
    /// `Immutable`; git branch / local copy / zip-url → `Updatable` (re-fetch;
    /// commit is idempotent).
    pub async fn preflight(&self, origin: &Origin) -> Result<UpdatePreflight, ServiceError> {
        match origin {
            Origin::Git { r#ref, .. } => match r#ref {
                GitRef::Tag(t) => Ok(UpdatePreflight::Immutable {
                    reason: format!("pinned to git tag '{t}'; tags do not move"),
                }),
                GitRef::Commit(c) => Ok(UpdatePreflight::Immutable {
                    reason: format!("pinned to git commit '{c}'; commits do not move"),
                }),
                GitRef::Branch(_) | GitRef::Default => Ok(UpdatePreflight::Updatable),
            },
            Origin::Local { editable, .. } => {
                if *editable {
                    Ok(UpdatePreflight::Immutable {
                        reason: "editable local install is a live symlink to the source \
                                 directory; there is nothing to re-fetch"
                            .to_string(),
                    })
                } else {
                    Ok(UpdatePreflight::Updatable)
                }
            }
            Origin::ZipUrl { .. } => Ok(UpdatePreflight::Updatable),
            Origin::Repository {
                repo,
                skill,
                version,
            } => {
                let repo_manager = self.repository_manager().ok_or_else(|| {
                    ServiceError::Config(
                        "No repositories configured; cannot preflight an Origin::Repository \
                         update"
                            .to_string(),
                    )
                })?;
                let repo_name = resolve_repo_name(repo_manager, repo)?;
                let client = repo_manager.get_client(&repo_name).await?;
                let available = client
                    .get_versions(skill)
                    .await
                    .map_err(|e| ServiceError::Config(format!("Failed to get versions: {e}")))?;
                let candidates: Vec<String> = match version {
                    Some(constraint) => available
                        .into_iter()
                        .filter(|v| constraint.satisfies(v).unwrap_or(false))
                        .collect(),
                    None => available,
                };
                let Some(target_version) = newest_version(&candidates) else {
                    // Nothing satisfies the constraint: no update to offer.
                    return Ok(UpdatePreflight::UpToDate);
                };

                // Best-effort: the currently-installed `SkillId` is normally the
                // last path segment of the registry reference (`scope/id`).
                let local_id = skill.rsplit('/').next().unwrap_or(skill.as_str());
                let installed_version = match SkillId::new(local_id.to_string()) {
                    Ok(id) => self
                        .skill_manager()
                        .get_skill(&id)
                        .await?
                        .map(|s| s.version),
                    Err(_) => None,
                };

                match installed_version {
                    Some(current) if !is_newer(&target_version, &current).unwrap_or(true) => {
                        Ok(UpdatePreflight::UpToDate)
                    }
                    _ => Ok(UpdatePreflight::Updatable),
                }
            }
        }
    }
}

// ── Free helper functions ─────────────────────────────────────────────────────

/// Resolve a `repo` name (the `Origin::Repository.repo` field) against a
/// [`RepositoryManager`]: `"default"` resolves to the configured default
/// repository's name, anything else is used verbatim.
fn resolve_repo_name(repo_manager: &RepositoryManager, repo: &str) -> Result<String, ServiceError> {
    if repo == "default" {
        repo_manager
            .get_default_repository()
            .map(|r| r.name.clone())
            .ok_or_else(|| ServiceError::Config("No default repository configured".to_string()))
    } else {
        Ok(repo.to_string())
    }
}

/// Read and parse `SKILL.md`'s frontmatter from a fetched skill directory.
async fn read_skill_frontmatter(skill_path: &Path) -> Result<SkillFrontmatter, ServiceError> {
    let content = tokio::fs::read_to_string(skill_path.join("SKILL.md")).await?;
    parse_yaml_frontmatter(&content)
}

/// Derive `(SkillId, version)` from a fetched skill directory: `skill-project.toml`
/// `[metadata]` wins when present, else `SKILL.md` frontmatter (`metadata.id`/
/// `.version` sub-map, else `name`/top-level `version`, else `"1.0.0"`). Mirrors
/// `fastskill-cli`'s `create_skill_from_path` precedence.
fn derive_skill_id_and_version(
    skill_path: &Path,
    frontmatter: &SkillFrontmatter,
) -> Result<(SkillId, String), ServiceError> {
    let toml_path = skill_path.join("skill-project.toml");
    let mut id_from_toml = None;
    let mut version_from_toml = None;
    if toml_path.exists() {
        let content = std::fs::read_to_string(&toml_path)?;
        let project: SkillProjectToml = toml::from_str(&content).map_err(|e| {
            ServiceError::Validation(format!("Failed to parse skill-project.toml: {e}"))
        })?;
        if let Some(metadata) = project.metadata {
            id_from_toml = metadata.id;
            version_from_toml = metadata.version;
        }
    }

    let id_str = id_from_toml.unwrap_or_else(|| {
        frontmatter
            .metadata
            .as_ref()
            .and_then(|m| m.get("id").cloned())
            .unwrap_or_else(|| frontmatter.name.clone())
    });
    let id = SkillId::new(id_str)?;

    let version = version_from_toml
        .or_else(|| {
            frontmatter
                .metadata
                .as_ref()
                .and_then(|m| m.get("version").cloned())
        })
        .or_else(|| frontmatter.version.clone())
        .unwrap_or_else(|| "1.0.0".to_string());

    Ok((id, version))
}

/// Safely join an untrusted `subdir` (from a git tree reference) onto a trusted
/// clone `root`, rejecting path traversal. Mirrors the CLI's
/// `install_utils::safe_subdir_join`.
fn safe_subdir_join(root: &Path, subdir: &Path) -> Result<PathBuf, ServiceError> {
    use std::path::Component;

    let mut joined = root.to_path_buf();
    for component in subdir.components() {
        match component {
            Component::Normal(part) => {
                let part = part.to_str().ok_or_else(|| {
                    ServiceError::InvalidOperation(format!(
                        "Subdirectory '{}' contains a non-UTF-8 path component",
                        subdir.display()
                    ))
                })?;
                crate::security::path::validate_path_component(part).map_err(|e| {
                    ServiceError::InvalidOperation(format!(
                        "Invalid subdirectory '{}': {}",
                        subdir.display(),
                        e
                    ))
                })?;
                joined.push(part);
            }
            // `..`, root (`/`), prefix (`C:`), etc. are all traversal / absolute markers.
            _ => {
                return Err(ServiceError::InvalidOperation(format!(
                    "Subdirectory '{}' must be a relative path without '..' components",
                    subdir.display()
                )));
            }
        }
    }

    if joined.exists() {
        let canonical_root = root.canonicalize()?;
        let canonical_joined = joined.canonicalize()?;
        if !canonical_joined.starts_with(&canonical_root) {
            return Err(ServiceError::InvalidOperation(format!(
                "Subdirectory '{}' escapes the cloned repository",
                subdir.display()
            )));
        }
    }

    Ok(joined)
}

/// Resolve `HEAD`'s commit SHA in a freshly-cloned git repository.
async fn git_head_commit(repo_dir: &Path) -> Result<String, ServiceError> {
    let output = tokio::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(repo_dir)
        .output()
        .await?;
    if !output.status.success() {
        return Err(ServiceError::Custom(format!(
            "git rev-parse HEAD failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Remove whatever currently sits at `path` (file, symlink, or directory), if
/// anything, so a fresh move/copy/symlink can take its place.
async fn remove_existing_storage_path(path: &Path) -> Result<(), ServiceError> {
    if path.is_symlink() || path.is_file() {
        tokio::fs::remove_file(path).await?;
    } else if path.exists() {
        tokio::fs::remove_dir_all(path).await?;
    }
    Ok(())
}

/// Move `skill_path` into `storage_dir` (same-filesystem rename when possible,
/// falling back to a recursive copy across filesystems/temp-dir boundaries).
async fn move_or_copy_into_storage(
    skill_path: &Path,
    storage_dir: &Path,
) -> Result<(), ServiceError> {
    remove_existing_storage_path(storage_dir).await?;
    if let Some(parent) = storage_dir.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    if tokio::fs::rename(skill_path, storage_dir).await.is_ok() {
        return Ok(());
    }
    // Cross-device (or other rename failure): fall back to a recursive copy.
    copy_dir_recursive(skill_path, storage_dir).await
}

/// Symlink `storage_dir` -> `skill_path` (editable local installs).
async fn symlink_into_storage(skill_path: &Path, storage_dir: &Path) -> Result<(), ServiceError> {
    remove_existing_storage_path(storage_dir).await?;
    if let Some(parent) = storage_dir.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    #[cfg(unix)]
    {
        tokio::fs::symlink(skill_path, storage_dir).await?;
    }
    #[cfg(windows)]
    {
        std::os::windows::fs::symlink_dir(skill_path, storage_dir)?;
    }
    #[cfg(all(not(unix), not(windows)))]
    {
        return Err(ServiceError::InvalidOperation(
            "Editable installations are not supported on this platform. Use a Unix-based \
             system or Docker."
                .to_string(),
        ));
    }
    Ok(())
}

/// Recursively copy a directory from `src` to `dst`, rejecting symlink entries
/// (SEC-4: a symlink inside the source tree must not be silently dereferenced
/// and its target's contents exfiltrated into the copy).
async fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), ServiceError> {
    tokio::fs::create_dir_all(dst).await?;
    let mut entries = tokio::fs::read_dir(src).await?;
    while let Some(entry) = entries.next_entry().await? {
        let ty = entry.file_type().await?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        if ty.is_symlink() {
            return Err(ServiceError::Validation(format!(
                "refusing to copy symlink: {}",
                src_path.display()
            )));
        }
        if ty.is_dir() {
            Box::pin(copy_dir_recursive(&src_path, &dst_path)).await?;
        } else {
            tokio::fs::copy(&src_path, &dst_path).await?;
        }
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::{FastSkillService, ServiceConfig};
    use tempfile::TempDir as TestTempDir;

    const VALID_SKILL_MD: &str =
        "---\nname: test-skill\nversion: \"1.0.0\"\ndescription: A test skill\n---\nBody\n";

    fn write_valid_skill(parent: &Path, dir_name: &str) -> PathBuf {
        let dir = parent.join(dir_name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("SKILL.md"), VALID_SKILL_MD).unwrap();
        dir
    }

    /// Set up a project directory (skill-project.toml + skills dir) and chdir
    /// into it, returning a guard that restores the cwd and holds the temp dir.
    fn setup_project() -> (TestTempDir, crate::test_utils::DirGuard, PathBuf) {
        let tmp = TestTempDir::new().unwrap();
        let original_dir = std::env::current_dir().ok();
        std::env::set_current_dir(tmp.path()).unwrap();
        std::fs::write(
            tmp.path().join("skill-project.toml"),
            "[tool.fastskill]\nskills_directory = \".claude/skills\"\n\n[dependencies]\n",
        )
        .unwrap();
        let skills_dir = tmp.path().join(".claude/skills");
        std::fs::create_dir_all(&skills_dir).unwrap();
        (tmp, crate::test_utils::DirGuard(original_dir), skills_dir)
    }

    async fn make_service(storage: &Path) -> FastSkillService {
        let config = ServiceConfig {
            skill_storage_path: storage.to_path_buf(),
            ..Default::default()
        };
        let mut service = FastSkillService::new(config).await.unwrap();
        service.initialize().await.unwrap();
        service
    }

    // ── add_from_origin: Local, end-to-end ────────────────────────────────────

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn test_add_from_origin_local_end_to_end() {
        let _lock = crate::test_utils::DIR_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let (tmp, _guard, skills_dir) = setup_project();
        let src = write_valid_skill(tmp.path(), "src-skill");
        let service = make_service(&skills_dir).await;

        let origin = Origin::Local {
            path: src.clone(),
            editable: false,
        };
        let outcome = service
            .add_from_origin(origin, AddMode::Fresh)
            .await
            .expect("add should succeed");

        assert_eq!(outcome.id, "test-skill");
        assert_eq!(outcome.resolved.version, "1.0.0");
        assert!(skills_dir.join("test-skill/SKILL.md").exists());

        // Manifest + lock were written.
        let project = SkillProjectToml::load_from_file(&tmp.path().join("skill-project.toml"))
            .expect("manifest should load");
        assert!(project
            .dependencies
            .expect("deps section")
            .dependencies
            .contains_key("test-skill"));
        let lock = ProjectSkillsLock::load_from_file(&tmp.path().join("skills.lock"))
            .expect("lock should load");
        assert_eq!(lock.skills.len(), 1);
        assert_eq!(lock.skills[0].id, "test-skill");

        // Registered with the skill manager.
        let id = SkillId::new("test-skill".to_string()).unwrap();
        assert!(service
            .skill_manager()
            .get_skill(&id)
            .await
            .unwrap()
            .is_some());
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn test_add_from_origin_fresh_conflict() {
        let _lock = crate::test_utils::DIR_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let (tmp, _guard, skills_dir) = setup_project();
        let src = write_valid_skill(tmp.path(), "src-skill");
        let service = make_service(&skills_dir).await;

        let origin = Origin::Local {
            path: src.clone(),
            editable: false,
        };
        service
            .add_from_origin(origin.clone(), AddMode::Fresh)
            .await
            .expect("first add should succeed");

        let result = service.add_from_origin(origin, AddMode::Fresh).await;
        assert!(matches!(result, Err(ServiceError::AlreadyIndexed(_))));
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn test_add_from_origin_update_overwrites() {
        let _lock = crate::test_utils::DIR_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let (tmp, _guard, skills_dir) = setup_project();
        let src = write_valid_skill(tmp.path(), "src-skill");
        let service = make_service(&skills_dir).await;

        let origin = Origin::Local {
            path: src.clone(),
            editable: false,
        };
        service
            .add_from_origin(origin.clone(), AddMode::Fresh)
            .await
            .expect("first add should succeed");

        // Update the source content, then re-add via Update mode.
        std::fs::write(
            src.join("SKILL.md"),
            "---\nname: test-skill\nversion: \"2.0.0\"\ndescription: updated\n---\nBody\n",
        )
        .unwrap();

        let outcome = service
            .add_from_origin(origin, AddMode::Update)
            .await
            .expect("update should succeed");
        assert_eq!(outcome.resolved.version, "2.0.0");

        let id = SkillId::new("test-skill".to_string()).unwrap();
        let skill = service
            .skill_manager()
            .get_skill(&id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(skill.version, "2.0.0");
    }

    #[cfg(unix)]
    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn test_add_from_origin_local_editable_symlinks() {
        let _lock = crate::test_utils::DIR_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let (tmp, _guard, skills_dir) = setup_project();
        let src = write_valid_skill(tmp.path(), "src-skill");
        let service = make_service(&skills_dir).await;

        let origin = Origin::Local {
            path: src.clone(),
            editable: true,
        };
        let outcome = service
            .add_from_origin(origin, AddMode::Fresh)
            .await
            .expect("editable add should succeed");

        let storage_path = skills_dir.join(&outcome.id);
        assert!(storage_path.is_symlink(), "editable install must symlink");
    }

    #[tokio::test]
    async fn test_add_from_origin_local_nonexistent_path() {
        let tmp = TestTempDir::new().unwrap();
        let storage = tmp.path().join("storage");
        let service = make_service(&storage).await;

        let origin = Origin::Local {
            path: tmp.path().join("does-not-exist"),
            editable: false,
        };
        let result = service.add_from_origin(origin, AddMode::Fresh).await;
        assert!(matches!(result, Err(ServiceError::InvalidOperation(_))));
    }

    // ── add_from_origin: ZipUrl, end-to-end (mock HTTP) ───────────────────────

    fn build_skill_zip() -> Vec<u8> {
        use std::io::Write;
        use zip::write::FileOptions;
        let mut buf = Vec::new();
        {
            let cursor = std::io::Cursor::new(&mut buf);
            let mut writer = zip::ZipWriter::new(cursor);
            let opts = FileOptions::default().compression_method(zip::CompressionMethod::Stored);
            writer.start_file("test-skill/SKILL.md", opts).unwrap();
            writer.write_all(VALID_SKILL_MD.as_bytes()).unwrap();
            writer.finish().unwrap();
        }
        buf
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn test_add_from_origin_zip_url_end_to_end() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let _lock = crate::test_utils::DIR_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let (_tmp, _guard, skills_dir) = setup_project();
        let service = make_service(&skills_dir).await;

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/pkg.zip"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(build_skill_zip()))
            .mount(&server)
            .await;

        let origin = Origin::ZipUrl {
            url: format!("{}/pkg.zip", server.uri()),
        };
        let outcome = service
            .add_from_origin(origin, AddMode::Fresh)
            .await
            .expect("zip-url add should succeed");
        assert_eq!(outcome.id, "test-skill");
        assert!(skills_dir.join("test-skill/SKILL.md").exists());
    }

    // ── add_from_origin: Repository without a repository manager ─────────────

    #[tokio::test]
    async fn test_add_from_origin_repository_requires_manager() {
        let tmp = TestTempDir::new().unwrap();
        let storage = tmp.path().join("storage");
        let service = make_service(&storage).await;

        let origin = Origin::Repository {
            repo: "default".to_string(),
            skill: "scope/skill".to_string(),
            version: None,
        };
        let result = service.add_from_origin(origin, AddMode::Fresh).await;
        assert!(matches!(result, Err(ServiceError::Config(_))));
    }

    // ── fetch_git: GitRef::Commit is a clear, fast error (no clone-by-commit) ──

    #[tokio::test]
    async fn test_add_from_origin_git_commit_ref_unsupported() {
        let tmp = TestTempDir::new().unwrap();
        let storage = tmp.path().join("storage");
        let service = make_service(&storage).await;

        let origin = Origin::Git {
            url: "https://example.com/x.git".to_string(),
            r#ref: GitRef::Commit("deadbeef".to_string()),
            subdir: None,
        };
        let result = service.add_from_origin(origin, AddMode::Fresh).await;
        assert!(matches!(result, Err(ServiceError::InvalidOperation(_))));
    }

    // ── preflight ──────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_preflight_git_tag_is_immutable() {
        let tmp = TestTempDir::new().unwrap();
        let service = make_service(&tmp.path().join("storage")).await;
        let origin = Origin::Git {
            url: "u".to_string(),
            r#ref: GitRef::Tag("v1.0.0".to_string()),
            subdir: None,
        };
        assert!(matches!(
            service.preflight(&origin).await.unwrap(),
            UpdatePreflight::Immutable { .. }
        ));
    }

    #[tokio::test]
    async fn test_preflight_git_commit_is_immutable() {
        let tmp = TestTempDir::new().unwrap();
        let service = make_service(&tmp.path().join("storage")).await;
        let origin = Origin::Git {
            url: "u".to_string(),
            r#ref: GitRef::Commit("abc123".to_string()),
            subdir: None,
        };
        assert!(matches!(
            service.preflight(&origin).await.unwrap(),
            UpdatePreflight::Immutable { .. }
        ));
    }

    #[tokio::test]
    async fn test_preflight_git_branch_is_updatable() {
        let tmp = TestTempDir::new().unwrap();
        let service = make_service(&tmp.path().join("storage")).await;
        let origin = Origin::Git {
            url: "u".to_string(),
            r#ref: GitRef::Branch("main".to_string()),
            subdir: None,
        };
        assert!(matches!(
            service.preflight(&origin).await.unwrap(),
            UpdatePreflight::Updatable
        ));
    }

    #[tokio::test]
    async fn test_preflight_git_default_is_updatable() {
        let tmp = TestTempDir::new().unwrap();
        let service = make_service(&tmp.path().join("storage")).await;
        let origin = Origin::Git {
            url: "u".to_string(),
            r#ref: GitRef::Default,
            subdir: None,
        };
        assert!(matches!(
            service.preflight(&origin).await.unwrap(),
            UpdatePreflight::Updatable
        ));
    }

    #[tokio::test]
    async fn test_preflight_local_editable_is_immutable() {
        let tmp = TestTempDir::new().unwrap();
        let service = make_service(&tmp.path().join("storage")).await;
        let origin = Origin::Local {
            path: tmp.path().to_path_buf(),
            editable: true,
        };
        assert!(matches!(
            service.preflight(&origin).await.unwrap(),
            UpdatePreflight::Immutable { .. }
        ));
    }

    #[tokio::test]
    async fn test_preflight_local_copy_is_updatable() {
        let tmp = TestTempDir::new().unwrap();
        let service = make_service(&tmp.path().join("storage")).await;
        let origin = Origin::Local {
            path: tmp.path().to_path_buf(),
            editable: false,
        };
        assert!(matches!(
            service.preflight(&origin).await.unwrap(),
            UpdatePreflight::Updatable
        ));
    }

    #[tokio::test]
    async fn test_preflight_zip_url_is_updatable() {
        let tmp = TestTempDir::new().unwrap();
        let service = make_service(&tmp.path().join("storage")).await;
        let origin = Origin::ZipUrl {
            url: "https://example.com/x.zip".to_string(),
        };
        assert!(matches!(
            service.preflight(&origin).await.unwrap(),
            UpdatePreflight::Updatable
        ));
    }

    #[tokio::test]
    async fn test_preflight_repository_requires_manager() {
        let tmp = TestTempDir::new().unwrap();
        let service = make_service(&tmp.path().join("storage")).await;
        let origin = Origin::Repository {
            repo: "default".to_string(),
            skill: "scope/skill".to_string(),
            version: None,
        };
        let result = service.preflight(&origin).await;
        assert!(matches!(result, Err(ServiceError::Config(_))));
    }

    // ── resolve_repo_name ──────────────────────────────────────────────────────

    #[test]
    fn test_resolve_repo_name_default_alias() {
        use crate::core::repository::{RepositoryConfig, RepositoryDefinition, RepositoryType};
        let manager = RepositoryManager::from_definitions(vec![RepositoryDefinition {
            name: "my-registry".to_string(),
            repo_type: RepositoryType::HttpRegistry,
            priority: 0,
            config: RepositoryConfig::HttpRegistry {
                index_url: "https://example.com/index".to_string(),
            },
            auth: None,
            storage: None,
        }]);
        assert_eq!(
            resolve_repo_name(&manager, "default").unwrap(),
            "my-registry"
        );
        assert_eq!(
            resolve_repo_name(&manager, "my-registry").unwrap(),
            "my-registry"
        );
    }

    #[test]
    fn test_resolve_repo_name_no_repositories_errors() {
        let manager = RepositoryManager::from_definitions(Vec::new());
        assert!(resolve_repo_name(&manager, "default").is_err());
    }

    // ── safe_subdir_join ───────────────────────────────────────────────────────

    #[test]
    fn test_safe_subdir_join_rejects_dotdot() {
        let root = TestTempDir::new().unwrap();
        let result = safe_subdir_join(root.path(), Path::new("../../../etc"));
        assert!(matches!(result, Err(ServiceError::InvalidOperation(_))));
    }

    #[test]
    fn test_safe_subdir_join_rejects_absolute() {
        let root = TestTempDir::new().unwrap();
        let result = safe_subdir_join(root.path(), Path::new("/etc/passwd"));
        assert!(matches!(result, Err(ServiceError::InvalidOperation(_))));
    }

    #[test]
    fn test_safe_subdir_join_accepts_nested_relative() {
        let root = TestTempDir::new().unwrap();
        std::fs::create_dir_all(root.path().join("skills/inner")).unwrap();
        let joined = safe_subdir_join(root.path(), Path::new("skills/inner")).unwrap();
        assert_eq!(joined, root.path().join("skills").join("inner"));
    }

    // ── copy_dir_recursive ─────────────────────────────────────────────────────

    #[cfg(unix)]
    #[tokio::test]
    async fn test_copy_dir_recursive_rejects_symlink() {
        use std::os::unix::fs::symlink;
        let tmp = TestTempDir::new().unwrap();
        let src = tmp.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("SKILL.md"), "# skill\n").unwrap();
        let secret = tmp.path().join("secret.txt");
        std::fs::write(&secret, "TOP SECRET").unwrap();
        symlink(&secret, src.join("creds")).unwrap();

        let dst = tmp.path().join("dst");
        let result = copy_dir_recursive(&src, &dst).await;
        assert!(matches!(result, Err(ServiceError::Validation(_))));
        assert!(!dst.join("creds").exists());
    }

    #[tokio::test]
    async fn test_copy_dir_recursive_copies_regular_tree() {
        let tmp = TestTempDir::new().unwrap();
        let src = tmp.path().join("src");
        std::fs::create_dir_all(src.join("nested")).unwrap();
        std::fs::write(src.join("SKILL.md"), "# skill\n").unwrap();
        std::fs::write(src.join("nested/file.txt"), "data").unwrap();

        let dst = tmp.path().join("dst");
        copy_dir_recursive(&src, &dst).await.unwrap();
        assert!(dst.join("SKILL.md").exists());
        assert!(dst.join("nested/file.txt").exists());
    }

    // ── derive_skill_id_and_version ────────────────────────────────────────────

    #[test]
    fn test_derive_skill_id_and_version_toml_wins() {
        let tmp = TestTempDir::new().unwrap();
        std::fs::write(
            tmp.path().join("SKILL.md"),
            "---\nname: from-md\nversion: \"2.0.0\"\ndescription: d\n---\n",
        )
        .unwrap();
        std::fs::write(
            tmp.path().join("skill-project.toml"),
            "[metadata]\nid = \"from-toml\"\nversion = \"1.5.0\"\n",
        )
        .unwrap();
        let content = std::fs::read_to_string(tmp.path().join("SKILL.md")).unwrap();
        let frontmatter = parse_yaml_frontmatter(&content).unwrap();
        let (id, version) = derive_skill_id_and_version(tmp.path(), &frontmatter).unwrap();
        assert_eq!(id.as_str(), "from-toml");
        assert_eq!(version, "1.5.0");
    }

    #[test]
    fn test_derive_skill_id_and_version_falls_back_to_frontmatter_name() {
        let tmp = TestTempDir::new().unwrap();
        std::fs::write(
            tmp.path().join("SKILL.md"),
            "---\nname: fallback-name\ndescription: d\n---\n",
        )
        .unwrap();
        let content = std::fs::read_to_string(tmp.path().join("SKILL.md")).unwrap();
        let frontmatter = parse_yaml_frontmatter(&content).unwrap();
        let (id, version) = derive_skill_id_and_version(tmp.path(), &frontmatter).unwrap();
        assert_eq!(id.as_str(), "fallback-name");
        assert_eq!(version, "1.0.0");
    }
}
