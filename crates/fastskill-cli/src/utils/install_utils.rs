//! Utilities for installing skills from various sources

use crate::error::{CliError, CliResult};
use chrono::Utc;
use fastskill_core::core::repository::RepositoryManager;
use fastskill_core::core::sources::SourcesManager;
use fastskill_core::core::{
    manifest::SkillEntry, manifest::SkillSource, skill_manager::SourceType,
};
use fastskill_core::{FastSkillService, SkillDefinition};
use std::path::{Path, PathBuf};

/// Safely join an untrusted `subdir` (from a shareable manifest / git tree URL)
/// onto a trusted clone `root`, rejecting path traversal.
///
/// Each path component is validated via `fastskill_core::security::path::validate_path_component`
/// (rejecting `..`, `/`, `\`, and absolute components). After joining, if the target exists it is
/// canonicalized and asserted to stay within the canonicalized `root`, so a `subdir` escaping the
/// clone (via `..` or a symlink) is rejected with `CliError::InvalidSource`.
pub(crate) fn safe_subdir_join(root: &Path, subdir: &Path) -> CliResult<PathBuf> {
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
            // `..`, root (`/`), prefix (`C:`), etc. are all traversal / absolute markers.
            _ => {
                return Err(CliError::InvalidSource(format!(
                    "Subdirectory '{}' must be a relative path without '..' components",
                    subdir.display()
                )));
            }
        }
    }

    // If the join resolves to an existing path, canonicalize both sides and assert containment
    // (defends against symlinks inside the clone pointing outside it).
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

/// Create a symlink for editable installs
/// This is platform-specific: Unix uses tokio::fs::symlink, Windows uses std::os::windows::fs::symlink_dir
/// On unsupported platforms, returns EditableNotSupported error
async fn create_editable_symlink(src: &Path, dst: &Path) -> CliResult<()> {
    #[cfg(unix)]
    {
        tokio::fs::symlink(src, dst).await.map_err(CliError::Io)
    }

    #[cfg(windows)]
    {
        use std::os::windows::fs::symlink_dir;
        symlink_dir(src, dst).map_err(|e| {
            CliError::Io(std::io::Error::new(
                e.kind(),
                format!(
                    "Windows symlink permission denied: {}. Enable Developer Mode \
                     (Settings → For developers) or run as Administrator.",
                    e
                ),
            ))
        })
    }

    #[cfg(all(not(unix), not(windows)))]
    {
        Err(CliError::Io(std::io::Error::other(
            "Editable installations are not supported on this platform. Use a Unix-based system or Docker.",
        )))
    }
}

/// Install a single skill from a SkillEntry
pub async fn install_skill_from_entry(
    service: &FastSkillService,
    entry: SkillEntry,
    sources_manager: Option<&SourcesManager>,
) -> CliResult<SkillDefinition> {
    match &entry.source {
        SkillSource::Git {
            url,
            branch,
            tag,
            subdir,
        } => {
            install_from_git(
                service,
                url,
                branch.as_deref(),
                tag.as_deref(),
                subdir.as_ref(),
            )
            .await
        }
        SkillSource::Local { path, editable } => install_from_local(service, path, *editable).await,
        SkillSource::ZipUrl { base_url, version } => {
            install_from_zip_url(service, base_url, version.as_deref()).await
        }
        SkillSource::Source {
            name,
            skill,
            version,
        } => {
            if let Some(sources_mgr) = sources_manager {
                install_from_source(service, sources_mgr, name, skill, version.as_deref()).await
            } else {
                Err(CliError::Config(
                    "Sources manager required for source-based installation".to_string(),
                ))
            }
        }
    }
}

/// Configuration for git-based skill installation
struct GitInstallConfig<'a> {
    url: &'a str,
    branch: Option<&'a str>,
    tag: Option<&'a str>,
    subdir: Option<&'a PathBuf>,
}

