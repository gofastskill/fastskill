//! The core `Origin`-ref inference seam (spec 003 Phase 3, "smart input").
//!
//! `FastSkillService::infer_origin(&str) -> Result<Origin, ServiceError>` resolves
//! a single raw string — the **Origin ref** a user types or pastes (a git URL, a
//! `.zip` URL, a local path, or a `scope/skill[@version]` id) — into a typed
//! [`Origin`]. This is the *one* inference path: both `fastskill serve`'s HTTP
//! install endpoint and `fastskill-cli`'s `add` command call it, so there is a
//! single place the git-URL/skill-id/path classification rules live rather than
//! two copies drifting apart (the CLI previously had its own private
//! `detect_skill_source` + `parse_git_url`; see ADR-0005 and the "Origin ref"
//! entry in `CONTEXT.md`).
//!
//! Classification rules (identical to the pre-seam CLI behavior):
//! - `scope/skill`, `skill`, or `skill[@version]` (an id, not a URL/path) →
//!   [`Origin::Repository`], resolved against the injected
//!   [`RepositoryManager`](crate::core::repository::RepositoryManager)'s default
//!   repository (recorded by its concrete name — ADR-0005 §Q4).
//! - A `git`/`http`/`https` URL whose path does **not** end in `.zip` →
//!   [`Origin::Git`] (branch/tag/subdir parsed the same way `parse_git_url` did:
//!   a `?branch=` query param, or a GitHub `/tree/<branch>[/<subdir>]` path).
//! - A URL whose path ends in `.zip` → [`Origin::ZipUrl`].
//! - Anything else (a local path, `.zip` or directory) → [`Origin::Local`]
//!   (`editable: false`; the CLI's `-e`/`--editable` flag is applied on top by
//!   the caller, since it is not part of the ref string itself).

use crate::core::origin::{GitRef, Origin};
use crate::core::service::{FastSkillService, ServiceError};
use crate::core::version::VersionConstraint;
use std::path::PathBuf;
use url::Url;

/// Git repository info parsed from a URL: the clean clone URL plus any
/// branch/subdir extracted from query params or a GitHub tree-URL path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitUrlInfo {
    /// The repository URL, normalized to end in `.git`.
    pub repo_url: String,
    pub branch: Option<String>,
    pub subdir: Option<PathBuf>,
}

/// True for skill-id refs: `skill`, `skill@version`, `scope/skill`, or
/// `scope/skill@version`. Not a URL scheme (contains `:`) or a Windows-style
/// path (contains `\`).
pub fn is_skill_id(input: &str) -> bool {
    if input.contains('\\') || input.contains(':') {
        return false;
    }
    #[allow(clippy::expect_used)]
    let skill_id_pattern =
        regex::Regex::new(r"^[a-zA-Z0-9_-]+(/[a-zA-Z0-9_-]+)?(@[a-zA-Z0-9_.-]+)?$")
            .expect("skill-id pattern is a compile-time constant and always valid");
    skill_id_pattern.is_match(input)
}

/// Split `skill@version` into `(skill, Some(version))`, or `(skill, None)` when
/// there is no `@`.
pub fn parse_skill_id_ref(input: &str) -> (String, Option<String>) {
    match input.find('@') {
        Some(at_pos) => (
            input[..at_pos].to_string(),
            Some(input[at_pos + 1..].to_string()),
        ),
        None => (input.to_string(), None),
    }
}

