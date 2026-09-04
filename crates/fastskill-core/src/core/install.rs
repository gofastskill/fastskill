//! The core install seam (ADR-0005): `add_from_origin(origin, mode)`.
//!
//! `add_from_origin = commit(fetch(origin), origin, mode)`. `fetch` is the only
//! per-variant part (match-dispatched, produces a temp dir + the [`Resolved`]
//! facts); `commit` is the shared pipeline (validate → atomic-move into the
//! skills dir → upsert Manifest → write Lock → reindex-if-provider). `mode` only
//! governs the id-conflict policy. `add`/`update` are one operation.

use crate::core::cache::{CacheIdentity, SkillCache, SourceIndex, SourceIndexEntry, ZipValidator};
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
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
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
    /// Non-fatal things the caller should show the user — currently a local
    /// origin outside the project tree, which cannot be recorded portably. The
    /// install succeeded; the Manifest just will not resolve elsewhere.
    pub warnings: Vec<String>,
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
    /// origin and overwrites. `groups` are recorded on the Manifest + Lock entry;
    /// on `Update`, an empty `groups` preserves whatever groups the skill already
    /// had. Equivalent to `commit(fetch(origin), origin, mode, groups)`.
    pub async fn add_from_origin(
        &self,
        origin: Origin,
        mode: AddMode,
        groups: Vec<String>,
    ) -> Result<AddOutcome, ServiceError> {
        let fetched = self.fetch(&origin).await?;
        self.commit(fetched, origin, mode, groups).await
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

        // PRD 006 / RFQ 004 (US-002): resolve the ref to a SHA before deciding
        // whether to clone at all, so a repeat install of the same commit — even
        // from a different project — never re-clones.
        let cache = self.skill_cache();
        let ref_key = git_ref_cache_key(git_ref);
        let resolved_sha = resolve_git_sha(cache, url, &ref_key, branch, tag).await?;

        let (temp_dir, commit_hash) = if let Some(cached) = cache.get(&CacheIdentity::Git {
            sha: resolved_sha.clone(),
        }) {
            // Cache hit: copy the previously-cloned content out of the
            // (immutable, shared) cache entry rather than cloning again.
            let temp_dir = TempDir::new()?;
            copy_dir_recursive(&cached.path, temp_dir.path()).await?;
            (temp_dir, resolved_sha)
        } else {
            // Miss: clone as before. The commit actually landed on is the
            // source of truth for both the cache key and the recorded
            // resolved fact — it may differ from `resolved_sha` if the ref
            // moved between the `ls_remote` above and this clone (a race,
            // not an error); using it here self-heals the index instead of
            // caching under a now-stale SHA.
            let temp_dir = crate::storage::git::clone_repository(url, branch, tag, None).await?;
            let actual_sha = git_head_commit(temp_dir.path()).await?;
            // `.git` metadata is only ever needed transiently, to resolve the
            // commit just cloned to (just done, above) — the content cache is
            // only ever read back as skill *files* (`copy_dir_recursive`
            // above), never as a git repository, so caching `.git` is pure
            // bloat (and can dwarf the skill itself on a real repository).
            // Strip it before publishing so the cache only ever holds skill
            // content. Best-effort: if this fails, the clone (and thus the
            // install) is still perfectly usable — it only means this one
            // `put` below caches `.git` too, same as before this fix.
            if let Err(e) = strip_git_dir(temp_dir.path()).await {
                tracing::warn!(
                    "failed to strip .git before publishing to the content cache: {}",
                    e
                );
            }

            if let Err(e) = cache.put(
                &CacheIdentity::Git {
                    sha: actual_sha.clone(),
                },
                temp_dir.path(),
            ) {
                // The clone already succeeded and is usable; a cache-write
                // failure only costs this run the "install once per
                // machine" win, so it must not fail the install.
                tracing::warn!("failed to publish git clone to content cache: {}", e);
            }
            if let Err(e) = record_git_resolution(cache, url, &ref_key, &actual_sha) {
                tracing::warn!("failed to record git ref resolution: {}", e);
            }

            (temp_dir, actual_sha)
        };

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
        // A relative local path is relative to the *project*, not to whatever
        // directory this process happens to be in. On the server, cwd is
        // arbitrary and resolving against it would read some other tree
        // entirely; for a CLI invocation the injected root is absent and cwd
        // is the project, which is the same answer.
        let resolved_path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            let base = match self.project_root() {
                Some(root) => root.clone(),
                None => std::env::current_dir()?,
            };
            base.join(path)
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
            // A `.zip` is always extracted (never symlinked), matching the
            // pre-cache behavior where `editable` had no effect on a zip
            // source: there is no "live" directory to point a symlink at.
            fetch_local_zip(self.skill_cache(), &resolved_path, temp_dir.path()).await?
        } else if editable {
            // FR-7: an editable install bypasses the content cache entirely.
            // `commit` will symlink this path in place; the original directory
            // must survive untouched (it stays the live, user-owned source), so
            // `skill_path` points straight at it rather than a temp-dir copy.
            crate::storage::git::validate_cloned_skill(&resolved_path)?
        } else {
            fetch_local_dir(self.skill_cache(), &resolved_path, temp_dir.path()).await?
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

    /// Fetch a remote `.zip` (spec 007 "zip-url caching"). Unlike every other
    /// origin, a bare URL resolves to no identity *before* fetching — no SHA,
    /// no pinned version, no local tree already on disk — so this cannot
    /// just mirror `fetch_git`/`fetch_repository`'s "resolve identity, then
    /// check cache" shape. Instead it prefers an HTTP conditional request
    /// (`If-None-Match`/`If-Modified-Since` against a previously-recorded
    /// `ETag`/`Last-Modified`) as the cheap "did it change?" check, with
    /// download-then-hash as the correctness fallback when a server sends no
    /// validators at all. See [`fetch_zip_url_cached`] for the full state
    /// machine.
    async fn fetch_zip_url(&self, url: &str) -> Result<Fetched, ServiceError> {
        let cache = self.skill_cache();
        let client = reqwest::Client::new();
        let temp_dir = TempDir::new()?;
        let skill_path = fetch_zip_url_cached(cache, &client, url, temp_dir.path()).await?;

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
        let cache = self.skill_cache();

        // PRD 006 / RFQ 004 (US-003): resolve to a concrete version before
        // touching the network at all. An exact pin needs no listing —
        // `resolved_version` is already known — so the offline "a pinned,
        // cached version installs with no network" criterion holds even
        // before the content-cache check below. `newest`/a range constraint
        // resolves via the on-disk index `repos refresh` populates (US-005),
        // not a live listing call.
        let resolved_version = resolve_registry_version(cache, &repo_name, skill, version)?;

        let identity = CacheIdentity::Registry {
            source: repo_name.clone(),
            skill: skill.to_string(),
            version: resolved_version.clone(),
        };

        let temp_dir = TempDir::new()?;
        let skill_path = if let Some(cached) = cache.get(&identity) {
            // Cache hit: copy the previously-downloaded skill out of the
            // (immutable, shared) cache entry rather than downloading again.
            let dest = temp_dir.path().join("cached");
            copy_dir_recursive(&cached.path, &dest).await?;
            crate::storage::git::validate_cloned_skill(&dest)?
        } else {
            // Miss: download and extract as before, then publish into the
            // content cache under the resolved version's identity.
            let client = repo_manager.get_client(&repo_name).await?;
            let zip_data = client
                .download(skill, &resolved_version)
                .await
                .map_err(|e| ServiceError::Config(format!("Failed to download package: {e}")))?;

            let extract_path = temp_dir.path().join("extracted");
            tokio::fs::create_dir_all(&extract_path).await?;
            let zip_path = temp_dir
                .path()
                .join(format!("package-{resolved_version}.zip"));
            tokio::fs::write(&zip_path, &zip_data).await?;

            let zip_handler = crate::storage::zip::ZipHandler::new()?;
            zip_handler.extract_to_dir(&zip_path, &extract_path)?;
            let skill_path = crate::storage::git::validate_cloned_skill(&extract_path)?;

            if let Err(e) = cache.put(&identity, &skill_path) {
                // The download already succeeded and is usable; a cache-write
                // failure only costs this run the "download once per
                // machine" win, so it must not fail the install.
                tracing::warn!("failed to publish registry package to content cache: {}", e);
            }
            skill_path
        };

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
        groups: Vec<String>,
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

        let warnings = self.upsert_manifest_and_lock(&skill_def, &groups)?;

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
            warnings,
        })
    }

    /// Upsert the skill-project.toml `[dependencies]` entry and the project
    /// `skills.lock` entry for a just-installed skill. Resolves the project file
    /// from the current working directory (mirrors the CLI's
    /// `manifest_utils::add_skill_to_project_toml` / `update_lock_file`).
    ///
    /// Returns any warnings the caller should surface (see [`AddOutcome`]).
    fn upsert_manifest_and_lock(
        &self,
        skill_def: &SkillDefinition,
        groups: &[String],
    ) -> Result<Vec<String>, ServiceError> {
        // Resolve the project from the injected root (the served project, for the
        // `serve` path) if present; otherwise walk up from the process cwd, which
        // is correct for a CLI invocation. Never resolve solely from cwd on the
        // server, where cwd is arbitrary (would write to the wrong project).
        let start_dir = match self.project_root() {
            Some(root) => root.clone(),
            None => std::env::current_dir()?,
        };
        let project_file_result = resolve_project_file(&start_dir);
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
        // Effective groups: explicit `groups` win; an empty list preserves whatever
        // the skill already had (so `update` never silently drops group membership).
        let effective_groups: Option<Vec<String>> = if !groups.is_empty() {
            Some(groups.to_vec())
        } else {
            project
                .dependencies
                .as_ref()
                .and_then(|d| d.dependencies.get(&skill_def.id.to_string()))
                .and_then(|spec| match spec {
                    DependencySpec::Inline { groups, .. } => groups.clone(),
                    DependencySpec::Version(_) => None,
                })
        };

        // The persisted form of the origin. A local path is stored relative to
        // the Manifest's own directory so a committed skill-project.toml +
        // skills.lock names the same skill in every checkout; an out-of-tree
        // path cannot be made portable, so it stays absolute and is warned
        // about by name rather than written silently.
        let manifest_dir = project_file_path.parent().unwrap_or(Path::new("."));
        let (portable_origin, unportable) = skill_def.origin.to_manifest_relative(manifest_dir);
        let warnings: Vec<String> = unportable
            .iter()
            .map(|u| u.warning(skill_def.id.as_str()))
            .collect();

        if let Some(deps) = project.dependencies.as_mut() {
            deps.dependencies.insert(
                skill_def.id.to_string(),
                DependencySpec::Inline {
                    origin: portable_origin.clone(),
                    groups: effective_groups.clone(),
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
        // The Lock is committed next to the Manifest and is what `install --lock`
        // reads, so it records the same portable origin.
        let mut locked_def = skill_def.clone();
        locked_def.origin = portable_origin;
        lock.update_skill(&locked_def);
        // Mirror the manifest's groups onto the lock entry (update_skill does not
        // carry them from the manifest).
        if let Some(entry) = lock
            .skills
            .iter_mut()
            .find(|s| s.id == skill_def.id.as_str())
        {
            entry.groups = effective_groups.clone().unwrap_or_default();
        }
        lock.save_to_file(&lock_path)
            .map_err(|e| ServiceError::Config(format!("Failed to save skills.lock: {e}")))?;

        Ok(warnings)
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

                // PRD 006 (US-003, "Resolved Defaults": update implicitly
                // refreshes just the sources it touches). This listing call
                // already reached the network for the preflight decision below;
                // persist it into the on-disk index so a same-invocation
                // `add_from_origin(.., AddMode::Update, ..)` for a `newest`/range
                // origin resolves through the content-cache path (US-003)
                // instead of requiring a separate `repos refresh` the caller
                // never ran. Best-effort: a write failure must not fail the
                // preflight, which has already succeeded.
                if let Err(e) =
                    upsert_source_index_entry(self.skill_cache(), &repo_name, skill, &available)
                {
                    tracing::warn!("failed to update cached index for '{repo_name}': {}", e);
                }

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

mod support;

use support::*;
pub use support::{LOCAL_COPY_INVOCATIONS, ZIP_URL_BODY_BYTES_DOWNLOADED};

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests;