/// Clone git repository and determine skill path
async fn clone_and_find_skill(
    config: &GitInstallConfig<'_>,
) -> CliResult<(tempfile::TempDir, PathBuf)> {
    use crate::utils::parse_git_url;
    use fastskill_core::storage::git::{clone_repository, validate_cloned_skill};

    let git_info = parse_git_url(config.url)?;
    let branch = config.branch.or(git_info.branch.as_deref());
    let temp_dir = clone_repository(&git_info.repo_url, branch, config.tag, None)
        .await
        .map_err(|e| CliError::GitCloneFailed(e.to_string()))?;

    let skill_base_path = if let Some(subdir) = config.subdir.or(git_info.subdir.as_ref()) {
        let subdir_path = safe_subdir_join(temp_dir.path(), subdir)?;
        if !subdir_path.exists() {
            return Err(CliError::InvalidSource(format!(
                "Specified subdirectory '{}' does not exist",
                subdir.display()
            )));
        }
        subdir_path
    } else {
        temp_dir.path().to_path_buf()
    };

    let skill_path = validate_cloned_skill(&skill_base_path)
        .map_err(|e| CliError::SkillValidationFailed(e.to_string()))?;

    Ok((temp_dir, skill_path))
}

/// Copy skill to storage and update metadata
async fn copy_skill_to_storage(
    service: &FastSkillService,
    skill_path: &Path,
    skill_def: &mut SkillDefinition,
) -> CliResult<()> {
    use crate::commands::add::copy_dir_recursive;

    let skill_storage_dir = service
        .config()
        .skill_storage_path
        .join(skill_def.id.as_str());

    tokio::fs::create_dir_all(&skill_storage_dir)
        .await
        .map_err(CliError::Io)?;
    copy_dir_recursive(skill_path, &skill_storage_dir).await?;

    skill_def.skill_file = skill_storage_dir.join("SKILL.md");
    Ok(())
}

async fn register_skill(
    service: &FastSkillService,
    skill_def: &SkillDefinition,
    update: fastskill_core::core::skill_manager::SkillUpdate,
) -> CliResult<()> {
    service
        .skill_manager()
        .force_register_skill(skill_def.clone())
        .await
        .map_err(CliError::Service)?;

    service
        .skill_manager()
        .update_skill(&skill_def.id, update)
        .await
        .map_err(|e| {
            super::service_error_to_cli(e, service.config().skill_storage_path.as_path(), false)
        })?;

    Ok(())
}

fn base_skill_update(def: &SkillDefinition) -> fastskill_core::core::skill_manager::SkillUpdate {
    fastskill_core::core::skill_manager::SkillUpdate {
        source_url: def.source_url.clone(),
        source_type: def.source_type.clone(),
        fetched_at: def.fetched_at,
        ..Default::default()
    }
}

async fn install_from_git(
    service: &FastSkillService,
    url: &str,
    branch: Option<&str>,
    tag: Option<&str>,
    subdir: Option<&PathBuf>,
) -> CliResult<SkillDefinition> {
    use crate::commands::add::create_skill_from_path;

    let config = GitInstallConfig {
        url,
        branch,
        tag,
        subdir,
    };

    let (_temp_dir, skill_path) = clone_and_find_skill(&config).await?;
    let mut skill_def = create_skill_from_path(&skill_path, "git", false)?;

    copy_skill_to_storage(service, &skill_path, &mut skill_def).await?;

    skill_def.source_url = Some(url.to_string());
    skill_def.source_type = Some(SourceType::GitUrl);
    skill_def.source_branch = branch.map(|s| s.to_string());
    skill_def.source_tag = tag.map(|s| s.to_string());
    skill_def.source_subdir = subdir.cloned();
    skill_def.fetched_at = Some(Utc::now());
    skill_def.editable = false;

    register_skill(
        service,
        &skill_def,
        fastskill_core::core::skill_manager::SkillUpdate {
            source_branch: skill_def.source_branch.clone(),
            source_tag: skill_def.source_tag.clone(),
            source_subdir: skill_def.source_subdir.clone(),
            ..base_skill_update(&skill_def)
        },
    )
    .await?;

    Ok(skill_def)
}

