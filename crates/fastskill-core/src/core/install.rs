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

        self.upsert_manifest_and_lock(&skill_def, &groups)?;

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
    fn upsert_manifest_and_lock(
        &self,
        skill_def: &SkillDefinition,
        groups: &[String],
    ) -> Result<(), ServiceError> {
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
        lock.update_skill(skill_def);
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

/// Resolve `(repo_name, skill, version)` to a concrete version string (PRD 006
/// "Local Skill Cache", US-003), without ever calling the registry's live
/// listing endpoint:
///
/// - An exact pin (bare `1.2.3`, or explicit `=1.2.3`) needs no listing at
///   all — it *is* the resolved version. This is what makes "a pinned, cached
///   version installs with no network" possible.
/// - `None` ("newest") or a range constraint (`^`, `~`, `>=`, ...) resolves
///   against the on-disk [`crate::core::cache::SourceIndex`] that `repos
///   refresh` populates (US-005) — never a live call. With no cached index
///   (or no matching entry/candidate), fails with an error naming `repos
///   refresh` rather than silently falling back to the network.
fn resolve_registry_version(
    cache: &SkillCache,
    repo_name: &str,
    skill: &str,
    version: Option<&VersionConstraint>,
) -> Result<String, ServiceError> {
    if let Some(exact) = version.and_then(VersionConstraint::as_exact) {
        return Ok(exact);
    }

    let idx = cache.read_source_index(repo_name)?.ok_or_else(|| {
        ServiceError::Config(format!(
            "no cached index for repository '{repo_name}'; run `fastskill repos refresh \
             {repo_name}` to resolve the newest version of '{skill}'"
        ))
    })?;
    let entry = idx
        .entries
        .iter()
        .find(|e| e.skill == skill)
        .ok_or_else(|| {
            ServiceError::Config(format!(
                "skill '{skill}' not found in the cached index for repository '{repo_name}'; run \
             `fastskill repos refresh {repo_name}` to refresh it"
            ))
        })?;
    let candidates: Vec<String> = match version {
        Some(constraint) => entry
            .versions
            .iter()
            .filter(|v| constraint.satisfies(v).unwrap_or(false))
            .cloned()
            .collect(),
        None => entry.versions.clone(),
    };
    newest_version(&candidates).ok_or_else(|| {
        ServiceError::Config(format!(
            "no version of '{skill}' in the cached index for repository '{repo_name}' satisfies \
             the requested constraint; run `fastskill repos refresh {repo_name}` to refresh it"
        ))
    })
}

/// Upsert a single skill's versions into the on-disk [`SourceIndex`] for
/// `repo_name`, leaving every other entry untouched (PRD 006 US-003,
/// "Resolved Defaults": `update` implicitly refreshes just the sources it
/// touches). Used by [`FastSkillService::preflight`]'s `Origin::Repository`
/// branch to persist a listing call it already made live, so a
/// same-invocation update can resolve through the index instead of needing a
/// separate `repos refresh`.
///
/// `client.get_versions` (this function's only caller) has no `name`/
/// `description` to offer, so an existing entry's `name`/`description`
/// (spec 008) are left as they were rather than clobbered with blanks, and a
/// newly-created entry gets empty strings until a `repos refresh` or a live
/// marketplace fetch fills them in.
fn upsert_source_index_entry(
    cache: &SkillCache,
    repo_name: &str,
    skill: &str,
    versions: &[String],
) -> Result<(), ServiceError> {
    let mut idx = cache
        .read_source_index(repo_name)?
        .unwrap_or_else(|| SourceIndex {
            fetched_at: chrono::Utc::now(),
            entries: Vec::new(),
        });
    idx.fetched_at = chrono::Utc::now();
    if let Some(entry) = idx.entries.iter_mut().find(|e| e.skill == skill) {
        entry.versions = versions.to_vec();
    } else {
        idx.entries.push(SourceIndexEntry {
            skill: skill.to_string(),
            versions: versions.to_vec(),
            name: String::new(),
            description: String::new(),
        });
    }
    cache.write_source_index(repo_name, &idx)
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

/// A stable string key for a [`GitRef`] within the git-resolutions index,
/// combined with `url` by [`crate::core::cache::GitResolutions`]. Not
/// `GitRef`'s `Display` (it has none). The `Default`/`Branch`/`Tag` arms
/// delegate to [`crate::core::cache::GitResolutions::branch_or_tag_key`] — the
/// single place that encoding lives — so this stays interchangeable with the
/// resolutions `repos refresh` records (PRD 006, US-005). `Commit` has no
/// branch/tag form and is encoded here, its only caller.
fn git_ref_cache_key(git_ref: &GitRef) -> String {
    match git_ref {
        GitRef::Default => crate::core::cache::GitResolutions::branch_or_tag_key(None, None),
        GitRef::Branch(b) => crate::core::cache::GitResolutions::branch_or_tag_key(Some(b), None),
        GitRef::Tag(t) => crate::core::cache::GitResolutions::branch_or_tag_key(None, Some(t)),
        GitRef::Commit(c) => format!("commit:{c}"),
    }
}

/// Resolve `url`+`ref_key` (via `branch`/`tag`) to a commit SHA (PRD 006, US-002).
///
/// Prefers a live [`crate::storage::git::ls_remote`] and records the result. If
/// that fails — offline, DNS down, etc. — and a previous resolution for the
/// same `url`+`ref_key` is recorded in the index cache, proceeds from it with a
/// warning instead of failing outright; with no prior resolution, propagates
/// the `ls_remote` error as-is (today's error).
async fn resolve_git_sha(
    cache: &SkillCache,
    url: &str,
    ref_key: &str,
    branch: Option<&str>,
    tag: Option<&str>,
) -> Result<String, ServiceError> {
    match crate::storage::git::ls_remote(url, branch, tag).await {
        Ok(sha) => {
            if let Err(e) = record_git_resolution(cache, url, ref_key, &sha) {
                tracing::warn!("failed to record git ref resolution: {}", e);
            }
            Ok(sha)
        }
        Err(err) => {
            let resolutions = cache.read_git_resolutions()?;
            match resolutions.get(url, ref_key) {
                Some(resolution) => {
                    tracing::warn!(
                        "could not resolve the latest commit for '{url}' ({err}); using the \
                         resolution cached on {resolved_at} instead: {sha}",
                        resolved_at = resolution.resolved_at,
                        sha = resolution.sha,
                    );
                    Ok(resolution.sha.clone())
                }
                None => Err(err),
            }
        }
    }
}

/// Record a successful `url`+`ref_key -> sha` resolution in the git-resolutions
/// index, so a later offline install of the same ref can fall back to it.
fn record_git_resolution(
    cache: &SkillCache,
    url: &str,
    ref_key: &str,
    sha: &str,
) -> Result<(), ServiceError> {
    let mut resolutions = cache.read_git_resolutions()?;
    resolutions.insert(url, ref_key, sha.to_string(), chrono::Utc::now());
    cache.write_git_resolutions(&resolutions)
}

// ── Local origin content cache (PRD 006 "Local Skill Cache", US-004) ──────────

/// Number of times a local-origin fetch actually copied from the original
/// source directory or extracted a `.zip` (rather than reusing the content
/// cache). Instrumentation-only, kept for the same reason as
/// `storage::git::CLONE_INVOCATIONS` / `registry::client::DOWNLOAD_INVOCATIONS`:
/// proving a cache hit skipped the expensive step is otherwise only
/// observable by timing. Deliberately not gated behind `#[cfg(test)]` —
/// integration tests are a separate compilation unit and cannot see items
/// scoped that way.
#[doc(hidden)] // test instrumentation, not supported public API
pub static LOCAL_COPY_INVOCATIONS: AtomicUsize = AtomicUsize::new(0);

/// Fetch a non-editable, non-zip local directory through the content cache
/// (US-004): hash the validated skill directory's tree, check the cache under
/// that identity, and either copy the cached content out or copy from the
/// source and `put` it for next time.
async fn fetch_local_dir(
    cache: &SkillCache,
    resolved_path: &Path,
    temp_dir: &Path,
) -> Result<PathBuf, ServiceError> {
    let validated_source = crate::storage::git::validate_cloned_skill(resolved_path)?;
    let identity = CacheIdentity::Local {
        tree_hash: compute_local_tree_hash(&validated_source)?,
    };

    if let Some(cached) = cache.get(&identity) {
        let dest = temp_dir.join("cached");
        copy_dir_recursive(&cached.path, &dest).await?;
        return crate::storage::git::validate_cloned_skill(&dest);
    }

    LOCAL_COPY_INVOCATIONS.fetch_add(1, Ordering::SeqCst);
    let dest = temp_dir.join("copied");
    copy_dir_recursive(&validated_source, &dest).await?;
    if let Err(e) = cache.put(&identity, &dest) {
        // The copy already succeeded and is usable; a cache-write failure
        // only costs this run the "install once per machine" win, so it must
        // not fail the install.
        tracing::warn!("failed to publish local source to content cache: {}", e);
    }
    Ok(dest)
}

/// Fetch a non-editable local `.zip` path through the content cache (US-004).
/// Per FR-6's sibling rule for this story, a `.zip` is cached by the bytes of
/// the archive itself, not a tree walk of its extracted contents — the
/// archive's own bytes are its identity.
async fn fetch_local_zip(
    cache: &SkillCache,
    resolved_path: &Path,
    temp_dir: &Path,
) -> Result<PathBuf, ServiceError> {
    let bytes = tokio::fs::read(resolved_path).await?;
    let identity = CacheIdentity::Local {
        tree_hash: hash_bytes(&bytes),
    };

    if let Some(cached) = cache.get(&identity) {
        let dest = temp_dir.join("cached");
        copy_dir_recursive(&cached.path, &dest).await?;
        return crate::storage::git::validate_cloned_skill(&dest);
    }

    LOCAL_COPY_INVOCATIONS.fetch_add(1, Ordering::SeqCst);
    let extract_path = temp_dir.join("extracted");
    tokio::fs::create_dir_all(&extract_path).await?;
    let zip_handler = crate::storage::zip::ZipHandler::new()?;
    zip_handler.extract_to_dir(resolved_path, &extract_path)?;
    let skill_path = crate::storage::git::validate_cloned_skill(&extract_path)?;
    if let Err(e) = cache.put(&identity, &skill_path) {
        tracing::warn!(
            "failed to publish local zip contents to content cache: {}",
            e
        );
    }
    Ok(skill_path)
}

// ── Zip-URL content cache (spec 007 "zip-url caching") ─────────────────────

/// Number of times [`fetch_zip_url_cached`] has actually read a full response
/// body over the network — incremented by the number of bytes read, never on
/// a `304 Not Modified` (which per HTTP carries no body). Instrumentation
/// only, kept for the same reason as `storage::git::CLONE_INVOCATIONS` /
/// `registry::client::DOWNLOAD_INVOCATIONS`: proving the 304 fast path really
/// downloaded nothing is otherwise only observable by timing. Deliberately
/// not gated behind `#[cfg(test)]` — integration tests are a separate
/// compilation unit and cannot see items scoped that way.
#[doc(hidden)] // test instrumentation, not supported public API
pub static ZIP_URL_BODY_BYTES_DOWNLOADED: AtomicUsize = AtomicUsize::new(0);

/// The outcome of one HTTP attempt against a zip URL, distinguishing "server
/// confirmed the bytes are unchanged" from "server sent (new) bytes" — the
/// two paths [`fetch_zip_url_cached`] needs to tell apart.
enum ZipFetchAttempt {
    /// `304 Not Modified`: no body was sent.
    NotModified,
    /// `200 OK` (or any other success status): a body was sent and read.
    Modified {
        bytes: Vec<u8>,
        etag: Option<String>,
        last_modified: Option<String>,
    },
}

/// Issue one GET against `url`, conditional on `validator` when given
/// (`If-None-Match`/`If-Modified-Since`). Any failure — connection refused,
/// DNS failure, timeout, or a non-2xx/304 status — is surfaced as a single
/// `ServiceError` so [`fetch_zip_url_cached`] can apply spec 007 FR-5's
/// offline fallback uniformly, the same way `resolve_git_sha` treats any
/// `ls_remote` failure as one case regardless of cause.
async fn zip_url_conditional_fetch(
    client: &reqwest::Client,
    url: &str,
    validator: Option<&ZipValidator>,
) -> Result<ZipFetchAttempt, ServiceError> {
    let mut request = client.get(url);
    if let Some(v) = validator {
        if let Some(etag) = &v.etag {
            request = request.header(reqwest::header::IF_NONE_MATCH, etag);
        }
        if let Some(last_modified) = &v.last_modified {
            request = request.header(reqwest::header::IF_MODIFIED_SINCE, last_modified);
        }
    }

    let response = request
        .send()
        .await
        .map_err(|e| ServiceError::InvalidOperation(format!("Failed to download '{url}': {e}")))?;

    if response.status() == reqwest::StatusCode::NOT_MODIFIED {
        if validator.is_none() {
            // A server returning 304 to a request that carried no validators
            // at all is a protocol violation on its part, not a state this
            // code can act on: there is nothing recorded to resolve it
            // against. Surface it as an error rather than guessing.
            return Err(ServiceError::InvalidOperation(format!(
                "'{url}' returned 304 Not Modified to an unconditional request (no validators \
                 were sent)"
            )));
        }
        return Ok(ZipFetchAttempt::NotModified);
    }

    let response = response
        .error_for_status()
        .map_err(|e| ServiceError::InvalidOperation(format!("Failed to download '{url}': {e}")))?;

    let etag = response
        .headers()
        .get(reqwest::header::ETAG)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let last_modified = response
        .headers()
        .get(reqwest::header::LAST_MODIFIED)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);

    let bytes = response
        .bytes()
        .await
        .map_err(|e| ServiceError::InvalidOperation(format!("Failed to read '{url}': {e}")))?
        .to_vec();
    ZIP_URL_BODY_BYTES_DOWNLOADED.fetch_add(bytes.len(), Ordering::SeqCst);

    Ok(ZipFetchAttempt::Modified {
        bytes,
        etag,
        last_modified,
    })
}

/// Extract freshly-downloaded archive `bytes`, publish them into the content
/// cache under their hash (FR-1: the archive bytes are the identity — same
/// input as `fetch_local_zip`'s local `.zip` handling), and record the new
/// validator (FR-2/FR-3). Dedups against the content cache first: identical
/// bytes served under a different URL (or by a server with no validators at
/// all) are stored once.
async fn store_downloaded_zip(
    cache: &SkillCache,
    url: &str,
    temp_dir: &Path,
    bytes: &[u8],
    etag: Option<String>,
    last_modified: Option<String>,
) -> Result<PathBuf, ServiceError> {
    let content_hash = hash_bytes(bytes);
    let identity = CacheIdentity::ZipUrl {
        content_hash: content_hash.clone(),
    };

    let skill_path = if let Some(cached) = cache.get(&identity) {
        let dest = temp_dir.join("cached");
        copy_dir_recursive(&cached.path, &dest).await?;
        crate::storage::git::validate_cloned_skill(&dest)?
    } else {
        let zip_path = temp_dir.join("package.zip");
        let extract_path = temp_dir.join("extracted");
        tokio::fs::write(&zip_path, bytes).await?;
        tokio::fs::create_dir_all(&extract_path).await?;

        let zip_handler = crate::storage::zip::ZipHandler::new()?;
        zip_handler.extract_to_dir(&zip_path, &extract_path)?;
        let skill_path = crate::storage::git::validate_cloned_skill(&extract_path)?;
        if let Err(e) = cache.put(&identity, &skill_path) {
            // The download already succeeded and is usable; a cache-write
            // failure only costs this run the "download once per machine"
            // win, so it must not fail the install.
            tracing::warn!("failed to publish zip-url content to content cache: {}", e);
        }
        skill_path
    };

    if let Err(e) = record_zip_validator(cache, url, &content_hash, etag, last_modified) {
        tracing::warn!("failed to record zip-url validator: {}", e);
    }

    Ok(skill_path)
}

/// Record (or replace) `url`'s validator in the zip-validators index (FR-2).
fn record_zip_validator(
    cache: &SkillCache,
    url: &str,
    content_hash: &str,
    etag: Option<String>,
    last_modified: Option<String>,
) -> Result<(), ServiceError> {
    let mut validators = cache.read_zip_validators()?;
    validators.insert(
        url,
        ZipValidator {
            etag,
            last_modified,
            content_hash: content_hash.to_string(),
            fetched_at: chrono::Utc::now(),
        },
    );
    cache.write_zip_validators(&validators)
}

/// Fetch a zip URL through the content cache (spec 007 FR-3): consult the
/// recorded validator, issue a conditional request, and follow whichever of
/// the spec's paths applies:
///
/// - **304** with the recorded hash still in the content cache (FR-3): serve
///   it straight from the cache — zero bytes of body downloaded.
/// - **304** with the recorded hash evicted, e.g. by `cache clean` (FR-4):
///   fall back to an unconditional download rather than failing.
/// - **200**: download, hash, store, and record the new validator (FR-3).
/// - **transport/status failure** (FR-5): if a recorded hash exists *and* is
///   still cached, proceed from it with a warning naming the recorded fetch
///   time — mirroring `resolve_git_sha`'s offline fallback. Otherwise
///   propagate the failure as-is.
async fn fetch_zip_url_cached(
    cache: &SkillCache,
    client: &reqwest::Client,
    url: &str,
    temp_dir: &Path,
) -> Result<PathBuf, ServiceError> {
    let validators = cache.read_zip_validators()?;
    let recorded = validators.get(url).cloned();

    match zip_url_conditional_fetch(client, url, recorded.as_ref()).await {
        Ok(ZipFetchAttempt::NotModified) => {
            // `zip_url_conditional_fetch` only returns `NotModified` when a
            // validator was actually sent, so `recorded` is present here.
            let Some(recorded) = recorded else {
                return Err(ServiceError::Custom(
                    "zip-url fetch reported 304 Not Modified with no recorded validator"
                        .to_string(),
                ));
            };
            if let Some(cached) = cache.get(&CacheIdentity::ZipUrl {
                content_hash: recorded.content_hash.clone(),
            }) {
                let dest = temp_dir.join("cached");
                copy_dir_recursive(&cached.path, &dest).await?;
                return crate::storage::git::validate_cloned_skill(&dest);
            }

            // FR-4: the server confirmed the bytes are unchanged, but this
            // machine no longer has them (e.g. `cache clean`). Fall back to
            // an unconditional download rather than failing.
            tracing::warn!(
                "cached content for '{url}' (hash {}) is no longer in the content cache; \
                 re-downloading",
                recorded.content_hash
            );
            match zip_url_conditional_fetch(client, url, None).await? {
                ZipFetchAttempt::Modified {
                    bytes,
                    etag,
                    last_modified,
                } => store_downloaded_zip(cache, url, temp_dir, &bytes, etag, last_modified).await,
                ZipFetchAttempt::NotModified => Err(ServiceError::Custom(format!(
                    "'{url}' returned 304 Not Modified to an unconditional re-download request"
                ))),
            }
        }
        Ok(ZipFetchAttempt::Modified {
            bytes,
            etag,
            last_modified,
        }) => store_downloaded_zip(cache, url, temp_dir, &bytes, etag, last_modified).await,
        Err(err) => {
            // FR-5: offline / transport failure. Proceed from a recorded,
            // still-cached hash with a warning; otherwise the failure stands.
            if let Some(recorded) = &recorded {
                if let Some(cached) = cache.get(&CacheIdentity::ZipUrl {
                    content_hash: recorded.content_hash.clone(),
                }) {
                    tracing::warn!(
                        "could not fetch '{url}' ({err}); using the content cached on \
                         {fetched_at} instead (hash {hash})",
                        fetched_at = recorded.fetched_at,
                        hash = recorded.content_hash,
                    );
                    let dest = temp_dir.join("cached");
                    copy_dir_recursive(&cached.path, &dest).await?;
                    return crate::storage::git::validate_cloned_skill(&dest);
                }
            }
            Err(err)
        }
    }
}

/// Compute a deterministic tree-hash of a validated skill directory (US-004):
/// every regular file's path (relative to `root`, with path separators
/// normalized to `/` so the hash is stable across platforms) and its content
/// bytes feed a SHA-256 digest, in a fixed (lexicographically sorted by
/// relative path) order so directory-read order never affects the result.
/// File mtimes and permissions are never read, so touching a file without
/// changing its content produces the same hash. Rejects symlinks (mirrors
/// `copy_dir_recursive`'s SEC-4 stance): a symlink inside the source tree
/// must not be silently dereferenced and hashed by its target's contents.
fn compute_local_tree_hash(root: &Path) -> Result<String, ServiceError> {
    let mut entries = collect_tree_entries(root, root)?;
    entries.sort_by(|a, b| a.0.cmp(&b.0));

    let mut hasher = Sha256::new();
    for (rel_path, content) in &entries {
        // Length-prefix each field so no concatenation of adjacent
        // path/content bytes can collide across different (path, content)
        // splits.
        hasher.update((rel_path.len() as u64).to_le_bytes());
        hasher.update(rel_path.as_bytes());
        hasher.update((content.len() as u64).to_le_bytes());
        hasher.update(content);
    }
    Ok(crate::utils::to_hex_lower(&hasher.finalize()))
}

/// Recursively collect `(relative_path, content)` pairs for every regular
/// file under `dir`, relative to `root` with `/`-normalized separators.
fn collect_tree_entries(dir: &Path, root: &Path) -> Result<Vec<(String, Vec<u8>)>, ServiceError> {
    let mut entries = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let path = entry.path();

        if file_type.is_symlink() {
            return Err(ServiceError::Validation(format!(
                "refusing to hash symlink: {}",
                path.display()
            )));
        } else if file_type.is_dir() {
            entries.extend(collect_tree_entries(&path, root)?);
        } else {
            let rel = path
                .strip_prefix(root)
                .map_err(|e| ServiceError::Custom(format!("path escaped its root: {e}")))?;
            let rel_str = rel
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/");
            let content = std::fs::read(&path)?;
            entries.push((rel_str, content));
        }
    }
    Ok(entries)
}

