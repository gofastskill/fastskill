use super::*;

// ── Free helper functions ─────────────────────────────────────────────────────

/// Resolve a `repo` name (the `Origin::Repository.repo` field) against a
/// [`RepositoryManager`]: `"default"` resolves to the configured default
/// repository's name, anything else is used verbatim.
pub(super) fn resolve_repo_name(
    repo_manager: &RepositoryManager,
    repo: &str,
) -> Result<String, ServiceError> {
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
pub(super) fn resolve_registry_version(
    cache: &SkillCache,
    repo_name: &str,
    skill: &str,
    version: Option<&VersionConstraint>,
) -> Result<String, ServiceError> {
    if let Some(exact) = version.and_then(VersionConstraint::as_exact) {
        return validate_resolved_version(exact);
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
    let newest = newest_version(&candidates).ok_or_else(|| {
        ServiceError::Config(format!(
            "no version of '{skill}' in the cached index for repository '{repo_name}' satisfies \
             the requested constraint; run `fastskill repos refresh {repo_name}` to refresh it"
        ))
    })?;
    validate_resolved_version(newest)
}

/// Enforce that a resolved version is usable as a single path component.
///
/// The resolved version is interpolated into filesystem paths -- the
/// `package-{version}.zip` staging file in the registry install path -- and
/// into a [`CacheIdentity`], whose own `relative_path` is componentwise
/// validated. For a `newest`/range resolution the value originates in the
/// registry's listing response, by way of the on-disk source index, so it is
/// remote-controlled: [`newest_version`] ranks unparseable versions lowest but
/// still returns one when *every* candidate is unparseable, and nothing else
/// upstream constrains the string. Check it here, where both resolution paths
/// converge, instead of at each downstream use.
fn validate_resolved_version(version: String) -> Result<String, ServiceError> {
    crate::security::path::validate_path_component(&version).map_err(|e| {
        ServiceError::Validation(format!(
            "repository advertised an unusable version '{version}': {e}"
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
pub(super) fn upsert_source_index_entry(
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
pub(super) async fn read_skill_frontmatter(
    skill_path: &Path,
) -> Result<SkillFrontmatter, ServiceError> {
    let content = tokio::fs::read_to_string(skill_path.join("SKILL.md")).await?;
    parse_yaml_frontmatter(&content)
}

/// Derive `(SkillId, version)` from a fetched skill directory: `skill-project.toml`
/// `[metadata]` wins when present, else `SKILL.md` frontmatter (`metadata.id`/
/// `.version` sub-map, else `name`/top-level `version`, else `"1.0.0"`). Mirrors
/// `fastskill-cli`'s `create_skill_from_path` precedence.
pub(super) fn derive_skill_id_and_version(
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
pub(super) fn safe_subdir_join(root: &Path, subdir: &Path) -> Result<PathBuf, ServiceError> {
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
pub(super) fn git_ref_cache_key(git_ref: &GitRef) -> String {
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
pub(super) async fn resolve_git_sha(
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
pub(super) fn record_git_resolution(
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
pub(super) async fn fetch_local_dir(
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
pub(super) async fn fetch_local_zip(
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
pub(super) enum ZipFetchAttempt {
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
pub(super) async fn zip_url_conditional_fetch(
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
pub(super) async fn store_downloaded_zip(
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
pub(super) fn record_zip_validator(
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
pub(super) async fn fetch_zip_url_cached(
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
pub(super) fn compute_local_tree_hash(root: &Path) -> Result<String, ServiceError> {
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
pub(super) fn collect_tree_entries(
    dir: &Path,
    root: &Path,
) -> Result<Vec<(String, Vec<u8>)>, ServiceError> {
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
pub(super) fn hash_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    crate::utils::to_hex_lower(&hasher.finalize())
}

/// Resolve `HEAD`'s commit SHA in a freshly-cloned git repository.
pub(super) async fn git_head_commit(repo_dir: &Path) -> Result<String, ServiceError> {
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
pub(super) async fn strip_git_dir(repo_root: &Path) -> Result<(), ServiceError> {
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
pub(super) async fn remove_existing_storage_path(path: &Path) -> Result<(), ServiceError> {
    if path.is_symlink() || path.is_file() {
        tokio::fs::remove_file(path).await?;
    } else if path.exists() {
        tokio::fs::remove_dir_all(path).await?;
    }
    Ok(())
}

/// Move `skill_path` into `storage_dir` (same-filesystem rename when possible,
/// falling back to a recursive copy across filesystems/temp-dir boundaries).
pub(super) async fn move_or_copy_into_storage(
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
pub(super) async fn symlink_into_storage(
    skill_path: &Path,
    storage_dir: &Path,
) -> Result<(), ServiceError> {
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
pub(super) async fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), ServiceError> {
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
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn cache_with_versions(root: &TempDir, versions: &[&str]) -> SkillCache {
        let cache = SkillCache::at_root(root.path());
        cache
            .write_source_index(
                "acme",
                &SourceIndex {
                    fetched_at: chrono::Utc::now(),
                    entries: vec![SourceIndexEntry {
                        skill: "widget".to_string(),
                        versions: versions.iter().map(|v| (*v).to_string()).collect(),
                        name: String::new(),
                        description: String::new(),
                    }],
                },
            )
            .unwrap();
        cache
    }

    /// A registry's listing response reaches this function through the on-disk
    /// source index, so the version strings in it are remote-controlled.
    /// `newest_version` ranks unparseable versions lowest but still returns one
    /// when *every* candidate is unparseable, so a traversal string can be the
    /// selected version -- and the selection is then interpolated into
    /// filesystem paths (`install.rs`'s `package-{version}.zip`) and into a
    /// `CacheIdentity`. Reject it here, at the single resolution choke point,
    /// rather than at each downstream use.
    #[test]
    fn a_traversal_version_from_the_cached_index_is_rejected() {
        let root = TempDir::new().unwrap();
        let cache = cache_with_versions(&root, &["../../../../etc/cron.d/pwned"]);

        let err = resolve_registry_version(&cache, "acme", "widget", None).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("../../../../etc/cron.d/pwned"),
            "the error should name the version it rejected, got: {msg}"
        );
    }

    /// An absolute version string is the other shape that escapes a `join`:
    /// `Path::join` *replaces* the root when its argument is absolute.
    #[test]
    fn an_absolute_version_from_the_cached_index_is_rejected() {
        let root = TempDir::new().unwrap();
        let cache = cache_with_versions(&root, &["/etc/cron.d/pwned"]);

        assert!(resolve_registry_version(&cache, "acme", "widget", None).is_err());
    }

    /// The guard must not cost ordinary resolution: a normal semver set still
    /// resolves, and still resolves by semver order rather than lexically.
    #[test]
    fn ordinary_semver_versions_still_resolve_newest_first() {
        let root = TempDir::new().unwrap();
        let cache = cache_with_versions(&root, &["1.2.3", "1.9.0", "1.10.0"]);

        let v = resolve_registry_version(&cache, "acme", "widget", None).unwrap();
        assert_eq!(v, "1.10.0");
    }

    /// Pre-release and build metadata are legal semver and contain characters
    /// (`-`, `+`, `.`) a component validator could over-reject.
    #[test]
    fn a_prerelease_version_is_not_rejected_by_the_guard() {
        let root = TempDir::new().unwrap();
        let cache = cache_with_versions(&root, &["1.0.0-rc.1"]);

        let v = resolve_registry_version(&cache, "acme", "widget", None).unwrap();
        assert_eq!(v, "1.0.0-rc.1");
    }
}
