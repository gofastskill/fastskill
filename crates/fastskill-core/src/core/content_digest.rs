//! The content digest: one string that identifies the exact files of an installed skill
//! directory. Locks pin it as `checksum`, bundle records and bundle archives pin it per
//! member, and install, update and remove compare it with what is on disk before they touch
//! anything. See [ADR-0017](../../../../docs/adr/0017-versioned-content-digests.md).
//!
//! # Current form
//!
//! `sha256-tree-v2:<64 lowercase hex>` — SHA-256 over:
//!
//! 1. the domain tag [`V2_DOMAIN`], length-prefixed;
//! 2. for every regular file, in byte order of its path: the path relative to the directory
//!    (UTF-8, components joined with `/`), length-prefixed, then the file's contents,
//!    length-prefixed.
//!
//! Every length is a big-endian `u64`, so the byte stream decodes to exactly one tree.
//! Symbolic links are refused. File names that are not valid UTF-8 are refused, because
//! they have no portable spelling.
//!
//! # Legacy form
//!
//! Before ADR-0017 the digest was 64 bare hex characters, and it hashed file contents
//! without their length, so the bytes of one file could stand in for the next file's path
//! and contents. Legacy digests are still accepted when they match, with a warning, so that
//! Locks and bundles written by older releases keep installing. Every digest fastskill
//! writes is in the current form; a legacy value is replaced whenever its record is
//! rewritten.

use crate::core::service::ServiceError;
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Read;
use std::path::{Component, Path};
use walkdir::WalkDir;

/// Prefix of every current-form content digest.
pub const CONTENT_DIGEST_PREFIX: &str = "sha256-tree-v2:";

/// Domain tag hashed first, so a current-form digest never equals a hash of anything else.
const V2_DOMAIN: &str = "fastskill content digest v2";

/// Compute the current-form content digest of the skill directory at `path`.
pub fn content_digest(path: &Path) -> Result<String, ServiceError> {
    ensure_directory(path)?;
    let mut files = Vec::new();
    for entry in WalkDir::new(path) {
        let entry = entry.map_err(|error| ServiceError::Io(std::io::Error::other(error)))?;
        refuse_symlink(&entry)?;
        if entry.file_type().is_file() {
            files.push((
                portable_relative_path(path, entry.path())?,
                entry.into_path(),
            ));
        }
    }
    files.sort_by(|left, right| left.0.cmp(&right.0));

    let mut hasher = Sha256::new();
    hash_field(&mut hasher, V2_DOMAIN);
    for (relative, file) in &files {
        hash_field(&mut hasher, relative);
        hash_file_framed(&mut hasher, file)?;
    }
    Ok(format!(
        "{CONTENT_DIGEST_PREFIX}{}",
        crate::utils::to_hex_lower(&hasher.finalize())
    ))
}

/// Whether `expected` names the contents of the directory at `path`.
///
/// A legacy digest is compared with the legacy algorithm and, when it matches, a warning
/// says the record is weaker than the current form. Any other value is compared with
/// [`content_digest`], so only a current-form digest can match. Content that cannot be read
/// is an error whatever the recorded form.
pub fn content_digest_matches(expected: &str, path: &Path) -> Result<bool, ServiceError> {
    if !is_legacy_digest(expected) {
        // An unrecognised record never matches, but unreadable content is still an error.
        return Ok(content_digest(path)? == expected);
    }
    let matches = legacy_content_digest(path)? == expected;
    if matches {
        warn_legacy_digest_accepted();
    }
    Ok(matches)
}

/// The digest to record for content that matched `stored`: the current form of `path`
/// when `stored` is a matching legacy digest, and `stored` unchanged otherwise. Used where
/// a rewrite carries a recorded digest forward instead of recomputing it.
pub fn upgraded_digest(stored: &str, path: &Path) -> Result<String, ServiceError> {
    if is_legacy_digest(stored) && content_digest_matches(stored, path)? {
        return content_digest(path);
    }
    Ok(stored.to_string())
}

