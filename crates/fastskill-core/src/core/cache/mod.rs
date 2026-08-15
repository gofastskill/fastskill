//! Local skill cache (PRD 006 "Local Skill Cache", US-001; see RFQ 004 for
//! the architecture rationale).
//!
//! A machine-global, content-addressed store for fetched skill content, plus
//! the per-source index it is checked against. [`SkillCache`] is the single
//! seam later stories (US-002 git, US-003 registry, US-004 local, US-005
//! `repos refresh`) go through to read or write the on-disk cache — this
//! story has **no production callers yet**; it only builds and tests the
//! store itself.
//!
//! ## On-disk layout
//!
//! ```text
//! <cache-root>/
//!   git/<sha>/                             content, keyed by commit SHA
//!   registry/<source>/<skill>/<version>/   content, keyed by pinned version
//!   local/<tree-hash>/                     content, keyed by tree/archive hash
//!   index/<source>.json                    what a source currently advertises
//!   index/git-resolutions.json             url+ref -> resolved sha (see `index`)
//!   tmp/                                   private staging area for atomic writes
//!   CACHEDIR.TAG                           https://bford.info/cachedir/ marker
//! ```
//!
//! `fastskill cache info`/`clean` (US-006) are the read/delete side of this
//! store: [`SkillCache::stats`] reports entry counts and disk usage per
//! [`ContentSourceKind`]; [`SkillCache::clean`] removes content entries
//! (never `index/`) with the same crash-safety posture as `put` and several
//! independent checks against deleting outside the cache root — see
//! `clean`'s own docs for the adversarial argument.
//!
//! `<cache-root>` defaults to the platform cache dir (`dirs::cache_dir()`,
//! e.g. `~/.cache/fastskill` on Linux) and is overridable via the
//! `FASTSKILL_CACHE_DIR` env var (FR-1).
//!
//! ## Crash- and concurrency-safety
//!
//! Every `put` assembles the new content in a private, uniquely-named
//! staging directory under `tmp/`, then publishes it with a single atomic
//! rename into its final, identity-keyed path. [`SkillCache::get`] only ever
//! looks at that final path, so:
//!
//! - a reader can never observe a partially-written entry (a crash between
//!   "assembled in staging" and "renamed into place" just leaves inert,
//!   never-looked-at bytes under `tmp/`);
//! - a concurrent duplicate `put` of the same identity degrades to a
//!   harmless no-op: whichever writer's rename lands first wins, and the
//!   loser simply observes the identity is now published and returns it.
//!
//! On Windows, a rename can transiently fail with `PermissionDenied` if
//! another process has briefly opened the source or target (e.g. an AV
//! scanner) — `publish` retries that case a bounded number of times before
//! giving up, per the PRD's "Resolved Defaults".

pub mod index;

pub use index::{GitResolution, GitResolutions, SourceIndex, SourceIndexEntry};

use crate::core::service::ServiceError;
use serde::Serialize;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Environment variable that overrides the cache root (FR-1).
pub const FASTSKILL_CACHE_DIR_ENV: &str = "FASTSKILL_CACHE_DIR";

/// Directory (under the cache root) new content is assembled in before
/// being published via atomic rename.
const STAGING_DIR_NAME: &str = "tmp";

/// Prefix for staging directories, so any leftover from an interrupted
/// process is trivially recognizable as cache-internal scratch space.
const STAGING_DIR_PREFIX: &str = ".fastskill-cache-tmp-";

/// [`SkillCache::put`] best-effort marks the cache root with a
/// [CACHEDIR.TAG](https://bford.info/cachedir/) — the same convention backup
/// tools and `du`-style utilities already recognize — so a cache root is
/// self-identifying even before `fastskill cache clean` (PRD 006, US-006)
/// needs to reason about whether a directory "looks like" a fastskill cache.
const CACHE_TAG_FILE_NAME: &str = "CACHEDIR.TAG";
const CACHE_TAG_CONTENTS: &[u8] = b"Signature: 8a477f597d28d172789f06886806bc55\n\
# This file is a cache directory tag created by fastskill.\n\
# For information about cache directory tags, see https://bford.info/cachedir/\n";

/// Top-level entries `fastskill cache clean` (PRD 006, US-006) will accept
/// finding directly under a resolved cache root before it will touch
/// anything inside it. Anything else there means the directory does not
/// look like a fastskill cache root — most plausibly `FASTSKILL_CACHE_DIR`
/// pointing somewhere unintended (a home directory, a project checkout) —
/// and `clean` refuses outright rather than guessing. See
/// [`verify_looks_like_cache_root`].
const CACHE_ROOT_ALLOWED_ENTRIES: &[&str] = &[
    "git",
    "registry",
    "local",
    "index",
    STAGING_DIR_NAME,
    CACHE_TAG_FILE_NAME,
];

/// Bounded retry count for the Windows rename-over-a-briefly-open-target
/// dance (PRD 006, "Resolved Defaults: Windows atomicity").
#[cfg(windows)]
const WINDOWS_RENAME_MAX_RETRIES: u32 = 10;
#[cfg(windows)]
const WINDOWS_RENAME_RETRY_DELAY: std::time::Duration = std::time::Duration::from_millis(50);

