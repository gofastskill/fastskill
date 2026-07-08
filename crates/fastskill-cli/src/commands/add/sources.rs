//! Source type resolution and installation for the add command.

use crate::error::{CliError, CliResult};
use crate::utils::{detect_skill_source, parse_git_url, validate_skill_structure, SkillSource};
use fastskill_core::core::skill_manager::SourceType;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;
use tracing::info;

/// Resolved source type for the add operation
#[allow(dead_code)]
pub enum ResolvedSource {
    /// Local directory path
    Local(PathBuf),
    /// Git URL
    Git(String),
    /// ZIP file path
    Zip(PathBuf),
    /// Registry skill ID
    Registry(String),
}

/// Resolve source type from an explicit flag value or autodetect from the path/URL.
#[allow(dead_code)]
pub fn resolve_source_type(source: &str, source_type: Option<&str>) -> CliResult<ResolvedSource> {
    let Some(stype) = source_type else {
        return Ok(autodetect_source(source));
    };

    match stype {
        "registry" => Ok(ResolvedSource::Registry(source.to_string())),
        "github" | "git" => Ok(ResolvedSource::Git(source.to_string())),
        "local" => {
            let path = PathBuf::from(source);
            if path.extension().and_then(|s| s.to_str()) == Some("zip") {
                Ok(ResolvedSource::Zip(path))
            } else {
                Ok(ResolvedSource::Local(path))
            }
        }
        other => Err(CliError::Config(format!(
            "Unrecognized --source-type value: '{}'. Expected one of: registry, git, github, local",
            other
        ))),
    }
}

/// Autodetect source type from path/URL.
#[allow(dead_code)]
pub fn autodetect_source(source: &str) -> ResolvedSource {
    match detect_skill_source(source) {
        SkillSource::GitUrl(url) => ResolvedSource::Git(url),
        SkillSource::ZipFile(path) => ResolvedSource::Zip(path),
        SkillSource::Folder(path) => ResolvedSource::Local(path),
        SkillSource::SkillId(id) => ResolvedSource::Registry(id),
    }
}

/// Check if a path is a local path that could be a skill directory.
#[allow(dead_code)]
pub fn is_local_path(source: &str) -> bool {
    let path = Path::new(source);
    path.exists() || source.starts_with('.') || source.starts_with('/')
}

/// Safely join an untrusted `subdir` (from a shareable manifest / git tree URL)
/// onto a trusted clone `root`, rejecting path traversal.
///
/// Each component is validated via `fastskill_core::security::path::validate_path_component`
/// (rejecting `..`, `/`, `\`, and absolute components). After joining, if the target exists it
/// is canonicalized and asserted to stay within the canonicalized `root`, so a `subdir` escaping
/// the clone (via `..` or a symlink) is rejected with `CliError::InvalidSource`.
fn safe_subdir_join(root: &Path, subdir: &Path) -> CliResult<PathBuf> {
    use fastskill_core::security::path::validate_path_component;
    use std::path::Component;

    let mut joined = root.to_path_buf();
    for component in subdir.components() {
        match component {
            Component::Normal(part) => {
                let part = part.to_str().ok_or_else(|| {
                    CliError::InvalidSource(format!(
                        "Subdirectory '{}' contains a non-UTF-8 path component",
                        subdir.display()
                    ))
                })?;
                validate_path_component(part).map_err(|e| {
                    CliError::InvalidSource(format!(
                        "Invalid subdirectory '{}': {}",
                        subdir.display(),
                        e
                    ))
                })?;
                joined.push(part);
            }
            _ => {
                return Err(CliError::InvalidSource(format!(
                    "Subdirectory '{}' must be a relative path without '..' components",
                    subdir.display()
                )));
            }
        }
    }

    if joined.exists() {
        let canonical_root = root.canonicalize().map_err(CliError::Io)?;
        let canonical_joined = joined.canonicalize().map_err(CliError::Io)?;
        if !canonical_joined.starts_with(&canonical_root) {
            return Err(CliError::InvalidSource(format!(
                "Subdirectory '{}' escapes the cloned repository",
                subdir.display()
            )));
        }
    }

    Ok(joined)
}

