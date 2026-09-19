//! Choose the Repository a skill-id Origin ref resolves against.
//!
//! Repository conflicts resolve by priority (CONTEXT.md, "Repository"): the
//! highest-precedence repository that has the skill wins. A higher-precedence
//! repository is skipped only when its metadata proves the skill is absent;
//! when absence cannot be proven (offline with no cached index, or a refresh
//! failure) that repository is chosen, so a stale or missing index never lets a
//! lower-precedence repository shadow it.

use crate::core::cache::SkillCache;
use crate::core::repository::{RepositoryDefinition, RepositoryManager};
use crate::core::service::ServiceError;

/// What a repository's metadata says about one skill id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Presence {
    Listed,
    Absent,
    Unknown,
}

/// Repositories in resolution order: the default repository first (the same
/// one `get_default_repository` has always chosen), then the rest by priority,
/// with the name as a deterministic tie-breaker.
fn resolution_order(manager: &RepositoryManager) -> Vec<&RepositoryDefinition> {
    let default_name = manager.get_default_repository().map(|r| r.name.clone());
    let mut repos = manager.list_repositories();
    repos.sort_by(|a, b| {
        let a_default = Some(&a.name) == default_name.as_ref();
        let b_default = Some(&b.name) == default_name.as_ref();
        b_default
            .cmp(&a_default)
            .then(a.priority.cmp(&b.priority))
            .then(a.name.cmp(&b.name))
    });
    repos
}

fn cached_presence(cache: &SkillCache, repo: &str, skill: &str) -> Presence {
    match cache.read_source_index(repo) {
        Ok(Some(index)) if index.entries.iter().any(|e| e.skill == skill) => Presence::Listed,
        Ok(Some(_)) => Presence::Absent,
        Ok(None) => Presence::Unknown,
        Err(error) => {
            tracing::warn!("unreadable cached index for repository '{repo}': {error}");
            Presence::Unknown
        }
    }
}

async fn presence(
    manager: &RepositoryManager,
    cache: &SkillCache,
    repo: &str,
    skill: &str,
    offline: bool,
) -> Presence {
    let cached = cached_presence(cache, repo, skill);
    if cached == Presence::Listed || offline {
        return cached;
    }
    // A cached miss may be stale; only a fresh listing proves absence.
    match manager.refresh_index(cache, repo).await {
        Ok(_) => cached_presence(cache, repo, skill),
        Err(error) => {
            tracing::warn!(
                "could not refresh repository '{repo}' while resolving '{skill}': {error}"
            );
            Presence::Unknown
        }
    }
}