/// The immutable identity a piece of skill content is cached under. Modeled
/// as a sum type — not a formatted string — so an unrepresentable identity
/// (e.g. a git entry with no SHA) cannot be constructed, and each source
/// type carries exactly the fields it needs.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CacheIdentity {
    /// `git/<sha>/` — a resolved git commit (US-002).
    Git { sha: String },
    /// `registry/<source>/<skill>/<version>/` — a pinned registry artifact (US-003).
    Registry {
        source: String,
        skill: String,
        version: String,
    },
    /// `local/<tree-hash>/` — a local directory or `.zip`, content-hashed (US-004).
    Local { tree_hash: String },
}

impl CacheIdentity {
    /// This identity's path, relative to the cache root. Every component is
    /// validated so a malformed SHA/source/skill/version/hash can never
    /// escape its identity subtree (path traversal).
    fn relative_path(&self) -> Result<PathBuf, ServiceError> {
        match self {
            CacheIdentity::Git { sha } => Ok(PathBuf::from("git").join(validate_component(sha)?)),
            CacheIdentity::Registry {
                source,
                skill,
                version,
            } => Ok(PathBuf::from("registry")
                .join(validate_component(source)?)
                .join(validate_component(skill)?)
                .join(validate_component(version)?)),
            CacheIdentity::Local { tree_hash } => {
                Ok(PathBuf::from("local").join(validate_component(tree_hash)?))
            }
        }
    }
}

/// Validate a single identity path component: non-empty, no separators, no
/// `..`. Reuses the same seam every other user-controlled path component in
/// this codebase goes through (`crate::security::path`).
pub(crate) fn validate_component(component: &str) -> Result<&str, ServiceError> {
    if component.is_empty() {
        return Err(ServiceError::Validation(
            "cache identity component must not be empty".to_string(),
        ));
    }
    crate::security::path::validate_path_component(component)
        .map_err(|e| ServiceError::Validation(format!("invalid cache identity component: {e}")))?;
    Ok(component)
}

/// A cache hit: the on-disk directory holding cached skill content for some
/// [`CacheIdentity`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CachedContent {
    pub identity: CacheIdentity,
    /// Absolute path to the published content directory. Always the final,
    /// identity-keyed path — never a path inside the cache's own staging
    /// area — so callers can copy out of it immediately.
    pub path: PathBuf,
}

/// The three top-level content kinds a [`CacheIdentity`] belongs to — also
/// its on-disk directory name directly under the cache root (`git`,
/// `registry`, `local`; see the module docs' on-disk layout). `fastskill
/// cache info`/`clean` (PRD 006, US-006) report and scope by this rather
/// than a free-form string, so an invalid `--source` can never be turned
/// into a path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ContentSourceKind {
    Git,
    Registry,
    Local,
}

impl ContentSourceKind {
    /// All three kinds, in the order `fastskill cache info` reports them.
    pub const ALL: [ContentSourceKind; 3] = [
        ContentSourceKind::Git,
        ContentSourceKind::Registry,
        ContentSourceKind::Local,
    ];

    /// This kind's directory name directly under the cache root.
    pub fn dir_name(self) -> &'static str {
        match self {
            ContentSourceKind::Git => "git",
            ContentSourceKind::Registry => "registry",
            ContentSourceKind::Local => "local",
        }
    }

    /// Depth (in directories) from `<cache-root>/<dir_name>/` down to a leaf
    /// identity directory — the unit `stats`/`clean` count and delete as one
    /// entry. `git/<sha>/` and `local/<tree-hash>/` are one level deep;
    /// `registry/<source>/<skill>/<version>/` is three.
    fn leaf_depth(self) -> u32 {
        match self {
            ContentSourceKind::Git | ContentSourceKind::Local => 1,
            ContentSourceKind::Registry => 3,
        }
    }
}

impl std::fmt::Display for ContentSourceKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.dir_name())
    }
}

impl std::str::FromStr for ContentSourceKind {
    type Err = ServiceError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "git" => Ok(ContentSourceKind::Git),
            "registry" => Ok(ContentSourceKind::Registry),
            "local" => Ok(ContentSourceKind::Local),
            other => Err(ServiceError::Validation(format!(
                "unknown cache source '{other}'; expected one of: git, registry, local"
            ))),
        }
    }
}

/// Entry count + total on-disk bytes for one [`ContentSourceKind`], as
/// reported by [`SkillCache::stats`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct ContentSourceStats {
    pub entry_count: usize,
    pub total_bytes: u64,
}

/// `fastskill cache info` (PRD 006, US-006): the resolved cache root, plus
/// per-[`ContentSourceKind`] entry counts and disk usage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CacheStats {
    pub root: PathBuf,
    pub git: ContentSourceStats,
    pub registry: ContentSourceStats,
    pub local: ContentSourceStats,
}

