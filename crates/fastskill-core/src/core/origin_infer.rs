//! The core `Origin`-ref inference seam (spec 003 Phase 3, "smart input").
//!
//! `FastSkillService::infer_origin(&str) -> Result<Origin, ServiceError>` resolves
//! a single raw string — the **Origin ref** a user types or pastes (a git URL, a
//! `.zip` URL, a local path, or a `scope/skill[@version]` id) — into a typed
//! [`Origin`]. This is the *one* inference path: both `fastskill server serve`'s HTTP
//! install endpoint and `fastskill-cli`'s `add` command call it, so there is a
//! single place the git-URL/skill-id/path classification rules live rather than
//! two copies drifting apart (the CLI previously had its own private
//! `detect_skill_source` + `parse_git_url`; see ADR-0005 and the "Origin ref"
//! entry in `CONTEXT.md`).
//!
//! Classification rules (identical to the pre-seam CLI behavior):
//! - `scope/skill`, `skill`, or `skill[@version]` (an id, not a URL/path) →
//!   [`Origin::Repository`], resolved against an explicitly selected repository
//!   or else the highest-priority configured repository that has the skill
//!   (recorded by its concrete name — ADR-0005 §Q4; see [`repository_walk`]).
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

mod repository_walk;

/// Caller context for [`FastSkillService::infer_origin_with`].
#[derive(Debug, Clone, Copy, Default)]
pub struct InferOptions<'a> {
    /// A repository the caller selected explicitly (`--repository`). A
    /// skill-id ref resolves against it without consulting other repositories;
    /// the caller validates that it exists.
    pub repository: Option<&'a str>,
    /// Use only cached repository metadata while choosing a repository.
    pub offline: bool,
}

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

/// True when `git_url` is a GitHub *browser* link (`/org/repo/tree/<branch>[/<subdir>]`)
/// rather than a clone URL. Git cannot clone such a URL, so manifest schema v2 refuses it
/// and keeps the repository, the subdirectory and the branch in separate fields.
pub fn is_github_tree_url(git_url: &str) -> bool {
    github_tree_segments(git_url).is_some()
}

/// Everything a GitHub tree URL carries after `/tree/`: `<branch>[/<subdir>]`, undivided.
/// The URL alone cannot say where the branch ends, because a branch name may contain `/` —
/// only an explicitly declared ref can. `None` when this is not a GitHub tree URL, or when
/// nothing follows `/tree/`.
pub fn github_tree_path(git_url: &str) -> Option<String> {
    let segments = github_tree_segments(git_url)?;
    let path = segments.get(3..)?.join("/");
    (!path.is_empty()).then_some(path)
}

/// The path segments of `git_url` when it is a GitHub tree URL, else `None`.
fn github_tree_segments(git_url: &str) -> Option<Vec<String>> {
    let url = Url::parse(git_url).ok()?;
    if url.host_str() != Some("github.com") {
        return None;
    }
    let segments: Vec<String> = url
        .path()
        .split('/')
        .filter(|segment| !segment.is_empty())
        .map(str::to_string)
        .collect();
    (segments.get(2).map(String::as_str) == Some("tree")).then_some(segments)
}

/// The subdirectory a GitHub tree path names once the declared ref is taken off the front:
/// `Some(None)` when the path is exactly the ref, `Some(Some(dir))` for the remainder, and
/// `None` when the path does not start with the ref at all — which means the two disagree.
///
/// Splitting on the declared ref rather than on the first path segment is the only way to
/// get a branch containing `/` right, and the declared ref is the only place that is known.
pub fn tree_path_subdir(tree_path: &str, ref_name: &str) -> Option<Option<PathBuf>> {
    if tree_path == ref_name {
        return Some(None);
    }
    tree_path
        .strip_prefix(&format!("{ref_name}/"))
        .map(|remainder| Some(PathBuf::from(remainder)))
}