// ── Source installation handlers ──────────────────────────────────────────────

/// Find SKILL.md in a directory (root first, then one level of subdirectories).
pub(super) fn find_skill_in_directory(dir: &Path) -> CliResult<PathBuf> {
    if dir.join("SKILL.md").exists() {
        return Ok(dir.to_path_buf());
    }
    let entries = fs::read_dir(dir).map_err(CliError::Io)?;
    for entry in entries {
        let entry = entry.map_err(CliError::Io)?;
        let path = entry.path();
        if path.is_dir() && path.join("SKILL.md").exists() {
            return Ok(path);
        }
    }
    Err(CliError::Validation(format!(
        "SKILL.md not found in: {}",
        dir.display()
    )))
}

fn extract_zip_to_temp(zip_path: &Path) -> CliResult<TempDir> {
    let temp_dir = TempDir::new().map_err(CliError::Io)?;
    let extract_path = temp_dir.path();
    use fastskill_core::storage::zip::ZipHandler;
    let zip_handler = ZipHandler::new()
        .map_err(|e| CliError::InvalidSource(format!("Failed to create ZIP handler: {}", e)))?;
    zip_handler
        .extract_to_dir(zip_path, extract_path)
        .map_err(|e| match e {
            fastskill_core::core::service::ServiceError::Validation(msg) => {
                CliError::InvalidSource(format!("ZIP extraction validation failed: {}", msg))
            }
            fastskill_core::core::service::ServiceError::Io(err) => CliError::Io(err),
            _ => CliError::InvalidSource(format!("ZIP extraction failed: {}", e)),
        })?;
    Ok(temp_dir)
}

pub(super) async fn add_from_zip(ctx: &super::AddContext<'_>, zip_path: &Path) -> CliResult<()> {
    info!("Adding skill from zip file: {}", zip_path.display());
    if !zip_path.exists() {
        return Err(CliError::InvalidSource(format!(
            "Zip file does not exist: {}",
            zip_path.display()
        )));
    }
    let canonical_zip_path = zip_path.canonicalize().map_err(|e| {
        CliError::InvalidSource(format!(
            "Failed to resolve absolute path for '{}': {}",
            zip_path.display(),
            e
        ))
    })?;
    let _temp_dir = extract_zip_to_temp(&canonical_zip_path)?;
    let extract_path = _temp_dir.path();
    let skill_path = find_skill_in_directory(extract_path)?;
    validate_skill_structure(&skill_path)?;
    let skill_def = super::skill_def::create_skill_from_path(&skill_path, "zip", false)?;
    let version = skill_def.version.clone();
    let target = super::InstallTarget {
        storage_dir: ctx
            .service
            .config()
            .skill_storage_path
            .join(skill_def.id.as_str()),
        meta: super::SourceMeta {
            source_url: Some(canonical_zip_path.to_string_lossy().to_string()),
            source_type: Some(SourceType::ZipFile),
            source_branch: None,
            source_tag: None,
            source_subdir: None,
            installed_from: None,
        },
        version_display: version,
    };
    super::install::install_via_download(ctx, &skill_path, skill_def, target).await
}

pub(super) async fn add_from_folder(
    ctx: &super::AddContext<'_>,
    folder_path: &Path,
) -> CliResult<()> {
    info!("Adding skill from folder: {}", folder_path.display());
    validate_skill_structure(folder_path)?;
    let skill_def = super::skill_def::create_skill_from_path(folder_path, "local", ctx.editable)?;
    let canonical_path = folder_path.canonicalize().map_err(|e| {
        CliError::InvalidSource(format!(
            "Failed to resolve absolute path for '{}': {}",
            folder_path.display(),
            e
        ))
    })?;
    let target = super::InstallTarget {
        storage_dir: ctx
            .service
            .config()
            .skill_storage_path
            .join(skill_def.id.as_str()),
        meta: super::SourceMeta {
            source_url: Some(canonical_path.to_string_lossy().to_string()),
            source_type: Some(SourceType::LocalPath),
            source_branch: None,
            source_tag: None,
            source_subdir: None,
            installed_from: None,
        },
        version_display: skill_def.version.clone(),
    };
    super::install::install_via_local_path(ctx, &canonical_path, skill_def, target).await
}