/// Setup skill in storage directory (editable or copy)
pub(crate) async fn setup_skill_in_storage(
    skill_path: &Path,
    skill_storage_dir: &Path,
    editable: bool,
) -> CliResult<()> {
    use crate::commands::add::copy_dir_recursive;

    if skill_storage_dir.exists() {
        tokio::fs::remove_dir_all(skill_storage_dir)
            .await
            .map_err(CliError::Io)?;
    }

    if editable {
        create_editable_symlink(skill_path, skill_storage_dir).await?;
    } else {
        copy_dir_recursive(skill_path, skill_storage_dir).await?;
    }

    Ok(())
}

async fn install_from_local(
    service: &FastSkillService,
    path: &PathBuf,
    editable: bool,
) -> CliResult<SkillDefinition> {
    use crate::commands::add::create_skill_from_path;

    let skill_path = if path.is_absolute() {
        path.clone()
    } else {
        std::env::current_dir().map_err(CliError::Io)?.join(path)
    };
    if !skill_path.exists() {
        return Err(CliError::InvalidSource(format!(
            "Local path does not exist: {}",
            skill_path.display()
        )));
    }

    let mut skill_def = create_skill_from_path(&skill_path, "local", editable)?;
    let skill_storage_dir = service
        .config()
        .skill_storage_path
        .join(skill_def.id.as_str());

    setup_skill_in_storage(&skill_path, &skill_storage_dir, editable).await?;

    skill_def.skill_file = skill_storage_dir.join("SKILL.md");
    skill_def.source_url = Some(skill_path.to_string_lossy().to_string());
    skill_def.source_type = Some(SourceType::LocalPath);
    skill_def.fetched_at = Some(Utc::now());
    skill_def.editable = editable;

    register_skill(
        service,
        &skill_def,
        fastskill_core::core::skill_manager::SkillUpdate {
            editable: Some(skill_def.editable),
            ..base_skill_update(&skill_def)
        },
    )
    .await?;

    Ok(skill_def)
}

/// Extract a ZIP archive at `zip_path` into `dst` with the SEC-3-capped `ZipHandler`,
/// mapping `ServiceError` onto the appropriate `CliError`.
pub(crate) fn extract_zip(zip_path: &std::path::Path, dst: &std::path::Path) -> CliResult<()> {
    use fastskill_core::storage::zip::ZipHandler;
    let zip_handler = ZipHandler::new()
        .map_err(|e| CliError::InvalidSource(format!("Failed to create ZIP handler: {}", e)))?;
    zip_handler
        .extract_to_dir(zip_path, dst)
        .map_err(|e| match e {
            fastskill_core::core::service::ServiceError::Validation(msg) => {
                CliError::InvalidSource(format!("ZIP extraction validation failed: {}", msg))
            }
            fastskill_core::core::service::ServiceError::Io(err) => CliError::Io(err),
            _ => CliError::InvalidSource(format!("ZIP extraction failed: {}", e)),
        })
}