/// Name the repository `skill` resolves against. With one repository there is
/// nothing to choose and no metadata is read.
pub(super) async fn choose_repository(
    manager: &RepositoryManager,
    cache: &SkillCache,
    skill: &str,
    offline: bool,
) -> Result<String, ServiceError> {
    let order = resolution_order(manager);
    let Some(first) = order.first() else {
        return Err(ServiceError::Config(
            "No default repository configured. Use 'fastskill repo add' to add a \
             repository before installing by skill id."
                .to_string(),
        ));
    };
    if order.len() == 1 {
        return Ok(first.name.clone());
    }
    for repo in &order {
        match presence(manager, cache, &repo.name, skill, offline).await {
            Presence::Listed | Presence::Unknown => return Ok(repo.name.clone()),
            Presence::Absent => {}
        }
    }
    let checked = order
        .iter()
        .map(|r| r.name.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    Err(ServiceError::Config(format!(
        "skill '{skill}' was not found in any configured repository (checked: {checked}); \
         check the id with `fastskill skill search {skill}`"
    )))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::core::cache::{SourceIndex, SourceIndexEntry};
    use crate::core::repository::{RepositoryConfig, RepositoryType};
    use tempfile::TempDir;

    fn local_repo(name: &str, priority: u32, path: &std::path::Path) -> RepositoryDefinition {
        RepositoryDefinition {
            name: name.to_string(),
            repo_type: RepositoryType::Local,
            priority,
            config: RepositoryConfig::Local {
                path: path.to_path_buf(),
            },
            auth: None,
            storage: None,
        }
    }

    fn write_skill(root: &std::path::Path, id: &str) {
        let dir = root.join(id);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("SKILL.md"),
            format!("---\nname: {id}\ndescription: fixture\nversion: 1.0.0\n---\nbody\n"),
        )
        .unwrap();
    }

    fn seed_index(cache: &SkillCache, repo: &str, skills: &[&str]) {
        let index = SourceIndex {
            fetched_at: chrono::Utc::now(),
            entries: skills
                .iter()
                .map(|skill| SourceIndexEntry {
                    skill: skill.to_string(),
                    versions: vec!["1.0.0".to_string()],
                    name: skill.to_string(),
                    description: String::new(),
                })
                .collect(),
        };
        cache.write_source_index(repo, &index).unwrap();
    }

    /// Two local repositories: `a` (default, priority 0) and `b` (priority 1).
    fn fixture(a_skills: &[&str], b_skills: &[&str]) -> (TempDir, RepositoryManager, SkillCache) {
        let tmp = TempDir::new().unwrap();
        let (a, b) = (tmp.path().join("a"), tmp.path().join("b"));
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        a_skills.iter().for_each(|s| write_skill(&a, s));
        b_skills.iter().for_each(|s| write_skill(&b, s));
        let manager = RepositoryManager::from_definitions(vec![
            local_repo("a", 0, &a),
            local_repo("b", 1, &b),
        ]);
        let cache = SkillCache::at_root(tmp.path().join("cache"));
        (tmp, manager, cache)
    }

    #[tokio::test]
    async fn skill_only_in_lower_priority_repository_resolves_there() {
        let (_tmp, manager, cache) = fixture(&["other"], &["cli-rust-dev"]);
        let repo = choose_repository(&manager, &cache, "cli-rust-dev", false)
            .await
            .unwrap();
        assert_eq!(repo, "b");
    }

    #[tokio::test]
    async fn higher_priority_repository_wins_when_both_list_the_skill() {
        let (_tmp, manager, cache) = fixture(&["shared"], &["shared"]);
        let repo = choose_repository(&manager, &cache, "shared", false)
            .await
            .unwrap();
        assert_eq!(repo, "a");
    }

    #[tokio::test]
    async fn stale_cached_miss_is_refreshed_before_falling_through() {
        // `a` now has the skill but its cached index predates it.
        let (_tmp, manager, cache) = fixture(&["shared"], &["shared"]);
        seed_index(&cache, "a", &[]);
        let repo = choose_repository(&manager, &cache, "shared", false)
            .await
            .unwrap();
        assert_eq!(repo, "a");
    }

    #[tokio::test]
    async fn offline_unknown_higher_priority_repository_is_not_shadowed() {
        let (_tmp, manager, cache) = fixture(&[], &["cli-rust-dev"]);
        seed_index(&cache, "b", &["cli-rust-dev"]);
        let repo = choose_repository(&manager, &cache, "cli-rust-dev", true)
            .await
            .unwrap();
        assert_eq!(repo, "a");
    }

    #[tokio::test]
    async fn offline_proven_absence_falls_through_to_cached_listing() {
        let (_tmp, manager, cache) = fixture(&[], &[]);
        seed_index(&cache, "a", &["other"]);
        seed_index(&cache, "b", &["cli-rust-dev"]);
        let repo = choose_repository(&manager, &cache, "cli-rust-dev", true)
            .await
            .unwrap();
        assert_eq!(repo, "b");
    }

    #[tokio::test]
    async fn missing_everywhere_names_every_repository_checked() {
        let (_tmp, manager, cache) = fixture(&["other"], &["other"]);
        let error = choose_repository(&manager, &cache, "nope", false)
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("'nope'"), "{error}");
        assert!(error.contains("checked: a, b"), "{error}");
    }

    #[tokio::test]
    async fn single_repository_is_chosen_without_reading_metadata() {
        let tmp = TempDir::new().unwrap();
        let manager = RepositoryManager::from_definitions(vec![local_repo(
            "only",
            0,
            &tmp.path().join("does-not-exist"),
        )]);
        let cache = SkillCache::at_root(tmp.path().join("cache"));
        let repo = choose_repository(&manager, &cache, "anything", false)
            .await
            .unwrap();
        assert_eq!(repo, "only");
        assert!(cache.read_source_index("only").unwrap().is_none());
    }
}