impl CacheStats {
    /// The stats for one [`ContentSourceKind`].
    pub fn for_kind(&self, kind: ContentSourceKind) -> ContentSourceStats {
        match kind {
            ContentSourceKind::Git => self.git,
            ContentSourceKind::Registry => self.registry,
            ContentSourceKind::Local => self.local,
        }
    }

    /// Entry counts and bytes summed across all three kinds.
    pub fn total(&self) -> ContentSourceStats {
        ContentSourceStats {
            entry_count: self.git.entry_count + self.registry.entry_count + self.local.entry_count,
            total_bytes: self.git.total_bytes + self.registry.total_bytes + self.local.total_bytes,
        }
    }
}

/// Result of `fastskill cache clean` (PRD 006, US-006): content entries
/// removed and bytes reclaimed. Never covers `index/` — see
/// [`SkillCache::clean`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CleanReport {
    pub entries_removed: usize,
    pub bytes_reclaimed: u64,
}

/// The on-disk, machine-global skill cache (PRD 006 / RFQ 004). The single
/// seam through which any code reads or writes cached skill content and
/// index data — see the module docs for the on-disk layout and safety
/// properties.
#[derive(Debug, Clone)]
pub struct SkillCache {
    root: PathBuf,
}

impl SkillCache {
    /// Construct a cache rooted at an explicit directory.
    ///
    /// Prefer this in tests — pointed at a `tempfile::TempDir` — over
    /// [`SkillCache::from_env`]: `FASTSKILL_CACHE_DIR` is process-global, so
    /// asserting on its resolution races against any other test in the same
    /// process that also touches it. Test env-var resolution itself
    /// separately (see `resolve_root`'s tests) rather than through this
    /// constructor.
    pub fn at_root(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Construct a cache rooted at [`SkillCache::resolve_root`] (env
    /// override or the platform cache dir). What production call sites use.
    pub fn from_env() -> Result<Self, ServiceError> {
        Ok(Self::at_root(Self::resolve_root()?))
    }

    /// Resolve the cache root: `FASTSKILL_CACHE_DIR` if set to a non-blank
    /// value, else the platform cache dir (`dirs::cache_dir()/fastskill`,
    /// e.g. `~/.cache/fastskill` on Linux).
    pub fn resolve_root() -> Result<PathBuf, ServiceError> {
        if let Ok(dir) = std::env::var(FASTSKILL_CACHE_DIR_ENV) {
            if !dir.trim().is_empty() {
                return Ok(PathBuf::from(dir));
            }
        }
        dirs::cache_dir()
            .map(|d| d.join("fastskill"))
            .ok_or_else(|| {
                ServiceError::Config(format!(
                    "could not determine the platform cache directory on this platform; set \
                 {FASTSKILL_CACHE_DIR_ENV} to override"
                ))
            })
    }

    /// The cache root this instance is rooted at.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Look up `identity` in the content cache.
    ///
    /// Returns `None` on a miss, including when the identity is present but
    /// not yet fully published — a torn/in-progress `put` never lives at the
    /// final path (see module docs) — and also degrades I/O errors (e.g. a
    /// permission problem reading the cache root) to a miss, since it is
    /// always safe to treat a cache as absent and re-fetch.
    pub fn get(&self, identity: &CacheIdentity) -> Option<CachedContent> {
        let rel = identity.relative_path().ok()?;
        let path = self.root.join(rel);
        if path.is_dir() {
            Some(CachedContent {
                identity: identity.clone(),
                path,
            })
        } else {
            None
        }
    }

    /// Publish `source_dir`'s contents into the cache under `identity`.
    ///
    /// Crash/concurrency-safe: `source_dir` is first copied into a private
    /// staging directory under `<root>/tmp/`, then published with a single
    /// atomic rename into the identity's final path. If another `put` for
    /// the same identity has already published — or wins a race against
    /// this call — this is a harmless no-op that returns the existing entry
    /// rather than erroring or double-writing.
    pub fn put(
        &self,
        identity: &CacheIdentity,
        source_dir: &Path,
    ) -> Result<CachedContent, ServiceError> {
        let rel = identity.relative_path()?;
        let final_path = self.root.join(&rel);

        if final_path.is_dir() {
            // Already cached: a previous put (this process or another)
            // already published this identity. Harmless no-op.
            return Ok(CachedContent {
                identity: identity.clone(),
                path: final_path,
            });
        }
        if !source_dir.is_dir() {
            return Err(ServiceError::Validation(format!(
                "cache put source is not a directory: {}",
                source_dir.display()
            )));
        }

        let staging_root = self.root.join(STAGING_DIR_NAME);
        fs::create_dir_all(&staging_root)?;
        if let Err(e) = write_cache_tag(&self.root) {
            // Cosmetic/self-identification only (see `verify_looks_like_cache_root`,
            // which does not require it); must never fail a `put`.
            tracing::warn!("failed to write cache directory tag: {}", e);
        }
        let staging = tempfile::Builder::new()
            .prefix(STAGING_DIR_PREFIX)
            .tempdir_in(&staging_root)?;

        copy_dir_contents(source_dir, staging.path())?;

        if let Some(parent) = final_path.parent() {
            fs::create_dir_all(parent)?;
        }

        match publish(staging.path(), &final_path) {
            Ok(()) => Ok(CachedContent {
                identity: identity.clone(),
                path: final_path,
            }),
            Err(_) if final_path.is_dir() => {
                // Lost a race to a concurrent `put` of the same identity —
                // the other writer already published; treat as a hit
                // instead of surfacing the rename failure as an error.
                Ok(CachedContent {
                    identity: identity.clone(),
                    path: final_path,
                })
            }
            Err(err) => Err(ServiceError::Io(err)),
        }
    }

    // ---- Index read/write APIs. Schema owned by `index`; see its docs. ---

    /// Read `<cache-root>/index/<source>.json`, or `None` if never written.
    pub fn read_source_index(&self, source: &str) -> Result<Option<SourceIndex>, ServiceError> {
        index::read_source_index(&self.root, source)
    }

    /// Atomically write `<cache-root>/index/<source>.json`.
    pub fn write_source_index(&self, source: &str, idx: &SourceIndex) -> Result<(), ServiceError> {
        index::write_source_index(&self.root, source, idx)
    }

    /// Read `<cache-root>/index/git-resolutions.json`, or an empty map if
    /// never written.
    pub fn read_git_resolutions(&self) -> Result<GitResolutions, ServiceError> {
        index::read_git_resolutions(&self.root)
    }

    /// Atomically write `<cache-root>/index/git-resolutions.json`.
    pub fn write_git_resolutions(&self, resolutions: &GitResolutions) -> Result<(), ServiceError> {
        index::write_git_resolutions(&self.root, resolutions)
    }

    // ---- `fastskill cache info`/`clean` (PRD 006, US-006). --------------

    /// Entry counts and on-disk bytes per [`ContentSourceKind`] (`fastskill
    /// cache info`). A cache root that has never been used (no `put` has
    /// ever run) reports all-zero stats rather than erroring.
    pub fn stats(&self) -> Result<CacheStats, ServiceError> {
        Ok(CacheStats {
            root: self.root.clone(),
            git: self.source_stats(ContentSourceKind::Git)?,
            registry: self.source_stats(ContentSourceKind::Registry)?,
            local: self.source_stats(ContentSourceKind::Local)?,
        })
    }

    fn source_stats(&self, kind: ContentSourceKind) -> Result<ContentSourceStats, ServiceError> {
        let dir = self.root.join(kind.dir_name());
        if !dir.is_dir() {
            return Ok(ContentSourceStats::default());
        }
        let mut stats = ContentSourceStats::default();
        for leaf in leaf_identity_dirs(&dir, kind.leaf_depth())? {
            stats.entry_count += 1;
            stats.total_bytes += dir_size(&leaf)?;
        }
        Ok(stats)
    }

    /// Remove content entries (`fastskill cache clean`): every
    /// [`ContentSourceKind`] when `source` is `None`, or just the given one.
    /// Never touches `index/` — an explicit `repos refresh` (US-005) is the
    /// only thing that invalidates the index cache, per the PRD's "removes
    /// all content entries" (content, not index) and "Resolved Defaults"
    /// (index has no v1 TTL/GC of its own). Never touches `tmp/` either:
    /// that is another process's live staging area, not a content entry.
    ///
    /// Safe to run while another fastskill process is fetching (US-001's
    /// locking + atomic publish already make a concurrent `get`/`put` race
    /// with a delete safe — worst case the other process's copy-out of a
    /// just-deleted entry fails and it re-fetches on retry, never a torn or
    /// corrupt read).
    ///
    /// See the module's safety notes (and this function's private helpers)
    /// for the adversarial argument: `clean` only ever descends into `git/`,
    /// `registry/`, `local/` under the *resolved* cache root; it refuses
    /// outright if that root contains anything it does not recognize
    /// ([`verify_looks_like_cache_root`]); it never follows a symlink while
    /// walking or deleting ([`leaf_identity_dirs`], [`remove_dir_no_symlinks`]);
    /// and it re-canonicalizes every entry immediately before deleting it and
    /// refuses to delete anything that no longer resolves back inside the
    /// canonical root ([`SkillCache::clean`]'s loop below).
    pub fn clean(&self, source: Option<ContentSourceKind>) -> Result<CleanReport, ServiceError> {
        if !self.root.is_dir() {
            // Nothing has ever been cached here (or it was already cleaned
            // down to nothing and the root itself removed by something
            // else): a no-op is the safe, honest outcome, not an error.
            return Ok(CleanReport::default());
        }
        verify_looks_like_cache_root(&self.root)?;
        // Canonicalized once, up front, as the trust anchor every candidate
        // deletion below is re-checked against.
        let canonical_root = self.root.canonicalize()?;

        let kinds: &[ContentSourceKind] = match &source {
            Some(kind) => std::slice::from_ref(kind),
            None => &ContentSourceKind::ALL,
        };

        let mut report = CleanReport::default();
        for &kind in kinds {
            let dir = self.root.join(kind.dir_name());
            if !dir.is_dir() {
                continue;
            }
            for leaf in leaf_identity_dirs(&dir, kind.leaf_depth())? {
                // Defense in depth: re-resolve the leaf's real path
                // immediately before deleting it and refuse unless it is
                // still inside the canonical cache root. Guards against a
                // symlinked intermediate directory (planted between the
                // walk above and this delete, or simply missed by the walk)
                // silently redirecting the delete outside the cache.
                let canonical_leaf = match leaf.canonicalize() {
                    Ok(p) => p,
                    // Vanished since the walk (e.g. a concurrent `clean`, or
                    // the entry this replaced was just deleted): nothing
                    // left to reclaim here, not an error.
                    Err(_) => continue,
                };
                if !canonical_leaf.starts_with(&canonical_root) {
                    tracing::warn!(
                        "refusing to delete cache entry outside the cache root: {}",
                        leaf.display()
                    );
                    continue;
                }
                let size = dir_size(&leaf).unwrap_or(0);
                remove_dir_no_symlinks(&leaf)?;
                report.entries_removed += 1;
                report.bytes_reclaimed += size;
            }
        }
        Ok(report)
    }
}

/// Publish a fully-assembled staging directory at `to` via atomic rename.
///
/// On Windows, a rename can transiently fail (`PermissionDenied`, a sharing
/// violation) if another process has briefly opened the source or target —
/// e.g. an AV scanner or search indexer. Retry a bounded number of times
/// before giving up. `fs::rename` is atomic on POSIX with no such caveat.
fn publish(from: &Path, to: &Path) -> io::Result<()> {
    #[cfg(not(windows))]
    {
        fs::rename(from, to)
    }
    #[cfg(windows)]
    {
        let mut attempt = 0;
        loop {
            match fs::rename(from, to) {
                Ok(()) => return Ok(()),
                Err(e)
                    if attempt < WINDOWS_RENAME_MAX_RETRIES
                        && e.kind() == io::ErrorKind::PermissionDenied =>
                {
                    attempt += 1;
                    std::thread::sleep(WINDOWS_RENAME_RETRY_DELAY);
                }
                Err(e) => return Err(e),
            }
        }
    }
}

/// Recursively copy the *contents* of `src` into the already-existing `dst`
/// directory. Rejects symlink entries — mirrors `install.rs`'s
/// `copy_dir_recursive` (SEC-4): a symlink inside cached content must not be
/// silently dereferenced and its target's contents pulled into the cache.
fn copy_dir_contents(src: &Path, dst: &Path) -> io::Result<()> {
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        if file_type.is_symlink() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("refusing to cache symlink: {}", src_path.display()),
            ));
        } else if file_type.is_dir() {
            fs::create_dir_all(&dst_path)?;
            copy_dir_contents(&src_path, &dst_path)?;
        } else {
            fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

/// Write [`CACHE_TAG_FILE_NAME`] at `root` if it is not already there.
/// Best-effort self-identification only — see the constant's docs and its
/// only caller, [`SkillCache::put`].
fn write_cache_tag(root: &Path) -> io::Result<()> {
    let tag_path = root.join(CACHE_TAG_FILE_NAME);
    if tag_path.is_file() {
        return Ok(());
    }
    fs::write(tag_path, CACHE_TAG_CONTENTS)
}

/// `fastskill cache clean`'s (US-006) first line of defense: refuse to
/// touch anything unless every entry directly under the resolved cache root
/// is one this module itself would have created
/// ([`CACHE_ROOT_ALLOWED_ENTRIES`]). If `FASTSKILL_CACHE_DIR` is
/// misconfigured to point at, say, a home directory or a project checkout,
/// that directory will almost certainly contain *something* unrecognized
/// (`.bashrc`, `Documents`, `src`, ...), and `clean` fails closed before it
/// ever lists — let alone deletes — anything inside `git/`, `registry/`, or
/// `local/`.
fn verify_looks_like_cache_root(root: &Path) -> Result<(), ServiceError> {
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !CACHE_ROOT_ALLOWED_ENTRIES.contains(&name.as_ref()) {
            return Err(ServiceError::Validation(format!(
                "refusing to clean '{}': it does not look like a fastskill cache root (found \
                 unexpected entry '{}'). If this is really your cache directory, remove any \
                 unrelated files from it first; otherwise check FASTSKILL_CACHE_DIR",
                root.display(),
                name,
            )));
        }
    }
    Ok(())
}