/// Best-effort v1 → v2 normalization of a single [`Origin`], for *comparing* a record
/// written before manifest schema v2 (a `skills.lock` entry) with its upgraded manifest
/// entry. Anything that is not a git origin carrying a GitHub tree URL passes through
/// untouched, and an explicit `ref`/`subdir` always wins over the URL-derived one.
///
/// Unlike the manifest upgrade this never fails: a lock is a record of what happened, not
/// a declaration to validate, so a URL that cannot be split is simply left alone.
pub fn normalize_git_tree_origin(origin: &Origin) -> Origin {
    let Origin::Git { url, r#ref, subdir } = origin else {
        return origin.clone();
    };
    if !is_github_tree_url(url) {
        return origin.clone();
    }
    let Ok(info) = parse_git_url(url) else {
        return origin.clone();
    };
    // An explicitly declared branch or tag is what the tree path is split on; only when
    // there is none does the URL's own first segment become the branch.
    let declared = match r#ref {
        GitRef::Branch(name) | GitRef::Tag(name) => Some(name.as_str()),
        GitRef::Default | GitRef::Commit(_) => None,
    };
    let derived_subdir = declared
        .and_then(|name| github_tree_path(url).and_then(|path| tree_path_subdir(&path, name)))
        .unwrap_or(info.subdir);
    let derived_ref = match (r#ref, &info.branch) {
        (GitRef::Default, Some(branch)) => GitRef::Branch(branch.clone()),
        _ => r#ref.clone(),
    };
    Origin::Git {
        url: info.repo_url,
        r#ref: derived_ref,
        subdir: subdir.clone().or(derived_subdir),
    }
}

impl FastSkillService {
    /// Resolve a raw **Origin ref** string into a typed [`Origin`] (ADR-0005 /
    /// spec 003 Phase 3). See the module docs for the full classification
    /// table. This fetches no skill content — `add_from_origin` does that — but
    /// choosing the repository for a skill id may refresh repository metadata
    /// when several repositories are configured.
    pub async fn infer_origin(&self, origin_ref: &str) -> Result<Origin, ServiceError> {
        self.infer_origin_with(origin_ref, InferOptions::default())
            .await
    }