async fn clone_and_validate_skill(
    git_url: &str,
    branch: Option<&str>,
    tag: Option<&str>,
) -> CliResult<(
    TempDir,
    PathBuf,
    fastskill_core::SkillDefinition,
    Option<String>,
    Option<String>,
)> {
    use fastskill_core::storage::git::{clone_repository, validate_cloned_skill};
    let git_info = parse_git_url(git_url)?;
    let branch = branch.or(git_info.branch.as_deref());
    let temp_dir = clone_repository(&git_info.repo_url, branch, tag, None)
        .await
        .map_err(|e| CliError::GitCloneFailed(e.to_string()))?;
    let skill_base_path = match &git_info.subdir {
        Some(subdir) => {
            let subdir_path = safe_subdir_join(temp_dir.path(), subdir)?;
            if !subdir_path.exists() {
                return Err(CliError::InvalidSource(format!(
                    "Specified subdirectory '{}' does not exist in cloned repository",
                    subdir.display()
                )));
            }
            subdir_path
        }
        None => temp_dir.path().to_path_buf(),
    };
    let skill_path = validate_cloned_skill(&skill_base_path)
        .map_err(|e| CliError::SkillValidationFailed(e.to_string()))?;
    let skill_def = super::skill_def::create_skill_from_path(&skill_path, "git", false)?;
    Ok((
        temp_dir,
        skill_path,
        skill_def,
        branch.map(String::from),
        tag.map(String::from),
    ))
}

pub(super) async fn add_from_git(
    ctx: &super::AddContext<'_>,
    git_url: &str,
    branch: Option<&str>,
    tag: Option<&str>,
) -> CliResult<()> {
    info!("Adding skill from git URL: {}", git_url);
    let (_temp_dir, skill_path, skill_def, branch_opt, tag_opt) =
        clone_and_validate_skill(git_url, branch, tag).await?;
    validate_skill_structure(&skill_path)?;
    let git_info = parse_git_url(git_url)?;
    let target = super::InstallTarget {
        storage_dir: ctx
            .service
            .config()
            .skill_storage_path
            .join(skill_def.id.as_str()),
        meta: super::SourceMeta {
            source_url: Some(git_url.to_string()),
            source_type: Some(SourceType::GitUrl),
            source_branch: branch_opt.or(git_info.branch),
            source_tag: tag_opt,
            source_subdir: git_info.subdir,
            installed_from: None,
        },
        version_display: skill_def.version.clone(),
    };
    super::install::install_via_download(ctx, &skill_path, skill_def, target).await
}

pub(super) fn parse_registry_scope_id(
    skill_id_input: &str,
) -> CliResult<(String, String, String, Option<String>)> {
    use crate::utils::parse_skill_id;
    use fastskill_core::security::path::validate_path_component;
    let (skill_id_full, version_opt) = parse_skill_id(skill_id_input);
    let (scope, expected_id) = match skill_id_full.find('/') {
        Some(slash_pos) => (
            skill_id_full[..slash_pos].to_string(),
            skill_id_full[slash_pos + 1..].to_string(),
        ),
        None => {
            return Err(CliError::Config(format!(
                "Registry skill ID must be in format 'scope/id', got: {}",
                skill_id_full
            )));
        }
    };
    // `scope` (and `id`) are joined into the storage path, so reject traversal
    // (`..`, `/`, `\`, absolute) before use — a crafted scope like `..` would
    // otherwise redirect the install one directory above the storage root.
    validate_path_component(&scope)
        .map_err(|e| CliError::Config(format!("Invalid registry scope '{}': {}", scope, e)))?;
    validate_path_component(&expected_id).map_err(|e| {
        CliError::Config(format!(
            "Invalid registry skill id '{}': {}",
            expected_id, e
        ))
    })?;
    Ok((skill_id_full, scope, expected_id, version_opt))
}