/// Download an archive from `url` into a fresh temp dir and extract it with the
/// SEC-3-capped `ZipHandler`, returning the temp dir (kept alive by the caller) and
/// the directory that contains `SKILL.md`.
async fn download_and_extract_zip(url: &str) -> CliResult<(tempfile::TempDir, PathBuf)> {
    use fastskill_core::storage::git::validate_cloned_skill;

    let response = reqwest::get(url)
        .await
        .map_err(|e| CliError::InvalidSource(format!("Failed to download '{}': {}", url, e)))?
        .error_for_status()
        .map_err(|e| CliError::InvalidSource(format!("Failed to download '{}': {}", url, e)))?;
    let bytes = response
        .bytes()
        .await
        .map_err(|e| CliError::InvalidSource(format!("Failed to read '{}': {}", url, e)))?;

    let temp_dir = tempfile::Builder::new()
        .prefix("fastskill-zip-")
        .tempdir()
        .map_err(CliError::Io)?;
    let zip_path = temp_dir.path().join("package.zip");
    let extract_path = temp_dir.path().join("extracted");
    tokio::fs::write(&zip_path, &bytes)
        .await
        .map_err(CliError::Io)?;
    tokio::fs::create_dir_all(&extract_path)
        .await
        .map_err(CliError::Io)?;

    extract_zip(&zip_path, &extract_path)?;

    let skill_path = validate_cloned_skill(&extract_path)
        .map_err(|e| CliError::SkillValidationFailed(e.to_string()))?;

    Ok((temp_dir, skill_path))
}

async fn install_from_zip_url(
    service: &FastSkillService,
    base_url: &str,
    version: Option<&str>,
) -> CliResult<SkillDefinition> {
    use crate::commands::add::create_skill_from_path;

    let (_temp_dir, skill_path) = download_and_extract_zip(base_url).await?;

    let mut skill_def = create_skill_from_path(&skill_path, "zip", false)?;
    copy_skill_to_storage(service, &skill_path, &mut skill_def).await?;

    skill_def.source_url = Some(base_url.to_string());
    skill_def.source_type = Some(SourceType::ZipFile);
    skill_def.source_tag = version.map(|s| s.to_string());
    skill_def.fetched_at = Some(Utc::now());
    skill_def.editable = false;

    register_skill(
        service,
        &skill_def,
        fastskill_core::core::skill_manager::SkillUpdate {
            source_tag: skill_def.source_tag.clone(),
            ..base_skill_update(&skill_def)
        },
    )
    .await?;

    Ok(skill_def)
}

/// Create and initialize package resolver
async fn create_package_resolver() -> CliResult<fastskill_core::core::resolver::PackageResolver> {
    use fastskill_core::core::resolver::PackageResolver;
    use std::sync::Arc;

    let repositories = crate::config::load_repositories_from_project()?;
    let repo_manager =
        fastskill_core::core::repository::RepositoryManager::from_definitions(repositories);

    let sources_mgr = if let Some(sources_manager) =
        create_sources_manager_from_repositories(&repo_manager)
            .map_err(|e| CliError::Config(format!("Failed to create sources manager: {}", e)))?
    {
        Arc::new(sources_manager)
    } else {
        return Err(CliError::Config(
            "No marketplace-based repositories found. PackageResolver requires at least one marketplace repository.".to_string(),
        ));
    };

    let mut resolver = PackageResolver::new(sources_mgr.clone());
    resolver
        .build_index()
        .await
        .map_err(|e| CliError::Config(format!("Failed to build resolver index: {}", e)))?;

    Ok(resolver)
}

/// Install skill based on resolved source configuration
async fn install_from_resolved_source(
    service: &FastSkillService,
    source_config: &fastskill_core::core::sources::SourceConfig,
    version: Option<&str>,
) -> CliResult<SkillDefinition> {
    match source_config {
        fastskill_core::core::sources::SourceConfig::Git {
            url,
            branch,
            tag,
            auth: _auth,
        } => install_from_git(service, url, branch.as_deref(), tag.as_deref(), None).await,
        fastskill_core::core::sources::SourceConfig::Local { path } => {
            install_from_local(service, path, false).await
        }
        fastskill_core::core::sources::SourceConfig::ZipUrl { base_url, .. } => {
            install_from_zip_url(service, base_url, version).await
        }
    }
}