/// SHA-256 hex digest of raw bytes.
fn hash_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    crate::utils::to_hex_lower(&hasher.finalize())
}

/// Resolve `HEAD`'s commit SHA in a freshly-cloned git repository.
async fn git_head_commit(repo_dir: &Path) -> Result<String, ServiceError> {
    let mut cmd = tokio::process::Command::new("git");
    cmd.args(["rev-parse", "HEAD"]).current_dir(repo_dir);
    // `GIT_DIR` beats `current_dir`, so without this a fastskill run nested
    // inside another git invocation (a hook, `rebase --exec`) would report the
    // *enclosing* repo's HEAD as the commit we just cloned.
    crate::storage::git::scrub_inherited_git_env(&mut cmd);
    let output = cmd.output().await?;
    if !output.status.success() {
        return Err(ServiceError::Custom(format!(
            "git rev-parse HEAD failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Remove the top-level `.git` directory from a freshly-cloned repository
/// root, if present. Called only after [`git_head_commit`] has already read
/// it and only on the just-cloned root (never on a subdirectory), since
/// `.git` always lives at the clone root regardless of `subdir`. A no-op if
/// there is nothing at `.git` (e.g. called twice, or on content that was
/// already stripped). Never follows a symlink for the removal (defense in
/// depth, mirroring `copy_dir_recursive`'s SEC-4 stance elsewhere in this
/// file) — a `.git` symlink is unlinked directly rather than dereferenced.
async fn strip_git_dir(repo_root: &Path) -> Result<(), ServiceError> {
    let git_dir = repo_root.join(".git");
    let Ok(metadata) = tokio::fs::symlink_metadata(&git_dir).await else {
        // Nothing there: already stripped, or never existed. Not an error.
        return Ok(());
    };
    if metadata.is_dir() {
        tokio::fs::remove_dir_all(&git_dir).await?;
    } else {
        // A symlink or a regular file (e.g. a `.git` gitlink file, as used
        // by worktrees/submodules): unlink it directly either way, never
        // following it.
        tokio::fs::remove_file(&git_dir).await?;
    }
    Ok(())
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
            .add_from_origin(origin, AddMode::Fresh, vec![])
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
    async fn test_add_from_origin_records_and_preserves_groups() {
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

        // Fresh add with an explicit group records it on manifest + lock.
        service
            .add_from_origin(origin.clone(), AddMode::Fresh, vec!["dev".to_string()])
            .await
            .expect("fresh add should succeed");

        let groups_in_manifest = || {
            let project =
                SkillProjectToml::load_from_file(&tmp.path().join("skill-project.toml")).unwrap();
            match project
                .dependencies
                .unwrap()
                .dependencies
                .remove("test-skill")
            {
                Some(DependencySpec::Inline { groups, .. }) => groups,
                _ => None,
            }
        };
        let lock_groups = || {
            let lock = ProjectSkillsLock::load_from_file(&tmp.path().join("skills.lock")).unwrap();
            lock.skills[0].groups.clone()
        };
        assert_eq!(groups_in_manifest(), Some(vec!["dev".to_string()]));
        assert_eq!(lock_groups(), vec!["dev".to_string()]);

        // Update with an empty groups list must PRESERVE the existing group.
        service
            .add_from_origin(origin, AddMode::Update, vec![])
            .await
            .expect("update should succeed");
        assert_eq!(
            groups_in_manifest(),
            Some(vec!["dev".to_string()]),
            "update with empty groups must preserve existing groups"
        );
        assert_eq!(lock_groups(), vec!["dev".to_string()]);
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
            .add_from_origin(origin.clone(), AddMode::Fresh, vec![])
            .await
            .expect("first add should succeed");

        let result = service
            .add_from_origin(origin, AddMode::Fresh, vec![])
            .await;
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
            .add_from_origin(origin.clone(), AddMode::Fresh, vec![])
            .await
            .expect("first add should succeed");

        // Update the source content, then re-add via Update mode.
        std::fs::write(
            src.join("SKILL.md"),
            "---\nname: test-skill\nversion: \"2.0.0\"\ndescription: updated\n---\nBody\n",
        )
        .unwrap();

        let outcome = service
            .add_from_origin(origin, AddMode::Update, vec![])
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
            .add_from_origin(origin, AddMode::Fresh, vec![])
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
        let result = service
            .add_from_origin(origin, AddMode::Fresh, vec![])
            .await;
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
            .add_from_origin(origin, AddMode::Fresh, vec![])
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
        let result = service
            .add_from_origin(origin, AddMode::Fresh, vec![])
            .await;
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
        let result = service
            .add_from_origin(origin, AddMode::Fresh, vec![])
            .await;
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

    // ── strip_git_dir (cache-bloat bugfix) ────────────────────────────────────

    #[tokio::test]
    async fn test_strip_git_dir_removes_a_directory_git() {
        let tmp = TestTempDir::new().unwrap();
        let root = tmp.path().join("clone");
        std::fs::create_dir_all(root.join(".git/objects")).unwrap();
        std::fs::write(root.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
        std::fs::write(root.join("SKILL.md"), "# skill\n").unwrap();

        strip_git_dir(&root).await.unwrap();

        assert!(!root.join(".git").exists(), ".git must be removed");
        assert!(
            root.join("SKILL.md").exists(),
            "sibling skill content must be untouched"
        );
    }

    #[tokio::test]
    async fn test_strip_git_dir_is_a_noop_when_absent() {
        let tmp = TestTempDir::new().unwrap();
        let root = tmp.path().join("clone");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("SKILL.md"), "# skill\n").unwrap();

        // Must not error just because there is nothing to strip.
        strip_git_dir(&root).await.unwrap();
        assert!(root.join("SKILL.md").exists());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_strip_git_dir_unlinks_without_following_a_symlinked_git() {
        use std::os::unix::fs::symlink;
        let tmp = TestTempDir::new().unwrap();
        let root = tmp.path().join("clone");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("SKILL.md"), "# skill\n").unwrap();

        // A directory outside `root` that a symlinked `.git` could otherwise
        // redirect a recursive removal into; it must survive untouched.
        let outside = tmp.path().join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("sentinel.txt"), "do not delete me").unwrap();
        symlink(&outside, root.join(".git")).unwrap();

        strip_git_dir(&root).await.unwrap();

        assert!(
            !root.join(".git").exists(),
            "the symlink entry itself must be gone"
        );
        assert!(
            outside.join("sentinel.txt").is_file(),
            "must never follow the symlink to delete its target"
        );
    }

    // ── compute_local_tree_hash / hash_bytes (US-004) ─────────────────────────

    #[test]
    fn test_tree_hash_stable_across_mtime_touch() {
        let tmp = TestTempDir::new().unwrap();
        let src = tmp.path().join("src");
        std::fs::create_dir_all(src.join("nested")).unwrap();
        std::fs::write(src.join("SKILL.md"), "---\nname: x\n---\n").unwrap();
        std::fs::write(src.join("nested/file.txt"), "data").unwrap();

        let before = compute_local_tree_hash(&src).unwrap();

        // Touch the file's mtime only, content untouched. Open for *write*:
        // Windows refuses to set a file's modified time through a read-only
        // handle ("Access is denied"), while unix is happy either way.
        let file = std::fs::OpenOptions::new()
            .write(true)
            .open(src.join("nested/file.txt"))
            .unwrap();
        let new_time = std::time::SystemTime::now() + std::time::Duration::from_secs(3600);
        file.set_modified(new_time).unwrap();

        let after = compute_local_tree_hash(&src).unwrap();
        assert_eq!(
            before, after,
            "touching a file's mtime alone must not change the tree-hash"
        );
    }

    #[test]
    fn test_tree_hash_changes_when_content_changes() {
        let tmp = TestTempDir::new().unwrap();
        let src = tmp.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("SKILL.md"), "---\nname: x\n---\nv1\n").unwrap();

        let before = compute_local_tree_hash(&src).unwrap();

        std::fs::write(src.join("SKILL.md"), "---\nname: x\n---\nv2\n").unwrap();
        let after = compute_local_tree_hash(&src).unwrap();

        assert_ne!(
            before, after,
            "changing a file's content must change the tree-hash"
        );
    }

    #[test]
    fn test_tree_hash_independent_of_directory_read_order() {
        let tmp = TestTempDir::new().unwrap();

        // Same relative paths/contents, written in a different order.
        let a = tmp.path().join("a");
        std::fs::create_dir_all(a.join("nested")).unwrap();
        std::fs::write(a.join("SKILL.md"), "one").unwrap();
        std::fs::write(a.join("nested/file.txt"), "two").unwrap();

        let b = tmp.path().join("b");
        std::fs::create_dir_all(b.join("nested")).unwrap();
        std::fs::write(b.join("nested/file.txt"), "two").unwrap();
        std::fs::write(b.join("SKILL.md"), "one").unwrap();

        assert_eq!(
            compute_local_tree_hash(&a).unwrap(),
            compute_local_tree_hash(&b).unwrap(),
            "identical (path, content) pairs must hash the same regardless of write/read order"
        );
    }

    #[test]
    fn test_tree_hash_sensitive_to_path_not_just_content() {
        let tmp = TestTempDir::new().unwrap();

        let a = tmp.path().join("a");
        std::fs::create_dir_all(&a).unwrap();
        std::fs::write(a.join("one.txt"), "same").unwrap();

        let b = tmp.path().join("b");
        std::fs::create_dir_all(&b).unwrap();
        std::fs::write(b.join("two.txt"), "same").unwrap();

        assert_ne!(
            compute_local_tree_hash(&a).unwrap(),
            compute_local_tree_hash(&b).unwrap(),
            "renaming a file must change the tree-hash even with identical content"
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_tree_hash_rejects_symlink() {
        use std::os::unix::fs::symlink;
        let tmp = TestTempDir::new().unwrap();
        let src = tmp.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("SKILL.md"), "# skill\n").unwrap();
        let secret = tmp.path().join("secret.txt");
        std::fs::write(&secret, "TOP SECRET").unwrap();
        symlink(&secret, src.join("creds")).unwrap();

        let result = compute_local_tree_hash(&src);
        assert!(matches!(result, Err(ServiceError::Validation(_))));
    }

    #[test]
    fn test_hash_bytes_is_deterministic_and_content_sensitive() {
        let h1 = hash_bytes(b"hello");
        let h2 = hash_bytes(b"hello");
        let h3 = hash_bytes(b"world");
        assert_eq!(h1, h2);
        assert_ne!(h1, h3);
    }

    // ── fetch_local: content-cache hit/miss (US-004) ──────────────────────────

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn test_fetch_local_dir_second_install_hits_cache_not_source() {
        let _lock = crate::test_utils::DIR_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let (tmp, _guard, skills_dir) = setup_project();
        let src = write_valid_skill(tmp.path(), "src-skill");
        let cache_root = TestTempDir::new().unwrap();
        let config = ServiceConfig {
            skill_storage_path: skills_dir.clone(),
            skill_cache_root: Some(cache_root.path().to_path_buf()),
            ..Default::default()
        };
        let mut service = FastSkillService::new(config).await.unwrap();
        service.initialize().await.unwrap();

        let origin = Origin::Local {
            path: src.clone(),
            editable: false,
        };

        let baseline = LOCAL_COPY_INVOCATIONS.load(Ordering::SeqCst);
        service
            .add_from_origin(origin.clone(), AddMode::Fresh, vec![])
            .await
            .expect("first add should succeed (cache miss)");
        assert_eq!(
            LOCAL_COPY_INVOCATIONS.load(Ordering::SeqCst) - baseline,
            1,
            "first install must copy from the source (cache miss)"
        );

        // Mutate the *original* source path so a re-copy would be observable
        // as different content -- proving a hit never touches it again.
        std::fs::write(
            src.join("SKILL.md"),
            "---\nname: test-skill\nversion: \"1.0.0\"\ndescription: A test skill\n---\nMUTATED\n",
        )
        .unwrap();

        // Re-install (Update) from a *content-identical-to-the-original*
        // copy of the source at a different path, so it resolves to the
        // same tree-hash identity as the first install without ever
        // re-reading the (now mutated) original.
        let src2 = write_valid_skill(tmp.path(), "src-skill-2");
        let origin2 = Origin::Local {
            path: src2,
            editable: false,
        };
        let outcome = service
            .add_from_origin(origin2, AddMode::Update, vec![])
            .await
            .expect("second add should succeed (cache hit)");
        assert_eq!(
            LOCAL_COPY_INVOCATIONS.load(Ordering::SeqCst) - baseline,
            1,
            "second install of the same identity must hit the cache, not copy again"
        );

        let installed =
            std::fs::read_to_string(skills_dir.join(&outcome.id).join("SKILL.md")).unwrap();
        assert_eq!(
            installed, VALID_SKILL_MD,
            "cache hit must be byte-identical (FR-8)"
        );
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn test_fetch_local_editable_bypasses_cache() {
        let _lock = crate::test_utils::DIR_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let (tmp, _guard, skills_dir) = setup_project();
        let src = write_valid_skill(tmp.path(), "src-skill");
        let cache_root = TestTempDir::new().unwrap();
        let config = ServiceConfig {
            skill_storage_path: skills_dir.clone(),
            skill_cache_root: Some(cache_root.path().to_path_buf()),
            ..Default::default()
        };
        let mut service = FastSkillService::new(config).await.unwrap();
        service.initialize().await.unwrap();

        let origin = Origin::Local {
            path: src.clone(),
            editable: true,
        };
        service
            .add_from_origin(origin, AddMode::Fresh, vec![])
            .await
            .expect("editable add should succeed");

        // FR-7: nothing gets published to the content cache for an editable install.
        let identity = CacheIdentity::Local {
            tree_hash: compute_local_tree_hash(&src).unwrap(),
        };
        assert!(
            service.skill_cache().get(&identity).is_none(),
            "editable install must bypass the content cache entirely"
        );
    }

    // ── fetch_local: `.zip` content-cache (US-004) ─────────────────────────────

    fn build_local_skill_zip(compression: zip::CompressionMethod) -> Vec<u8> {
        use std::io::Write;
        use zip::write::FileOptions;
        let mut buf = Vec::new();
        {
            let cursor = std::io::Cursor::new(&mut buf);
            let mut writer = zip::ZipWriter::new(cursor);
            let opts = FileOptions::default().compression_method(compression);
            writer.start_file("test-skill/SKILL.md", opts).unwrap();
            writer.write_all(VALID_SKILL_MD.as_bytes()).unwrap();
            writer.finish().unwrap();
        }
        buf
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn test_fetch_local_zip_second_install_hits_cache_not_source() {
        let _lock = crate::test_utils::DIR_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let (tmp, _guard, skills_dir) = setup_project();
        let cache_root = TestTempDir::new().unwrap();
        let config = ServiceConfig {
            skill_storage_path: skills_dir.clone(),
            skill_cache_root: Some(cache_root.path().to_path_buf()),
            ..Default::default()
        };
        let mut service = FastSkillService::new(config).await.unwrap();
        service.initialize().await.unwrap();

        let zip_bytes = build_local_skill_zip(zip::CompressionMethod::Stored);
        let zip_a = tmp.path().join("a.zip");
        std::fs::write(&zip_a, &zip_bytes).unwrap();

        let baseline = LOCAL_COPY_INVOCATIONS.load(Ordering::SeqCst);
        service
            .add_from_origin(
                Origin::Local {
                    path: zip_a,
                    editable: false,
                },
                AddMode::Fresh,
                vec![],
            )
            .await
            .expect("first zip add should succeed (cache miss)");
        assert_eq!(
            LOCAL_COPY_INVOCATIONS.load(Ordering::SeqCst) - baseline,
            1,
            "first install must extract the zip (cache miss)"
        );

        // Byte-identical zip at a different path -- same archive identity.
        let zip_b = tmp.path().join("b.zip");
        std::fs::write(&zip_b, &zip_bytes).unwrap();
        service
            .add_from_origin(
                Origin::Local {
                    path: zip_b,
                    editable: false,
                },
                AddMode::Update,
                vec![],
            )
            .await
            .expect("second zip add should succeed (cache hit)");
        assert_eq!(
            LOCAL_COPY_INVOCATIONS.load(Ordering::SeqCst) - baseline,
            1,
            "second install of a byte-identical zip must hit the cache, not re-extract"
        );
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn test_local_zip_identity_hashes_archive_bytes_not_extracted_tree() {
        let _lock = crate::test_utils::DIR_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let (tmp, _guard, skills_dir) = setup_project();
        let cache_root = TestTempDir::new().unwrap();
        let config = ServiceConfig {
            skill_storage_path: skills_dir.clone(),
            skill_cache_root: Some(cache_root.path().to_path_buf()),
            ..Default::default()
        };
        let mut service = FastSkillService::new(config).await.unwrap();
        service.initialize().await.unwrap();

        // Two archives that extract to byte-identical content, but whose own
        // bytes differ (different compression method). If identity were based
        // on the extracted tree, the second install would be a cache hit; per
        // FR (US-004), a `.zip` hashes the archive's own bytes, so it must not
        // be.
        let stored = tmp.path().join("stored.zip");
        std::fs::write(
            &stored,
            build_local_skill_zip(zip::CompressionMethod::Stored),
        )
        .unwrap();
        let deflated = tmp.path().join("deflated.zip");
        std::fs::write(
            &deflated,
            build_local_skill_zip(zip::CompressionMethod::Deflated),
        )
        .unwrap();
        assert_ne!(
            std::fs::read(&stored).unwrap(),
            std::fs::read(&deflated).unwrap(),
            "test fixture sanity: the two archives must differ at the byte level"
        );

        let baseline = LOCAL_COPY_INVOCATIONS.load(Ordering::SeqCst);
        service
            .add_from_origin(
                Origin::Local {
                    path: stored,
                    editable: false,
                },
                AddMode::Fresh,
                vec![],
            )
            .await
            .expect("stored-compression zip add should succeed");
        service
            .add_from_origin(
                Origin::Local {
                    path: deflated,
                    editable: false,
                },
                AddMode::Update,
                vec![],
            )
            .await
            .expect("deflated-compression zip add should succeed");

        assert_eq!(
            LOCAL_COPY_INVOCATIONS.load(Ordering::SeqCst) - baseline,
            2,
            "differently-encoded archives with identical extracted content must be \
             distinct cache identities (archive bytes, not the extracted tree)"
        );
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