/// Select the newest version by semver, not lexical string order.
///
/// A plain string sort ranks `"1.9.0"` above `"1.10.0"`, which would install the
/// wrong version. Unparseable versions rank lowest, mirroring the registry's
/// `get_latest_version`. Returns `None` for an empty input.
fn select_newest_version(versions: &[String]) -> Option<String> {
    let mut sorted = versions.to_vec();
    sorted.sort_by(|a, b| {
        semver::Version::parse(a)
            .ok()
            .cmp(&semver::Version::parse(b).ok())
    });
    sorted.last().cloned()
}

pub(super) async fn resolve_registry_version(
    repo_client: &(dyn fastskill_core::core::repository::RepositoryClient + Send + Sync),
    skill_id_full: &str,
    version_opt: Option<String>,
) -> CliResult<String> {
    if let Some(v) = version_opt {
        return Ok(v);
    }
    let versions = repo_client
        .get_versions(skill_id_full)
        .await
        .map_err(|e| CliError::Config(format!("Failed to get versions: {}", e)))?;
    if versions.is_empty() {
        return Err(CliError::Config(format!(
            "No versions found for skill '{}' in repository",
            skill_id_full
        )));
    }
    select_newest_version(&versions)
        .ok_or_else(|| CliError::Config("No versions available".to_string()))
}

async fn download_registry_package(
    repo_client: &(dyn fastskill_core::core::repository::RepositoryClient + Send + Sync),
    skill_id_full: &str,
    version: &str,
) -> CliResult<(TempDir, PathBuf, fastskill_core::SkillDefinition)> {
    let zip_data = repo_client
        .download(skill_id_full, version)
        .await
        .map_err(|e| CliError::Config(format!("Failed to download package: {}", e)))?;
    let temp_dir = TempDir::new().map_err(CliError::Io)?;
    let extract_path = temp_dir.path().join("extracted");
    fs::create_dir_all(&extract_path).map_err(CliError::Io)?;
    let temp_zip = temp_dir.path().join(format!("package-{}.zip", version));
    std::fs::write(&temp_zip, zip_data).map_err(CliError::Io)?;
    use fastskill_core::storage::zip::ZipHandler;
    let zip_handler = ZipHandler::new()
        .map_err(|e| CliError::InvalidSource(format!("Failed to create ZIP handler: {}", e)))?;
    zip_handler
        .extract_to_dir(&temp_zip, &extract_path)
        .map_err(|e| match e {
            fastskill_core::core::service::ServiceError::Validation(msg) => {
                CliError::InvalidSource(format!("ZIP extraction validation failed: {}", msg))
            }
            fastskill_core::core::service::ServiceError::Io(err) => CliError::Io(err),
            _ => CliError::InvalidSource(format!("ZIP extraction failed: {}", e)),
        })?;
    let skill_path = find_skill_in_directory(&extract_path)?;
    validate_skill_structure(&skill_path)?;
    let skill_def = super::skill_def::create_skill_from_path(&skill_path, "registry", false)?;
    Ok((temp_dir, skill_path, skill_def))
}