/// Collect every directory exactly `depth` levels below `base` — the leaf
/// identity directories `stats`/`clean` treat as one content entry (see
/// [`ContentSourceKind::leaf_depth`]).
///
/// Never follows a symlink: a symlinked entry anywhere in the walk is
/// skipped outright (neither descended into nor returned as a leaf), the
/// same SEC-4 stance [`copy_dir_contents`] takes for cache *writes*. A
/// non-directory entry at an intermediate level (unexpected, but not this
/// function's job to interpret) is likewise skipped rather than errored on.
fn leaf_identity_dirs(base: &Path, depth: u32) -> Result<Vec<PathBuf>, ServiceError> {
    let mut out = Vec::new();
    collect_dirs_at_depth(base, depth, &mut out)?;
    Ok(out)
}

fn collect_dirs_at_depth(
    dir: &Path,
    depth: u32,
    out: &mut Vec<PathBuf>,
) -> Result<(), ServiceError> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if file_type.is_symlink() || !file_type.is_dir() {
            continue;
        }
        if depth <= 1 {
            out.push(entry.path());
        } else {
            collect_dirs_at_depth(&entry.path(), depth - 1, out)?;
        }
    }
    Ok(())
}

/// Recursively sum the size of regular files under `dir`. Symlinks are
/// skipped rather than followed (mirrors [`copy_dir_contents`]'s stance);
/// `put` never writes one into the cache, but this stays honest even if
/// something else did.
fn dir_size(dir: &Path) -> Result<u64, ServiceError> {
    let mut total = 0u64;
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            continue;
        } else if file_type.is_dir() {
            total += dir_size(&entry.path())?;
        } else {
            total += entry.metadata()?.len();
        }
    }
    Ok(total)
}