    /// [`Self::infer_origin`] with an explicit repository selection and/or
    /// offline mode.
    pub async fn infer_origin_with(
        &self,
        origin_ref: &str,
        options: InferOptions<'_>,
    ) -> Result<Origin, ServiceError> {
        let trimmed = origin_ref.trim();
        if trimmed.is_empty() {
            return Err(ServiceError::InvalidOperation(
                "Origin ref cannot be empty".to_string(),
            ));
        }

        // Prefer an existing filesystem path over the deliberately permissive
        // skill-ID shorthand. Without this check, `fastskill skill add demo`
        // tries a repository lookup even when ./demo exists.
        if !PathBuf::from(trimmed).exists() && is_skill_id(trimmed) {
            return self.infer_repository_origin(trimmed, options).await;
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
                return Ok(Origin::Git {
                    url: git_info.repo_url,
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
    /// `skill@version`) into `Origin::Repository`, recorded by the concrete
    /// repository name (ADR-0005 §Q4).
    async fn infer_repository_origin(
        &self,
        skill_id_ref: &str,
        options: InferOptions<'_>,
    ) -> Result<Origin, ServiceError> {
        let (skill, version_str) = parse_skill_id_ref(skill_id_ref);
        let version = version_str
            .as_deref()
            .map(VersionConstraint::parse)
            .transpose()
            .map_err(|e| {
                ServiceError::InvalidOperation(format!("Invalid version constraint: {e}"))
            })?;

        let repo = match options.repository {
            Some(repository) => repository.to_string(),
            None => {
                let repo_manager = self.repository_manager().ok_or_else(|| {
                    ServiceError::Config(
                        "No default repository configured. Use 'fastskill repo add' to add a \
                         repository before installing by skill id."
                            .to_string(),
                    )
                })?;
                repository_walk::choose_repository(
                    repo_manager,
                    self.skill_cache(),
                    &skill,
                    options.offline,
                )
                .await?
            }
        };

        Ok(Origin::Repository {
            repo,
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
                url: "https://github.com/org/repo.git".to_string(),
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
                url: "https://github.com/org/repo.git".to_string(),
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

    #[test]
    fn infer_existing_bare_path_before_skill_id() {
        let _lock = crate::test_utils::DIR_MUTEX
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let tmp = TempDir::new().unwrap();
        let bare = tmp.path().join("demo");
        std::fs::create_dir(&bare).unwrap();
        let previous = std::env::current_dir().unwrap();
        struct Restore(std::path::PathBuf);
        impl Drop for Restore {
            fn drop(&mut self) {
                let _ = std::env::set_current_dir(&self.0);
            }
        }
        let _restore = Restore(previous);
        std::env::set_current_dir(tmp.path()).unwrap();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let origin = runtime.block_on(async {
            let service =
                with_default_repo(make_service(&tmp.path().join("storage")).await, "main");
            service.infer_origin("demo").await.unwrap()
        });
        assert!(
            matches!(origin, Origin::Local { path, .. } if path == std::path::Path::new("demo"))
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

    // ── GitHub browser ("/tree/") urls ─────────────────────────────────────

    #[test]
    fn github_tree_urls_are_recognized_and_nothing_else_is() {
        assert!(is_github_tree_url(
            "https://github.com/org/repo/tree/main/skill"
        ));
        assert!(is_github_tree_url("https://github.com/org/repo/tree/main"));
        for plain in [
            "https://github.com/org/repo.git",
            "https://github.com/org/tree.git",
            "https://gitlab.com/org/repo/-/tree/main/skill",
            "git://127.0.0.1:9418/repo.git",
            "not a url",
        ] {
            assert!(!is_github_tree_url(plain), "{plain} is not a browser url");
        }
    }

    #[test]
    fn github_tree_path_is_everything_after_tree() {
        assert_eq!(
            github_tree_path("https://github.com/org/repo/tree/feature/x/skills/foo").as_deref(),
            Some("feature/x/skills/foo")
        );
        assert_eq!(
            github_tree_path("https://github.com/org/repo/tree/main").as_deref(),
            Some("main")
        );
        // Nothing after `/tree/` names neither a branch nor a subdirectory.
        assert_eq!(github_tree_path("https://github.com/org/repo/tree"), None);
        assert_eq!(github_tree_path("https://github.com/org/repo.git"), None);
    }

    #[test]
    fn tree_path_subdir_splits_on_the_declared_ref() {
        assert_eq!(tree_path_subdir("main", "main"), Some(None));
        assert_eq!(
            tree_path_subdir("feature/x/skills/foo", "feature/x"),
            Some(Some(PathBuf::from("skills/foo")))
        );
        // A path that does not start with the ref means the two disagree.
        assert_eq!(tree_path_subdir("main/skill", "release"), None);
        assert_eq!(tree_path_subdir("mainline/skill", "main"), None);
    }

    #[test]
    fn normalizing_a_recorded_origin_produces_the_schema_two_form() {
        let normalized = normalize_git_tree_origin(&Origin::Git {
            url: "https://github.com/org/repo/tree/main/skill".to_string(),
            r#ref: GitRef::Branch("main".to_string()),
            subdir: None,
        });
        assert_eq!(
            normalized,
            Origin::Git {
                url: "https://github.com/org/repo.git".to_string(),
                r#ref: GitRef::Branch("main".to_string()),
                subdir: Some(PathBuf::from("skill")),
            }
        );

        // A branch containing `/` splits on the declared ref, as the manifest upgrade does.
        assert_eq!(
            normalize_git_tree_origin(&Origin::Git {
                url: "https://github.com/org/repo/tree/feature/x/skills/foo".to_string(),
                r#ref: GitRef::Branch("feature/x".to_string()),
                subdir: None,
            }),
            Origin::Git {
                url: "https://github.com/org/repo.git".to_string(),
                r#ref: GitRef::Branch("feature/x".to_string()),
                subdir: Some(PathBuf::from("skills/foo")),
            }
        );
    }

    #[test]
    fn normalizing_leaves_everything_else_alone() {
        for origin in [
            Origin::Git {
                url: "https://github.com/org/repo".to_string(),
                r#ref: GitRef::Default,
                subdir: None,
            },
            Origin::Git {
                url: "git://127.0.0.1:9418/repo.git".to_string(),
                r#ref: GitRef::Branch("main".to_string()),
                subdir: Some(PathBuf::from("skill")),
            },
            Origin::Local {
                path: PathBuf::from("./demo"),
                editable: true,
            },
            Origin::ZipUrl {
                url: "https://example.com/s.zip".to_string(),
            },
        ] {
            assert_eq!(normalize_git_tree_origin(&origin), origin);
        }
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