pub(super) async fn add_from_registry(
    ctx: &super::AddContext<'_>,
    skill_id_input: &str,
) -> CliResult<()> {
    info!("Adding skill from registry: {}", skill_id_input);
    let (skill_id_full, scope, expected_id, version_opt) = parse_registry_scope_id(skill_id_input)?;

    let repositories = crate::config::load_repositories_from_project()?;
    let repo_manager =
        fastskill_core::core::repository::RepositoryManager::from_definitions(repositories);
    let default_repo = repo_manager.get_default_repository().ok_or_else(|| {
        CliError::Config(
            "No default repository configured. Use 'fastskill repos add' to add a repository."
                .to_string(),
        )
    })?;
    if !matches!(
        default_repo.repo_type,
        fastskill_core::core::repository::RepositoryType::HttpRegistry
    ) {
        return Err(CliError::Config(format!(
            "Repository '{}' is not an http-registry type. Only http-registry repositories are supported for 'fastskill add' currently.",
            default_repo.name
        )));
    }

    let repo_client = repo_manager
        .get_client(&default_repo.name)
        .await
        .map_err(|e| CliError::Config(format!("Failed to get repository client: {}", e)))?;
    let version =
        resolve_registry_version(repo_client.as_ref(), &skill_id_full, version_opt).await?;

    let _skill_metadata = repo_client
        .get_skill(&skill_id_full, Some(&version))
        .await
        .map_err(|e| CliError::Config(format!("Failed to get skill: {}", e)))?
        .ok_or_else(|| {
            CliError::Config(format!(
                "Skill '{}' version '{}' not found in repository",
                skill_id_full, version
            ))
        })?;

    info!(
        "Downloading {}@{} from repository...",
        skill_id_full, version
    );
    let (_temp_dir, skill_path, skill_def) =
        download_registry_package(repo_client.as_ref(), &skill_id_full, &version).await?;

    if skill_def.id.as_str() != expected_id {
        return Err(CliError::Config(format!(
            "Skill ID mismatch: expected '{}' from registry reference '{}', but found '{}' in skill-project.toml",
            expected_id, skill_id_full, skill_def.id.as_str()
        )));
    }

    let target = super::InstallTarget {
        storage_dir: ctx
            .service
            .config()
            .skill_storage_path
            .join(&scope)
            .join(skill_def.id.as_str()),
        meta: super::SourceMeta {
            source_url: None,
            source_type: Some(SourceType::Source),
            source_branch: None,
            source_tag: None,
            source_subdir: None,
            installed_from: Some(default_repo.name.clone()),
        },
        version_display: version,
    };
    super::install::install_via_download(ctx, &skill_path, skill_def, target).await
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_registry_scope_id_rejects_dotdot_scope() {
        // SEC-6: a `..` scope would redirect the install above the storage root.
        let result = parse_registry_scope_id("../evilskill");
        assert!(result.is_err(), "'..' scope must be rejected");
        if let Err(CliError::Config(msg)) = result {
            assert!(
                msg.contains("scope"),
                "error should mention the invalid scope, got: {}",
                msg
            );
        } else {
            panic!("Expected CliError::Config for traversal scope");
        }
    }

    #[test]
    fn test_parse_registry_scope_id_rejects_backslash_scope() {
        // A backslash is a traversal character on Windows-style paths.
        assert!(parse_registry_scope_id("bad\\scope/id").is_err());
    }

    #[test]
    fn test_select_newest_version_semver_ordering() {
        // BUG-10: string sort would pick "1.9.0"; semver must pick "1.10.0".
        let versions = vec![
            "1.9.0".to_string(),
            "1.10.0".to_string(),
            "1.2.0".to_string(),
        ];
        assert_eq!(select_newest_version(&versions), Some("1.10.0".to_string()));
    }

    #[test]
    fn test_select_newest_version_prefers_parseable() {
        let versions = vec!["not-semver".to_string(), "0.1.0".to_string()];
        assert_eq!(select_newest_version(&versions), Some("0.1.0".to_string()));
    }

    #[test]
    fn test_select_newest_version_empty() {
        assert_eq!(select_newest_version(&[]), None);
    }

    #[test]
    fn test_safe_subdir_join_rejects_dotdot() {
        // SEC-5: a `..`-laden subdir must be rejected before any filesystem use.
        let root = tempfile::tempdir().unwrap();
        let result = safe_subdir_join(root.path(), Path::new("../../etc"));
        assert!(result.is_err(), "'..' subdir must be rejected");
        assert!(matches!(result, Err(CliError::InvalidSource(_))));
    }

    #[test]
    fn test_safe_subdir_join_rejects_absolute() {
        let root = tempfile::tempdir().unwrap();
        let result = safe_subdir_join(root.path(), Path::new("/etc/passwd"));
        assert!(matches!(result, Err(CliError::InvalidSource(_))));
    }

    #[test]
    fn test_safe_subdir_join_accepts_nested_relative() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("a/b")).unwrap();
        let joined = safe_subdir_join(root.path(), Path::new("a/b")).unwrap();
        assert_eq!(joined, root.path().join("a").join("b"));
    }

    #[test]
    fn test_resolve_invalid_source_type_returns_error() {
        let result = resolve_source_type("/some/path", Some("invalid-type"));
        assert!(
            result.is_err(),
            "invalid source type must return CliError::Config"
        );
        if let Err(CliError::Config(msg)) = result {
            assert!(
                msg.contains("invalid-type"),
                "error message must mention the invalid value"
            );
        } else {
            panic!("Expected CliError::Config");
        }
    }

    #[test]
    fn test_resolve_registry_source_type() {
        let result = resolve_source_type("my-skill@1.0.0", Some("registry")).unwrap();
        assert!(matches!(result, ResolvedSource::Registry(_)));
    }

    #[test]
    fn test_resolve_git_source_type() {
        let result = resolve_source_type("https://github.com/org/skill.git", Some("git")).unwrap();
        assert!(matches!(result, ResolvedSource::Git(_)));
    }

    #[test]
    fn test_resolve_local_source_type_directory() {
        let result = resolve_source_type("/some/local/path", Some("local")).unwrap();
        assert!(matches!(result, ResolvedSource::Local(_)));
    }

    #[test]
    fn test_resolve_local_source_type_zip() {
        let result = resolve_source_type("/some/skill.zip", Some("local")).unwrap();
        assert!(matches!(result, ResolvedSource::Zip(_)));
    }

    #[test]
    fn test_resolve_no_source_type_autodetects() {
        let result = resolve_source_type("my-skill@1.0.0", None).unwrap();
        assert!(matches!(result, ResolvedSource::Registry(_)));
    }

    #[test]
    fn test_parse_registry_scope_id_valid() {
        let (full, scope, id, version) = parse_registry_scope_id("my-scope/my-skill").unwrap();
        assert_eq!(full, "my-scope/my-skill");
        assert_eq!(scope, "my-scope");
        assert_eq!(id, "my-skill");
        assert!(version.is_none());
    }

    #[test]
    fn test_parse_registry_scope_id_with_version() {
        let (full, _scope, _id, version) =
            parse_registry_scope_id("my-scope/my-skill@1.2.3").unwrap();
        assert_eq!(full, "my-scope/my-skill");
        assert_eq!(version, Some("1.2.3".to_string()));
    }

    #[test]
    fn test_parse_registry_scope_id_no_scope_fails() {
        let result = parse_registry_scope_id("no-scope-skill");
        assert!(result.is_err());
        if let Err(CliError::Config(msg)) = result {
            assert!(msg.contains("scope/id"));
        } else {
            panic!("Expected CliError::Config");
        }
    }

    // ── is_local_path ─────────────────────────────────────────────────────────

    #[test]
    fn test_is_local_path_dot_and_slash_prefixes() {
        assert!(is_local_path("./relative"));
        assert!(is_local_path("/absolute/path"));
        assert!(is_local_path("../up"));
    }

    #[test]
    fn test_is_local_path_existing_path() {
        let tmp = tempfile::TempDir::new().unwrap();
        let p = tmp.path().join("thing");
        std::fs::write(&p, "x").unwrap();
        assert!(is_local_path(&p.to_string_lossy()));
    }

    #[test]
    fn test_is_local_path_bare_name_is_not_local() {
        // A bare, non-existent name is treated as a non-local (e.g. registry) ref.
        assert!(!is_local_path("some-skill-name-that-does-not-exist"));
    }

    // ── autodetect_source ─────────────────────────────────────────────────────

    #[test]
    fn test_autodetect_source_registry() {
        assert!(matches!(
            autodetect_source("scope/skill@1.0.0"),
            ResolvedSource::Registry(_)
        ));
    }

    #[test]
    fn test_autodetect_source_git() {
        assert!(matches!(
            autodetect_source("https://github.com/org/repo.git"),
            ResolvedSource::Git(_)
        ));
    }

    #[test]
    fn test_autodetect_source_zip() {
        assert!(matches!(
            autodetect_source("./bundle.zip"),
            ResolvedSource::Zip(_)
        ));
    }

    #[test]
    fn test_autodetect_source_folder() {
        assert!(matches!(
            autodetect_source("./some/folder"),
            ResolvedSource::Local(_)
        ));
    }

    // ── find_skill_in_directory ───────────────────────────────────────────────

    #[test]
    fn test_find_skill_in_directory_root() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(tmp.path().join("SKILL.md"), "# skill\n").unwrap();
        let found = find_skill_in_directory(tmp.path()).unwrap();
        assert_eq!(found, tmp.path());
    }

    #[test]
    fn test_find_skill_in_directory_subdir() {
        let tmp = tempfile::TempDir::new().unwrap();
        let sub = tmp.path().join("my-skill");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join("SKILL.md"), "# skill\n").unwrap();
        let found = find_skill_in_directory(tmp.path()).unwrap();
        assert_eq!(found, sub);
    }

    #[test]
    fn test_find_skill_in_directory_not_found() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("empty")).unwrap();
        let result = find_skill_in_directory(tmp.path());
        assert!(matches!(result, Err(CliError::Validation(_))));
    }

    // ── extract_zip_to_temp ───────────────────────────────────────────────────

    /// Build an in-memory zip containing the given (path, contents) entries.
    fn build_zip(entries: &[(&str, &str)]) -> Vec<u8> {
        use std::io::Write;
        use zip::write::FileOptions;
        let mut buf = Vec::new();
        {
            let cursor = std::io::Cursor::new(&mut buf);
            let mut writer = zip::ZipWriter::new(cursor);
            let opts = FileOptions::default().compression_method(zip::CompressionMethod::Stored);
            for (name, contents) in entries {
                writer.start_file(*name, opts).unwrap();
                writer.write_all(contents.as_bytes()).unwrap();
            }
            writer.finish().unwrap();
        }
        buf
    }

    #[test]
    fn test_extract_zip_to_temp_success() {
        let tmp = tempfile::TempDir::new().unwrap();
        let zip_bytes = build_zip(&[("test-skill/SKILL.md", "# skill\n")]);
        let zip_path = tmp.path().join("pkg.zip");
        std::fs::write(&zip_path, &zip_bytes).unwrap();

        let extracted = extract_zip_to_temp(&zip_path).unwrap();
        assert!(extracted.path().join("test-skill/SKILL.md").exists());
    }

    #[test]
    fn test_extract_zip_to_temp_invalid_zip() {
        let tmp = tempfile::TempDir::new().unwrap();
        let zip_path = tmp.path().join("bad.zip");
        std::fs::write(&zip_path, b"not a real zip file").unwrap();
        let result = extract_zip_to_temp(&zip_path);
        assert!(matches!(result, Err(CliError::InvalidSource(_))));
    }

    // ── resolve_registry_version / download_registry_package (mock client) ─────

    /// Minimal in-memory RepositoryClient for exercising version resolution and
    /// package download without touching the network.
    struct MockRepoClient {
        versions: Vec<String>,
        zip_data: Vec<u8>,
    }

    #[async_trait::async_trait]
    impl fastskill_core::core::repository::RepositoryClient for MockRepoClient {
        async fn list_skills(
            &self,
        ) -> Result<
            Vec<fastskill_core::core::metadata::SkillMetadata>,
            fastskill_core::core::repository::RepositoryClientError,
        > {
            Ok(Vec::new())
        }

        async fn get_skill(
            &self,
            _id: &str,
            _version: Option<&str>,
        ) -> Result<
            Option<fastskill_core::core::metadata::SkillMetadata>,
            fastskill_core::core::repository::RepositoryClientError,
        > {
            Ok(None)
        }

        async fn search(
            &self,
            _query: &str,
        ) -> Result<
            Vec<fastskill_core::core::metadata::SkillMetadata>,
            fastskill_core::core::repository::RepositoryClientError,
        > {
            Ok(Vec::new())
        }

        async fn download(
            &self,
            _id: &str,
            _version: &str,
        ) -> Result<Vec<u8>, fastskill_core::core::repository::RepositoryClientError> {
            Ok(self.zip_data.clone())
        }

        async fn get_versions(
            &self,
            _id: &str,
        ) -> Result<Vec<String>, fastskill_core::core::repository::RepositoryClientError> {
            Ok(self.versions.clone())
        }
    }

    #[tokio::test]
    async fn test_resolve_registry_version_explicit_version_short_circuits() {
        let client = MockRepoClient {
            versions: vec!["9.9.9".to_string()],
            zip_data: Vec::new(),
        };
        let v = resolve_registry_version(&client, "scope/skill", Some("1.2.3".to_string()))
            .await
            .unwrap();
        assert_eq!(v, "1.2.3");
    }

    #[tokio::test]
    async fn test_resolve_registry_version_picks_newest_semver() {
        let client = MockRepoClient {
            versions: vec![
                "1.9.0".to_string(),
                "1.10.0".to_string(),
                "1.2.0".to_string(),
            ],
            zip_data: Vec::new(),
        };
        let v = resolve_registry_version(&client, "scope/skill", None)
            .await
            .unwrap();
        assert_eq!(v, "1.10.0");
    }

    #[tokio::test]
    async fn test_resolve_registry_version_empty_errors() {
        let client = MockRepoClient {
            versions: Vec::new(),
            zip_data: Vec::new(),
        };
        let result = resolve_registry_version(&client, "scope/skill", None).await;
        assert!(matches!(result, Err(CliError::Config(_))));
    }

    #[tokio::test]
    async fn test_download_registry_package_success() {
        let zip_bytes = build_zip(&[(
            "test-skill/SKILL.md",
            "---\nname: test-skill\nversion: \"1.0.0\"\ndescription: A downloaded test skill\n---\n",
        )]);
        let client = MockRepoClient {
            versions: vec!["1.0.0".to_string()],
            zip_data: zip_bytes,
        };
        let (_temp_dir, skill_path, skill_def) =
            download_registry_package(&client, "scope/test-skill", "1.0.0")
                .await
                .unwrap();
        assert!(skill_path.join("SKILL.md").exists());
        assert_eq!(skill_def.id.as_str(), "test-skill");
    }

    #[tokio::test]
    async fn test_download_registry_package_invalid_zip() {
        let client = MockRepoClient {
            versions: vec!["1.0.0".to_string()],
            zip_data: b"garbage".to_vec(),
        };
        let result = download_registry_package(&client, "scope/test-skill", "1.0.0").await;
        assert!(result.is_err());
    }

    // ── add_from_zip end-to-end (real service + manifest) ─────────────────────

    const VALID_SKILL_MD: &str =
        "---\nname: test-skill\nversion: \"1.0.0\"\ndescription: A zip-installed test skill\n---\nBody\n";

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn test_add_from_zip_end_to_end() {
        use fastskill_core::{FastSkillService, ServiceConfig};

        let _lock = fastskill_core::test_utils::DIR_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());

        struct DirGuard(Option<PathBuf>);
        impl Drop for DirGuard {
            fn drop(&mut self) {
                if let Some(dir) = &self.0 {
                    let _ = std::env::set_current_dir(dir);
                }
            }
        }
        let _guard = DirGuard(std::env::current_dir().ok());

        let tmp = tempfile::TempDir::new().unwrap();
        std::env::set_current_dir(tmp.path()).unwrap();

        std::fs::write(
            tmp.path().join("skill-project.toml"),
            "[tool.fastskill]\nskills_directory = \".claude/skills\"\n\n[dependencies]\n",
        )
        .unwrap();
        let skills_dir = tmp.path().join(".claude/skills");
        std::fs::create_dir_all(&skills_dir).unwrap();

        let zip_path = tmp.path().join("pkg.zip");
        std::fs::write(
            &zip_path,
            build_zip(&[("test-skill/SKILL.md", VALID_SKILL_MD)]),
        )
        .unwrap();

        let config = ServiceConfig {
            skill_storage_path: skills_dir.clone(),
            ..Default::default()
        };
        let mut service = FastSkillService::new(config).await.unwrap();
        service.initialize().await.unwrap();

        let ctx = crate::commands::add::AddContext {
            service: &service,
            force: false,
            editable: false,
            groups: Vec::new(),
            global: false,
        };
        let result = add_from_zip(&ctx, &zip_path).await;
        assert!(result.is_ok(), "add_from_zip should succeed: {:?}", result);
        assert!(skills_dir.join("test-skill").join("SKILL.md").exists());
    }
}