/// Parse a git URL into a clean clone URL plus any branch/subdir it encodes:
/// a `?branch=` query param, or a GitHub `/org/repo/tree/<branch>[/<subdir>]`
/// path. Mirrors the pre-seam CLI's `parse_git_url`.
pub fn parse_git_url(git_url: &str) -> Result<GitUrlInfo, ServiceError> {
    let url = Url::parse(git_url)
        .map_err(|e| ServiceError::InvalidOperation(format!("Invalid git URL: {e}")))?;

    let query_branch = url
        .query_pairs()
        .find(|(key, _)| key == "branch")
        .map(|(_, value)| value.to_string());

    let path_segments: Vec<&str> = url.path().split('/').filter(|s| !s.is_empty()).collect();
    let mut branch = None;
    let mut subdir = None;

    if url.host_str() == Some("github.com")
        && path_segments.len() >= 3
        && path_segments.get(2) == Some(&"tree")
        && path_segments.len() >= 4
    {
        // GitHub tree URL: /org/repo/tree/branch[/subdir...]
        branch = Some(path_segments[3].to_string());
        if path_segments.len() > 4 {
            subdir = Some(PathBuf::from(path_segments[4..].join("/")));
        }
    }

    let mut clean_url = url.clone();
    clean_url.set_query(None);
    let mut path = clean_url.path().to_string();

    if url.host_str() == Some("github.com") && path.contains("/tree/") {
        if let Some(tree_pos) = path.find("/tree/") {
            path = path[..tree_pos].to_string();
        }
    }

    if !path.ends_with(".git") {
        path.push_str(".git");
    }

    clean_url.set_path(&path);
    let repo_url = clean_url.to_string();

    Ok(GitUrlInfo {
        repo_url,
        branch: branch.or(query_branch),
        subdir,
    })
}

impl FastSkillService {
    /// Resolve a raw **Origin ref** string into a typed [`Origin`] (ADR-0005 /
    /// spec 003 Phase 3). See the module docs for the full classification
    /// table. This does not fetch or validate anything — it only classifies;
    /// `add_from_origin` performs the actual fetch.
    pub async fn infer_origin(&self, origin_ref: &str) -> Result<Origin, ServiceError> {
        let trimmed = origin_ref.trim();
        if trimmed.is_empty() {
            return Err(ServiceError::InvalidOperation(
                "Origin ref cannot be empty".to_string(),
            ));
        }

        if is_skill_id(trimmed) {
            return self.infer_repository_origin(trimmed).await;
        }

        if let Ok(url) = Url::parse(trimmed) {
            if matches!(url.scheme(), "git" | "http" | "https") {
                if url.path().to_ascii_lowercase().ends_with(".zip") {
                    return Ok(Origin::ZipUrl {
                        url: trimmed.to_string(),
                    });
                }
                let git_info = parse_git_url(trimmed)?;
                let r#ref = match &git_info.branch {
                    Some(b) => GitRef::Branch(b.clone()),
                    None => GitRef::Default,
                };
                // The recorded URL is the ref exactly as given (not the
                // `.git`-normalized `repo_url`) — `parse_git_url` is consulted
                // only for the branch/subdir it can extract, mirroring the
                // pre-seam CLI's `build_origin`.
                return Ok(Origin::Git {
                    url: trimmed.to_string(),
                    r#ref,
                    subdir: git_info.subdir,
                });
            }
        }