async fn install_from_source(
    service: &FastSkillService,
    _sources_manager: &SourcesManager,
    source_name: &str,
    skill_name: &str,
    version: Option<&str>,
) -> CliResult<SkillDefinition> {
    use fastskill_core::core::resolver::ConflictStrategy;

    let resolver = create_package_resolver().await?;
    let version_constraint = version
        .map(|v| {
            fastskill_core::core::version::VersionConstraint::parse(v)
                .map_err(|e| CliError::Config(format!("Invalid version constraint '{}': {}", v, e)))
        })
        .transpose()?;
    let resolution = resolver
        .resolve_skill(
            skill_name,
            version_constraint.as_ref(),
            Some(source_name),
            ConflictStrategy::Priority,
        )
        .map_err(|e| {
            CliError::Config(format!("Failed to resolve skill '{}': {}", skill_name, e))
        })?;

    install_from_resolved_source(service, &resolution.candidate.source_config, version).await
}

/// Create SourcesManager from RepositoryManager for marketplace-based repositories
/// This is needed because PackageResolver requires SourcesManager
pub fn create_sources_manager_from_repositories(
    repo_manager: &RepositoryManager,
) -> CliResult<Option<SourcesManager>> {
    SourcesManager::from_repositories(repo_manager)
        .map_err(|e| CliError::Config(format!("Failed to create sources manager: {}", e)))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn test_safe_subdir_join_rejects_dotdot() {
        // SEC-5: an untrusted manifest `subdir` with `..` must be rejected.
        let root = tempfile::tempdir().unwrap();
        let result = safe_subdir_join(root.path(), Path::new("../../../etc"));
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
        std::fs::create_dir_all(root.path().join("skills/inner")).unwrap();
        let joined = safe_subdir_join(root.path(), Path::new("skills/inner")).unwrap();
        assert_eq!(joined, root.path().join("skills").join("inner"));
        // Canonicalized containment check must pass for a legitimate nested dir.
        assert!(joined.starts_with(root.path()));
    }

    #[tokio::test]
    async fn test_download_and_extract_zip_download_failure() {
        // PARTIAL-3: an unreachable URL surfaces as a clean InvalidSource error,
        // not a panic. Port 1 reliably refuses connections offline.
        let result = download_and_extract_zip("http://127.0.0.1:1/nope.zip").await;
        assert!(matches!(result, Err(CliError::InvalidSource(_))));
    }

    // ── shared test helpers ───────────────────────────────────────────────────

    const VALID_SKILL_MD: &str = "---\nname: test-skill\nversion: \"1.0.0\"\ndescription: A test skill for install flows\n---\nBody\n";

    /// Write a minimal, valid skill directory (SKILL.md only) and return it.
    fn write_valid_skill(parent: &Path, dir_name: &str) -> PathBuf {
        let dir = parent.join(dir_name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("SKILL.md"), VALID_SKILL_MD).unwrap();
        dir
    }

    /// Build an in-memory zip containing a single skill under `test-skill/`.
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

    async fn make_service(storage: &Path) -> FastSkillService {
        use fastskill_core::ServiceConfig;
        let config = ServiceConfig {
            skill_storage_path: storage.to_path_buf(),
            ..Default::default()
        };
        let mut service = FastSkillService::new(config).await.unwrap();
        service.initialize().await.unwrap();
        service
    }

    // ── setup_skill_in_storage ────────────────────────────────────────────────

    #[tokio::test]
    async fn test_setup_skill_in_storage_copy() {
        let tmp = tempfile::tempdir().unwrap();
        let src = write_valid_skill(tmp.path(), "src-skill");
        let dst = tmp.path().join("storage").join("test-skill");
        setup_skill_in_storage(&src, &dst, false).await.unwrap();
        assert!(dst.join("SKILL.md").exists());
        assert!(!dst.is_symlink());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_setup_skill_in_storage_editable_symlink() {
        let tmp = tempfile::tempdir().unwrap();
        let src = write_valid_skill(tmp.path(), "src-skill");
        let dst = tmp.path().join("storage").join("test-skill");
        std::fs::create_dir_all(dst.parent().unwrap()).unwrap();
        setup_skill_in_storage(&src, &dst, true).await.unwrap();
        assert!(dst.is_symlink(), "editable install must be a symlink");
    }

    #[tokio::test]
    async fn test_setup_skill_in_storage_overwrites_existing() {
        let tmp = tempfile::tempdir().unwrap();
        let src = write_valid_skill(tmp.path(), "src-skill");
        let dst = tmp.path().join("storage").join("test-skill");
        // Pre-existing directory with stale content.
        std::fs::create_dir_all(&dst).unwrap();
        std::fs::write(dst.join("stale.txt"), "old").unwrap();
        setup_skill_in_storage(&src, &dst, false).await.unwrap();
        assert!(dst.join("SKILL.md").exists());
        assert!(
            !dst.join("stale.txt").exists(),
            "stale content must be removed"
        );
    }

    // ── install_skill_from_entry: Local ───────────────────────────────────────

    #[tokio::test]
    async fn test_install_skill_from_entry_local_copy() {
        let tmp = tempfile::tempdir().unwrap();
        let src = write_valid_skill(tmp.path(), "src-skill");
        let storage = tmp.path().join("storage");
        let service = make_service(&storage).await;

        let entry = SkillEntry {
            id: "test-skill".to_string(),
            source: SkillSource::Local {
                path: src.clone(),
                editable: false,
            },
            version: "1.0.0".to_string(),
            groups: Vec::new(),
            editable: false,
        };
        let def = install_skill_from_entry(&service, entry, None)
            .await
            .unwrap();
        assert!(matches!(def.source_type, Some(SourceType::LocalPath)));
        assert!(!def.editable);
        assert!(def.skill_file.ends_with("SKILL.md"));
        assert!(def.skill_file.exists());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_install_skill_from_entry_local_editable() {
        let tmp = tempfile::tempdir().unwrap();
        let src = write_valid_skill(tmp.path(), "src-skill");
        let storage = tmp.path().join("storage");
        let service = make_service(&storage).await;

        let entry = SkillEntry {
            id: "test-skill".to_string(),
            source: SkillSource::Local {
                path: src.clone(),
                editable: true,
            },
            version: "1.0.0".to_string(),
            groups: Vec::new(),
            editable: true,
        };
        let def = install_skill_from_entry(&service, entry, None)
            .await
            .unwrap();
        assert!(def.editable);
        let stored = storage.join(def.id.as_str());
        assert!(stored.is_symlink(), "editable local install must symlink");
    }

    #[tokio::test]
    async fn test_install_from_local_nonexistent_path() {
        let tmp = tempfile::tempdir().unwrap();
        let storage = tmp.path().join("storage");
        let service = make_service(&storage).await;
        let entry = SkillEntry {
            id: "missing".to_string(),
            source: SkillSource::Local {
                path: tmp.path().join("does-not-exist"),
                editable: false,
            },
            version: "1.0.0".to_string(),
            groups: Vec::new(),
            editable: false,
        };
        let result = install_skill_from_entry(&service, entry, None).await;
        assert!(matches!(result, Err(CliError::InvalidSource(_))));
    }

    // ── install_skill_from_entry: Source without a sources manager ─────────────

    #[tokio::test]
    async fn test_install_skill_from_entry_source_requires_manager() {
        let tmp = tempfile::tempdir().unwrap();
        let storage = tmp.path().join("storage");
        let service = make_service(&storage).await;
        let entry = SkillEntry {
            id: "scope/skill".to_string(),
            source: SkillSource::Source {
                name: "myrepo".to_string(),
                skill: "scope/skill".to_string(),
                version: None,
            },
            version: "1.0.0".to_string(),
            groups: Vec::new(),
            editable: false,
        };
        let result = install_skill_from_entry(&service, entry, None).await;
        assert!(matches!(result, Err(CliError::Config(_))));
    }

    // ── install_skill_from_entry: ZipUrl (mock HTTP) ───────────────────────────

    #[tokio::test]
    async fn test_install_skill_from_entry_zip_url() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        let zip_bytes = build_skill_zip();
        Mock::given(method("GET"))
            .and(path("/pkg.zip"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(zip_bytes))
            .mount(&server)
            .await;

        let tmp = tempfile::tempdir().unwrap();
        let storage = tmp.path().join("storage");
        let service = make_service(&storage).await;

        let entry = SkillEntry {
            id: "test-skill".to_string(),
            source: SkillSource::ZipUrl {
                base_url: format!("{}/pkg.zip", server.uri()),
                version: Some("1.0.0".to_string()),
            },
            version: "1.0.0".to_string(),
            groups: Vec::new(),
            editable: false,
        };
        let def = install_skill_from_entry(&service, entry, None)
            .await
            .unwrap();
        assert!(matches!(def.source_type, Some(SourceType::ZipFile)));
        assert_eq!(def.source_tag, Some("1.0.0".to_string()));
        assert!(def.skill_file.exists());
    }

    #[tokio::test]
    async fn test_download_and_extract_zip_http_404() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/missing.zip"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;

        let result = download_and_extract_zip(&format!("{}/missing.zip", server.uri())).await;
        assert!(matches!(result, Err(CliError::InvalidSource(_))));
    }

    #[tokio::test]
    async fn test_download_and_extract_zip_success() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/ok.zip"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(build_skill_zip()))
            .mount(&server)
            .await;

        let (_temp, skill_path) = download_and_extract_zip(&format!("{}/ok.zip", server.uri()))
            .await
            .unwrap();
        assert!(skill_path.join("SKILL.md").exists());
    }

    // ── create_sources_manager_from_repositories ──────────────────────────────

    fn git_marketplace_repo(name: &str) -> fastskill_core::core::repository::RepositoryDefinition {
        use fastskill_core::core::repository::{
            RepositoryConfig, RepositoryDefinition, RepositoryType,
        };
        RepositoryDefinition {
            name: name.to_string(),
            repo_type: RepositoryType::GitMarketplace,
            priority: 0,
            config: RepositoryConfig::GitMarketplace {
                url: "https://github.com/org/marketplace".to_string(),
                branch: None,
                tag: None,
            },
            auth: None,
            storage: None,
        }
    }

    fn http_registry_repo(name: &str) -> fastskill_core::core::repository::RepositoryDefinition {
        use fastskill_core::core::repository::{
            RepositoryConfig, RepositoryDefinition, RepositoryType,
        };
        RepositoryDefinition {
            name: name.to_string(),
            repo_type: RepositoryType::HttpRegistry,
            priority: 0,
            config: RepositoryConfig::HttpRegistry {
                index_url: "https://example.com/index".to_string(),
            },
            auth: None,
            storage: None,
        }
    }

    #[test]
    fn test_create_sources_manager_from_marketplace_repo() {
        let repo_manager = RepositoryManager::from_definitions(vec![git_marketplace_repo("mp")]);
        let result = create_sources_manager_from_repositories(&repo_manager).unwrap();
        assert!(
            result.is_some(),
            "a marketplace repo yields a SourcesManager"
        );
    }

    #[test]
    fn test_create_sources_manager_from_http_registry_only_is_none() {
        let repo_manager = RepositoryManager::from_definitions(vec![http_registry_repo("reg")]);
        let result = create_sources_manager_from_repositories(&repo_manager).unwrap();
        assert!(
            result.is_none(),
            "http-registry-only repos produce no marketplace sources manager"
        );
    }

    #[test]
    fn test_create_sources_manager_from_empty_is_none() {
        let repo_manager = RepositoryManager::from_definitions(Vec::new());
        let result = create_sources_manager_from_repositories(&repo_manager).unwrap();
        assert!(result.is_none());
    }
}