/// Whether `value` has the legacy form: exactly 64 lowercase hex characters, no prefix.
pub fn is_legacy_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Whether every recorded digest names the contents of `path`.
pub fn all_match(recorded: &[&str], path: &Path) -> Result<bool, ServiceError> {
    for digest in recorded {
        if !content_digest_matches(digest, path)? {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Whether recorded digests disagree about one skill's contents. Two digests of the same
/// form disagree when they differ. A current-form and a legacy digest cannot be compared
/// without the content, so they are not reported here; callers check each against the
/// content itself.
pub fn recorded_digests_conflict<'a>(digests: impl IntoIterator<Item = &'a str>) -> bool {
    let digests: Vec<&str> = digests.into_iter().collect();
    digests.iter().enumerate().any(|(index, left)| {
        digests[index + 1..]
            .iter()
            .any(|right| left != right && is_legacy_digest(left) == is_legacy_digest(right))
    })
}

/// A directory's digest in both forms, for content that is compared with recorded digests
/// of either form many times, such as the members of a bundle artifact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DigestForms {
    /// The form fastskill records.
    pub(crate) current: String,
    /// The legacy form, used only to accept records written by older releases.
    pub(crate) legacy: String,
}

impl DigestForms {
    pub(crate) fn of_directory(path: &Path) -> Result<Self, ServiceError> {
        Ok(Self {
            current: content_digest(path)?,
            legacy: legacy_content_digest(path)?,
        })
    }

    /// Whether a recorded digest names this content, in either form.
    pub(crate) fn matches(&self, recorded: &str) -> bool {
        if recorded == self.current {
            return true;
        }
        let matches = recorded == self.legacy;
        if matches {
            warn_legacy_digest_accepted();
        }
        matches
    }
}

/// The digest algorithm before ADR-0017. Kept only to verify records older releases wrote;
/// never used to write a new record.
pub(crate) fn legacy_content_digest(path: &Path) -> Result<String, ServiceError> {
    ensure_directory(path)?;
    let mut hasher = Sha256::new();
    for entry in WalkDir::new(path).sort_by_file_name() {
        let entry = entry.map_err(|error| ServiceError::Io(std::io::Error::other(error)))?;
        refuse_symlink(&entry)?;
        if !entry.file_type().is_file() {
            continue;
        }
        let relative = entry.path().strip_prefix(path).map_err(|error| {
            ServiceError::Custom(format!("Failed to form skill digest path: {error}"))
        })?;
        hash_field(&mut hasher, relative.to_string_lossy().replace('\\', "/"));
        let mut file = fs::File::open(entry.path()).map_err(ServiceError::Io)?;
        let mut buffer = [0u8; 8192];
        loop {
            let read = file.read(&mut buffer).map_err(ServiceError::Io)?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
        }
    }
    Ok(crate::utils::to_hex_lower(&hasher.finalize()))
}

/// Hash `value` with its length, so that consecutive fields cannot run into each other.
pub(crate) fn hash_field(hasher: &mut Sha256, value: impl AsRef<[u8]>) {
    let value = value.as_ref();
    hasher.update((value.len() as u64).to_be_bytes());
    hasher.update(value);
}

fn hash_file_framed(hasher: &mut Sha256, path: &Path) -> Result<(), ServiceError> {
    let file = fs::File::open(path).map_err(ServiceError::Io)?;
    let length = file.metadata().map_err(ServiceError::Io)?.len();
    hash_framed(hasher, file, length, path)
}

/// Hash `length`, then exactly `length` bytes from `reader`. A reader that yields more or
/// fewer bytes means the file changed after its length was taken.
fn hash_framed(
    hasher: &mut Sha256,
    mut reader: impl Read,
    length: u64,
    path: &Path,
) -> Result<(), ServiceError> {
    hasher.update(length.to_be_bytes());
    let mut remaining = length;
    let mut buffer = [0u8; 8192];
    loop {
        let read = reader.read(&mut buffer).map_err(ServiceError::Io)?;
        if read == 0 {
            break;
        }
        remaining = remaining
            .checked_sub(read as u64)
            .ok_or_else(|| changed(path))?;
        hasher.update(&buffer[..read]);
    }
    if remaining != 0 {
        return Err(changed(path));
    }
    Ok(())
}

fn changed(path: &Path) -> ServiceError {
    ServiceError::Validation(format!(
        "{} changed while its content digest was computed; retry",
        path.display()
    ))
}

/// The path of `file` relative to `root`, as UTF-8 components joined with `/`. Joining
/// components, rather than rewriting separators in a string, keeps a Unix file name that
/// contains `\` distinct from a nested path.
fn portable_relative_path(root: &Path, file: &Path) -> Result<String, ServiceError> {
    let relative = file.strip_prefix(root).map_err(|error| {
        ServiceError::Custom(format!("Failed to form skill digest path: {error}"))
    })?;
    let mut parts = Vec::new();
    for component in relative.components() {
        let Component::Normal(name) = component else {
            return Err(ServiceError::Custom(format!(
                "Unexpected path component in {}",
                file.display()
            )));
        };
        parts.push(name.to_str().ok_or_else(|| {
            ServiceError::Validation(format!(
                "File name is not valid UTF-8, so it has no portable content digest: {}",
                file.display()
            ))
        })?);
    }
    Ok(parts.join("/"))
}

fn ensure_directory(path: &Path) -> Result<(), ServiceError> {
    if path.is_dir() {
        return Ok(());
    }
    Err(ServiceError::Validation(format!(
        "Expected skill directory at {}",
        path.display()
    )))
}

fn refuse_symlink(entry: &walkdir::DirEntry) -> Result<(), ServiceError> {
    if entry.file_type().is_symlink() {
        return Err(ServiceError::Validation(format!(
            "Symbolic links are not permitted in bundle members: {}",
            entry.path().display()
        )));
    }
    Ok(())
}

fn warn_legacy_digest_accepted() {
    crate::utils::warn_once(
        "legacy-content-digest",
        "accepted a legacy content digest written by an older fastskill; legacy digests are \
         weaker than the current `sha256-tree-v2` form, and fastskill replaces each one the \
         next time it rewrites that record (ADR-0017)",
    );
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
#[path = "content_digest_tests.rs"]
mod tests;