        // Anything else is a local filesystem path — a `.zip` archive or a
        // directory; the installer detects which at fetch time
        // (`add_from_origin`'s `fetch_local`), so both map to `Origin::Local`.
        Ok(Origin::Local {
            path: PathBuf::from(trimmed),
            editable: false,
        })
    }

    /// Resolve a classified skill-id ref (`scope/skill`, `skill`, or
    /// `skill@version`) into `Origin::Repository`, against the injected
    /// [`RepositoryManager`](crate::core::repository::RepositoryManager)'s
    /// default repository (recorded by its concrete name, ADR-0005 §Q4).
    async fn infer_repository_origin(&self, skill_id_ref: &str) -> Result<Origin, ServiceError> {
        let (skill, version_str) = parse_skill_id_ref(skill_id_ref);
        let version = version_str
            .as_deref()
            .map(VersionConstraint::parse)
            .transpose()
            .map_err(|e| {
                ServiceError::InvalidOperation(format!("Invalid version constraint: {e}"))
            })?;

        let repo_manager = self.repository_manager().ok_or_else(|| {
            ServiceError::Config(
                "No default repository configured. Use 'fastskill repos add' to add a \
                 repository before installing by skill id."
                    .to_string(),
            )
        })?;
        let default_repo = repo_manager.get_default_repository().ok_or_else(|| {
            ServiceError::Config(
                "No default repository configured. Use 'fastskill repos add' to add a \
                 repository before installing by skill id."
                    .to_string(),
            )
        })?;

        Ok(Origin::Repository {
            repo: default_repo.name.clone(),
            skill,
            version,
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::core::repository::RepositoryManager;
    use crate::{FastSkillService, ServiceConfig};
    use std::sync::Arc;
    use tempfile::TempDir;

    async fn make_service(storage: &std::path::Path) -> FastSkillService {
        let config = ServiceConfig {
            skill_storage_path: storage.to_path_buf(),
            ..Default::default()
        };
        let mut service = FastSkillService::new(config).await.unwrap();
        service.initialize().await.unwrap();
        service
    }

    fn with_default_repo(service: FastSkillService, name: &str) -> FastSkillService {
        use crate::core::repository::{RepositoryConfig, RepositoryDefinition, RepositoryType};
        let manager = RepositoryManager::from_definitions(vec![RepositoryDefinition {
            name: name.to_string(),
            repo_type: RepositoryType::HttpRegistry,
            priority: 0,
            config: RepositoryConfig::HttpRegistry {
                index_url: "https://example.com/index".to_string(),
            },
            auth: None,
            storage: None,
        }]);
        service.with_repository_manager(Arc::new(manager))
    }

    // ── git ────────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn infer_git_default_branch() {
        let tmp = TempDir::new().unwrap();
        let service = make_service(&tmp.path().join("storage")).await;
        let origin = service
            .infer_origin("https://github.com/org/repo.git")
            .await
            .unwrap();
        assert_eq!(
            origin,
            Origin::Git {
                url: "https://github.com/org/repo.git".to_string(),
                r#ref: GitRef::Default,
                subdir: None,
            }
        );
    }

    #[tokio::test]
    async fn infer_git_branch_from_query_param() {
        let tmp = TempDir::new().unwrap();
        let service = make_service(&tmp.path().join("storage")).await;
        let origin = service
            .infer_origin("https://github.com/org/repo?branch=dev")
            .await
            .unwrap();
        assert_eq!(
            origin,
            Origin::Git {
                url: "https://github.com/org/repo?branch=dev".to_string(),
                r#ref: GitRef::Branch("dev".to_string()),
                subdir: None,
            }
        );
    }

    #[tokio::test]
    async fn infer_git_tree_url_branch_and_subdir() {
        let tmp = TempDir::new().unwrap();
        let service = make_service(&tmp.path().join("storage")).await;
        let origin = service
            .infer_origin("https://github.com/org/repo/tree/main/skills/inner")
            .await
            .unwrap();
        assert_eq!(
            origin,
            Origin::Git {
                url: "https://github.com/org/repo/tree/main/skills/inner".to_string(),
                r#ref: GitRef::Branch("main".to_string()),
                subdir: Some(PathBuf::from("skills/inner")),
            }
        );
    }

    #[tokio::test]
    async fn infer_git_tag_is_not_inferred_from_string() {
        // There is no ref-string encoding for "tag" (only a GitHub tree URL's
        // branch, or a `?branch=` query param) — tag selection is a CLI/UI
        // override applied on top of the inferred `Default` ref.
        let tmp = TempDir::new().unwrap();
        let service = make_service(&tmp.path().join("storage")).await;
        let origin = service
            .infer_origin("https://gitlab.com/org/repo.git")
            .await
            .unwrap();
        assert_eq!(
            origin,
            Origin::Git {
                url: "https://gitlab.com/org/repo.git".to_string(),
                r#ref: GitRef::Default,
                subdir: None,
            }
        );
    }

    // ── zip-url ────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn infer_zip_url() {
        let tmp = TempDir::new().unwrap();
        let service = make_service(&tmp.path().join("storage")).await;
        let origin = service
            .infer_origin("https://example.com/skills/bundle.zip")
            .await
            .unwrap();
        assert_eq!(
            origin,
            Origin::ZipUrl {
                url: "https://example.com/skills/bundle.zip".to_string(),
            }
        );
    }

    #[tokio::test]
    async fn infer_zip_url_with_query_string() {
        let tmp = TempDir::new().unwrap();
        let service = make_service(&tmp.path().join("storage")).await;
        let origin = service
            .infer_origin("https://example.com/download.zip?token=abc")
            .await
            .unwrap();
        assert!(matches!(origin, Origin::ZipUrl { .. }));
    }

    // ── local ──────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn infer_local_dir() {
        let tmp = TempDir::new().unwrap();
        let service = make_service(&tmp.path().join("storage")).await;
        let origin = service.infer_origin("./some/folder").await.unwrap();
        assert_eq!(
            origin,
            Origin::Local {
                path: PathBuf::from("./some/folder"),
                editable: false,
            }
        );
    }

    #[tokio::test]
    async fn infer_local_zip() {
        let tmp = TempDir::new().unwrap();
        let service = make_service(&tmp.path().join("storage")).await;
        let origin = service.infer_origin("./bundle.zip").await.unwrap();
        assert_eq!(
            origin,
            Origin::Local {
                path: PathBuf::from("./bundle.zip"),
                editable: false,
            }
        );
    }

    // ── skill id / repository ──────────────────────────────────────────────

    #[tokio::test]
    async fn infer_scoped_skill_id() {
        let tmp = TempDir::new().unwrap();
        let service = with_default_repo(make_service(&tmp.path().join("storage")).await, "main");
        let origin = service.infer_origin("acme/widget").await.unwrap();
        assert_eq!(
            origin,
            Origin::Repository {
                repo: "main".to_string(),
                skill: "acme/widget".to_string(),
                version: None,
            }
        );
    }

    #[tokio::test]
    async fn infer_bare_skill_id() {
        let tmp = TempDir::new().unwrap();
        let service = with_default_repo(make_service(&tmp.path().join("storage")).await, "main");
        let origin = service.infer_origin("web-scraper").await.unwrap();
        assert_eq!(
            origin,
            Origin::Repository {
                repo: "main".to_string(),
                skill: "web-scraper".to_string(),
                version: None,
            }
        );
    }

    #[tokio::test]
    async fn infer_skill_id_with_version() {
        let tmp = TempDir::new().unwrap();
        let service = with_default_repo(make_service(&tmp.path().join("storage")).await, "main");
        let origin = service.infer_origin("pptx@1.2.3").await.unwrap();
        match origin {
            Origin::Repository {
                repo,
                skill,
                version,
            } => {
                assert_eq!(repo, "main");
                assert_eq!(skill, "pptx");
                assert_eq!(version.expect("version constraint").to_string(), "=1.2.3");
            }
            other => panic!("expected Repository, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn infer_skill_id_no_default_repo_errors() {
        let tmp = TempDir::new().unwrap();
        let service = make_service(&tmp.path().join("storage")).await;
        let result = service.infer_origin("acme/widget").await;
        assert!(matches!(result, Err(ServiceError::Config(_))));
    }

    #[tokio::test]
    async fn infer_empty_ref_errors() {
        let tmp = TempDir::new().unwrap();
        let service = make_service(&tmp.path().join("storage")).await;
        let result = service.infer_origin("   ").await;
        assert!(matches!(result, Err(ServiceError::InvalidOperation(_))));
    }

    // ── parse_git_url / is_skill_id (unit-level) ────────────────────────────

    #[test]
    fn parse_git_url_plain_adds_git_suffix() {
        let info = parse_git_url("https://github.com/org/repo").unwrap();
        assert_eq!(info.repo_url, "https://github.com/org/repo.git");
        assert!(info.branch.is_none());
        assert!(info.subdir.is_none());
    }

    #[test]
    fn parse_git_url_invalid_errors() {
        let result = parse_git_url("not a url");
        assert!(matches!(result, Err(ServiceError::InvalidOperation(_))));
    }

    #[test]
    fn is_skill_id_accepts_scoped_and_bare() {
        assert!(is_skill_id("web-scraper"));
        assert!(is_skill_id("scope/id"));
        assert!(is_skill_id("scope/id@1.0.0"));
    }

    #[test]
    fn is_skill_id_rejects_urls_and_paths() {
        assert!(!is_skill_id("https://github.com/org/repo"));
        assert!(!is_skill_id("git@github.com:org/repo"));
        assert!(!is_skill_id("/absolute/path"));
    }
}