/// Recursively delete `dir` and its contents without ever dereferencing a
/// symlink: a symlink entry is unlinked directly (never followed into its
/// target), matching [`copy_dir_contents`]'s SEC-4 stance for cache writes.
/// `dir` itself must not be a symlink (callers only ever pass leaves found
/// by [`leaf_identity_dirs`], which already excludes symlinked entries).
///
/// Deliberately does not use `std::fs::remove_dir_all`: this walks and
/// unlinks entries itself so the "never follow a symlink" guarantee is
/// explicit and auditable here rather than resting on `remove_dir_all`'s
/// platform-specific internals.
fn remove_dir_no_symlinks(dir: &Path) -> Result<(), ServiceError> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let path = entry.path();
        if file_type.is_symlink() {
            fs::remove_file(&path)?;
        } else if file_type.is_dir() {
            remove_dir_no_symlinks(&path)?;
        } else {
            fs::remove_file(&path)?;
        }
    }
    fs::remove_dir(dir)?;
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use std::sync::{Arc, Barrier};
    use std::thread;
    use tempfile::TempDir;

    fn write_file(dir: &Path, name: &str, contents: &str) {
        std::fs::write(dir.join(name), contents).unwrap();
    }

    /// A small, realistic skill directory used as a `put` source.
    fn sample_source_dir(root: &Path) -> PathBuf {
        let src = root.join("source");
        std::fs::create_dir_all(src.join("nested")).unwrap();
        write_file(&src, "SKILL.md", "---\nname: demo\n---\nbody\n");
        write_file(&src.join("nested"), "helper.py", "print('hi')\n");
        src
    }

    #[test]
    fn get_misses_when_identity_never_put() {
        let root = TempDir::new().unwrap();
        let cache = SkillCache::at_root(root.path());
        let identity = CacheIdentity::Git {
            sha: "deadbeef".to_string(),
        };

        assert!(cache.get(&identity).is_none());
    }

    #[test]
    fn put_then_get_is_a_hit_with_matching_content() {
        let root = TempDir::new().unwrap();
        let cache = SkillCache::at_root(root.path());
        let source = sample_source_dir(root.path());
        let identity = CacheIdentity::Registry {
            source: "acme".to_string(),
            skill: "demo".to_string(),
            version: "1.0.0".to_string(),
        };

        let put = cache.put(&identity, &source).unwrap();
        assert!(put.path.is_dir());
        assert_eq!(put.path, root.path().join("registry/acme/demo/1.0.0"));

        let hit = cache
            .get(&identity)
            .expect("expected a cache hit after put");
        assert_eq!(hit.path, put.path);
        assert_eq!(
            std::fs::read_to_string(hit.path.join("SKILL.md")).unwrap(),
            "---\nname: demo\n---\nbody\n"
        );
        assert_eq!(
            std::fs::read_to_string(hit.path.join("nested/helper.py")).unwrap(),
            "print('hi')\n"
        );
    }

    /// Env-var resolution logic, tested in isolation (per PRD guidance) so
    /// no other test's cache-root assumptions can race against it.
    #[test]
    fn env_var_override_changes_the_resolved_root() {
        let dir = TempDir::new().unwrap();
        let previous = std::env::var(FASTSKILL_CACHE_DIR_ENV).ok();

        std::env::set_var(FASTSKILL_CACHE_DIR_ENV, dir.path());
        let resolved = SkillCache::resolve_root().unwrap();

        match previous {
            Some(v) => std::env::set_var(FASTSKILL_CACHE_DIR_ENV, v),
            None => std::env::remove_var(FASTSKILL_CACHE_DIR_ENV),
        }

        assert_eq!(resolved, dir.path().to_path_buf());
    }

    #[test]
    fn concurrent_duplicate_put_is_a_harmless_noop() {
        let root = TempDir::new().unwrap();
        let cache = Arc::new(SkillCache::at_root(root.path()));
        let source = sample_source_dir(root.path());
        let identity = CacheIdentity::Local {
            tree_hash: "abc123".to_string(),
        };

        let barrier = Arc::new(Barrier::new(2));
        let handles: Vec<_> = (0..2)
            .map(|_| {
                let cache = Arc::clone(&cache);
                let source = source.clone();
                let identity = identity.clone();
                let barrier = Arc::clone(&barrier);
                thread::spawn(move || {
                    barrier.wait();
                    cache.put(&identity, &source)
                })
            })
            .collect();

        let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        for result in &results {
            assert!(
                result.is_ok(),
                "concurrent put must never error: {result:?}"
            );
        }

        let paths: Vec<_> = results.into_iter().map(|r| r.unwrap().path).collect();
        assert_eq!(
            paths[0], paths[1],
            "both callers converge on one final path"
        );
        assert_eq!(
            std::fs::read_to_string(paths[0].join("SKILL.md")).unwrap(),
            "---\nname: demo\n---\nbody\n"
        );

        // No leftover staging directories after the race resolves.
        let staging_root = root.path().join(STAGING_DIR_NAME);
        if staging_root.is_dir() {
            let leftovers: Vec<_> = std::fs::read_dir(&staging_root).unwrap().collect();
            assert!(
                leftovers.is_empty(),
                "no staging leftovers should remain once a race resolves"
            );
        }
    }

    #[test]
    fn torn_staged_write_is_never_visible_via_get() {
        let root = TempDir::new().unwrap();
        let cache = SkillCache::at_root(root.path());
        let identity = CacheIdentity::Git {
            sha: "cafefeed".to_string(),
        };

        // Simulate a crash between "assembled in staging" and "atomic
        // rename": content exists on disk, but only inside `tmp/`, never at
        // the identity's final path.
        let staging_root = root.path().join(STAGING_DIR_NAME);
        let torn = staging_root.join(format!("{STAGING_DIR_PREFIX}torn"));
        std::fs::create_dir_all(&torn).unwrap();
        write_file(&torn, "SKILL.md", "incomplete\n");

        assert!(cache.get(&identity).is_none());
        assert!(!root.path().join("git/cafefeed").exists());
    }

    #[cfg(unix)]
    #[test]
    fn put_rejects_a_symlink_in_the_source_tree() {
        let root = TempDir::new().unwrap();
        let cache = SkillCache::at_root(root.path());
        let source = sample_source_dir(root.path());
        std::os::unix::fs::symlink(root.path(), source.join("evil-link")).unwrap();

        let identity = CacheIdentity::Local {
            tree_hash: "haslink".to_string(),
        };

        assert!(cache.put(&identity, &source).is_err());
        assert!(cache.get(&identity).is_none());
    }

    #[test]
    fn cache_identity_rejects_path_traversal_components() {
        let identity = CacheIdentity::Registry {
            source: "../escape".to_string(),
            skill: "demo".to_string(),
            version: "1.0.0".to_string(),
        };

        assert!(identity.relative_path().is_err());
    }

    // ── `fastskill cache info`/`clean` (PRD 006, US-006) ──────────────────

    /// `put` a small sample skill under `identity`. The source lives in its
    /// own throwaway temp dir -- never inside `cache`'s root -- so tests
    /// that go on to call `clean` (which refuses a root containing anything
    /// it does not recognize) are not tripped up by their own fixture data.
    fn put_sample(cache: &SkillCache, identity: &CacheIdentity, marker: &str) {
        let src = TempDir::new().unwrap();
        write_file(
            src.path(),
            "SKILL.md",
            &format!("---\nname: {marker}\n---\nbody\n"),
        );
        cache.put(identity, src.path()).unwrap();
    }

    #[test]
    fn stats_on_a_never_used_root_is_all_zero() {
        let root = TempDir::new().unwrap();
        // Never call `put`: the root directory itself does not even exist.
        let cache = SkillCache::at_root(root.path().join("never-created"));

        let stats = cache.stats().unwrap();
        assert_eq!(stats.git, ContentSourceStats::default());
        assert_eq!(stats.registry, ContentSourceStats::default());
        assert_eq!(stats.local, ContentSourceStats::default());
    }

    #[test]
    fn stats_counts_entries_and_bytes_per_source_kind() {
        let root = TempDir::new().unwrap();
        let cache = SkillCache::at_root(root.path());

        put_sample(
            &cache,
            &CacheIdentity::Git {
                sha: "a".repeat(40),
            },
            "git-a",
        );
        put_sample(
            &cache,
            &CacheIdentity::Registry {
                source: "acme".to_string(),
                skill: "demo".to_string(),
                version: "1.0.0".to_string(),
            },
            "reg-a",
        );
        put_sample(
            &cache,
            &CacheIdentity::Registry {
                source: "acme".to_string(),
                skill: "demo".to_string(),
                version: "2.0.0".to_string(),
            },
            "reg-b",
        );
        put_sample(
            &cache,
            &CacheIdentity::Local {
                tree_hash: "deadbeef".to_string(),
            },
            "local-a",
        );

        let stats = cache.stats().unwrap();
        assert_eq!(stats.git.entry_count, 1);
        assert_eq!(stats.registry.entry_count, 2);
        assert_eq!(stats.local.entry_count, 1);
        assert!(stats.git.total_bytes > 0);
        assert!(stats.registry.total_bytes > 0);
        assert!(stats.local.total_bytes > 0);
        assert_eq!(stats.total().entry_count, 4);
    }

    #[test]
    fn clean_with_no_source_filter_removes_every_kind_but_not_the_index() {
        let root = TempDir::new().unwrap();
        let cache = SkillCache::at_root(root.path());

        put_sample(
            &cache,
            &CacheIdentity::Git {
                sha: "b".repeat(40),
            },
            "git-b",
        );
        put_sample(
            &cache,
            &CacheIdentity::Local {
                tree_hash: "cafef00d".to_string(),
            },
            "local-b",
        );
        cache
            .write_source_index(
                "acme",
                &SourceIndex {
                    fetched_at: chrono::Utc::now(),
                    entries: vec![],
                },
            )
            .unwrap();

        let report = cache.clean(None).unwrap();
        assert_eq!(report.entries_removed, 2);
        assert!(report.bytes_reclaimed > 0);

        let stats_after = cache.stats().unwrap();
        assert_eq!(stats_after.total(), ContentSourceStats::default());
        // The index cache is untouched by `clean` (PRD: "removes all content
        // entries" -- content, not index).
        assert!(cache.read_source_index("acme").unwrap().is_some());
    }

    #[test]
    fn clean_with_source_filter_only_removes_that_kind() {
        let root = TempDir::new().unwrap();
        let cache = SkillCache::at_root(root.path());

        put_sample(
            &cache,
            &CacheIdentity::Git {
                sha: "c".repeat(40),
            },
            "git-c",
        );
        put_sample(
            &cache,
            &CacheIdentity::Local {
                tree_hash: "0ff1ce".to_string(),
            },
            "local-c",
        );

        let report = cache.clean(Some(ContentSourceKind::Git)).unwrap();
        assert_eq!(report.entries_removed, 1);

        let stats_after = cache.stats().unwrap();
        assert_eq!(stats_after.git, ContentSourceStats::default());
        assert_eq!(stats_after.local.entry_count, 1, "local entry untouched");
    }

    #[test]
    fn clean_on_a_never_used_root_is_a_harmless_noop() {
        let root = TempDir::new().unwrap();
        let cache = SkillCache::at_root(root.path().join("never-created"));

        let report = cache.clean(None).unwrap();
        assert_eq!(report, CleanReport::default());
    }

    #[test]
    fn clean_refuses_a_root_that_does_not_look_like_a_fastskill_cache() {
        let root = TempDir::new().unwrap();
        // Simulate `FASTSKILL_CACHE_DIR` misconfigured to point at a real,
        // unrelated directory (e.g. a home directory) rather than an actual
        // cache root.
        std::fs::write(root.path().join("Documents.txt"), "not a cache").unwrap();
        let cache = SkillCache::at_root(root.path());

        let err = cache
            .clean(None)
            .expect_err("clean must refuse a root with unrecognized entries");
        assert!(matches!(err, ServiceError::Validation(_)));
        // Nothing was touched.
        assert!(root.path().join("Documents.txt").is_file());
    }

    #[cfg(unix)]
    #[test]
    fn clean_never_follows_or_deletes_through_a_symlinked_entry() {
        let root = TempDir::new().unwrap();
        let cache = SkillCache::at_root(root.path());

        // A real, legitimately cached entry.
        put_sample(
            &cache,
            &CacheIdentity::Local {
                tree_hash: "realentry".to_string(),
            },
            "local-real",
        );

        // A directory *outside* the cache root that a symlink could redirect
        // deletion into, plus a sentinel file inside it.
        let outside = TempDir::new().unwrap();
        std::fs::write(outside.path().join("sentinel.txt"), "do not delete me").unwrap();

        // Plant a symlink directly under `git/` masquerading as a cached SHA
        // entry, pointing at the directory outside the cache root.
        let git_dir = root.path().join("git");
        std::fs::create_dir_all(&git_dir).unwrap();
        std::os::unix::fs::symlink(outside.path(), git_dir.join("evilsha")).unwrap();

        let report = cache.clean(None).unwrap();

        // The symlink itself is never treated as a leaf identity directory
        // (skipped, not deleted, not followed) -- only the one real local
        // entry is counted.
        assert_eq!(report.entries_removed, 1);
        assert!(
            outside.path().join("sentinel.txt").is_file(),
            "clean must never delete through a symlink outside the cache root"
        );
        assert!(
            git_dir.join("evilsha").exists(),
            "the symlink entry itself is left alone, not silently deleted"
        );
    }

    #[test]
    fn content_source_kind_from_str_round_trips_and_rejects_unknown() {
        use std::str::FromStr;
        assert_eq!(
            ContentSourceKind::from_str("git").unwrap(),
            ContentSourceKind::Git
        );
        assert_eq!(
            ContentSourceKind::from_str("registry").unwrap(),
            ContentSourceKind::Registry
        );
        assert_eq!(
            ContentSourceKind::from_str("local").unwrap(),
            ContentSourceKind::Local
        );
        assert!(ContentSourceKind::from_str("../etc").is_err());
        assert!(ContentSourceKind::from_str("bogus").is_err());
    }
}
