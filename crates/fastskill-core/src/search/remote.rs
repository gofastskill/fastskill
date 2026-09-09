//! Remote search implementation for repository catalogs
//!
//! This module handles searching through remote skill repositories
//! configured in the user's repository configuration.

use super::{RepositorySearchFailure, SearchError, SearchExecution, SearchQuery, SearchResultItem};
use crate::core::service::FastSkillService;

/// Execute remote search query
pub async fn execute_remote_search(
    query: SearchQuery,
    repository_filter: Option<String>,
    service: &FastSkillService,
) -> Result<SearchExecution, SearchError> {
    let strict_repository = repository_filter.is_some();

    let repo_manager = service.repository_manager().ok_or_else(|| {
        SearchError::Config(
            "No repositories configured. Add one with 'fastskill repos add'.".to_string(),
        )
    })?;
    if repo_manager.list_repositories().is_empty() {
        return Err(SearchError::Config(
            "No repositories configured. Add one with 'fastskill repos add'.".to_string(),
        ));
    }

    let repos = if let Some(repo_name) = repository_filter {
        vec![repo_manager
            .get_repository(&repo_name)
            .ok_or_else(|| SearchError::Config(format!("Repository '{}' not found", repo_name)))?]
    } else {
        repo_manager.list_repositories()
    };

    let mut all_results = Vec::new();
    let mut failures = Vec::new();
    let mut successful_repositories = 0usize;

    for repo in repos {
        match repo_manager.get_client(&repo.name).await {
            Ok(client) => {
                match client.search(&query.query).await {
                    Ok(results) => {
                        successful_repositories += 1;
                        for result in results {
                            let id = result.id.to_string();
                            let version = result.version.clone();
                            let result_item = SearchResultItem {
                                id: id.clone(),
                                name: result.name,
                                description: if result.description.is_empty() {
                                    None
                                } else {
                                    Some(result.description)
                                },
                                source: repo.name.clone(),
                                similarity: None, // Remote search doesn't provide similarity scores
                                path: None,
                                repository: Some(repo.name.clone()),
                                version: Some(version.clone()),
                                install_command: Some(format!(
                                    "fastskill add {}@{} --repository {}",
                                    id, version, repo.name
                                )),
                            };
                            all_results.push(result_item);
                        }
                    }
                    Err(e) => {
                        if strict_repository {
                            return Err(SearchError::Repository(format!(
                                "Search on '{}' failed: {}",
                                repo.name, e
                            )));
                        }
                        failures.push(RepositorySearchFailure {
                            repository: repo.name.clone(),
                            message: e.to_string(),
                        });
                    }
                }
            }
            Err(e) => {
                if strict_repository {
                    return Err(SearchError::Repository(format!(
                        "Failed to load client for '{}': {}",
                        repo.name, e
                    )));
                }
                failures.push(RepositorySearchFailure {
                    repository: repo.name.clone(),
                    message: e.to_string(),
                });
                continue;
            } // Skip repositories that fail to load when searching across all repos
        }
    }

    // Sort by repository name for consistent ordering
    all_results.sort_by(|a, b| {
        a.repository
            .as_deref()
            .unwrap_or("")
            .cmp(b.repository.as_deref().unwrap_or(""))
            .then_with(|| a.name.cmp(&b.name))
    });
    all_results.truncate(query.limit);

    if successful_repositories == 0 && !failures.is_empty() {
        return Err(SearchError::Repository(format!(
            "All requested repositories failed: {}",
            failures
                .iter()
                .map(|failure| format!("{}: {}", failure.repository, failure.message))
                .collect::<Vec<_>>()
                .join("; ")
        )));
    }

    Ok(SearchExecution {
        results: all_results,
        failures,
    })
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::await_holding_lock
)]
mod tests {
    use super::*;
    use crate::core::manifest::SkillProjectToml;
    use crate::core::repository::{RepositoryDefinition, RepositoryManager};
    use crate::test_utils::DIR_MUTEX as CWD_LOCK;
    use crate::{FastSkillService, ServiceConfig};
    use std::fs;
    use std::path::Path;
    use std::sync::Arc;
    use std::sync::MutexGuard;
    use tempfile::TempDir;

    struct CurrentDirGuard {
        previous: std::path::PathBuf,
    }

    impl CurrentDirGuard {
        fn set(path: &Path) -> Self {
            let previous = std::env::current_dir().expect("failed to read current directory");
            std::env::set_current_dir(path).expect("failed to set current directory");
            Self { previous }
        }
    }

    impl Drop for CurrentDirGuard {
        fn drop(&mut self) {
            let _ = std::env::set_current_dir(&self.previous);
        }
    }

    fn enter_temp_workspace() -> (MutexGuard<'static, ()>, TempDir, CurrentDirGuard) {
        let lock = CWD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let temp_dir = tempfile::tempdir().expect("failed to create temp dir");
        let guard = CurrentDirGuard::set(temp_dir.path());
        (lock, temp_dir, guard)
    }

    fn write_project_with_invalid_repo(repo_name: &str) {
        let project_toml = format!(
            r#"
[dependencies]

[[tool.fastskill.repositories]]
name = "{repo_name}"
type = "http-registry"
priority = 0
index_url = "not-a-valid-url"
"#
        );

        fs::write("skill-project.toml", project_toml)
            .expect("failed to write skill-project.toml for test");
    }

    fn sample_query() -> SearchQuery {
        SearchQuery {
            query: "test".to_string(),
            scope: super::super::SearchScope::Remote,
            limit: 10,
            embedding: None,
        }
    }

    async fn injected_service(storage: &Path) -> FastSkillService {
        let project = SkillProjectToml::load_from_file(Path::new("skill-project.toml")).unwrap();
        let definitions = project
            .tool
            .and_then(|tool| tool.fastskill)
            .and_then(|fastskill| fastskill.repositories)
            .unwrap_or_default()
            .iter()
            .map(RepositoryDefinition::from)
            .collect();
        let manager = Arc::new(RepositoryManager::from_definitions(definitions));
        let mut service = FastSkillService::new(ServiceConfig {
            skill_storage_path: storage.to_path_buf(),
            ..Default::default()
        })
        .await
        .unwrap()
        .with_repository_manager(manager);
        service.initialize().await.unwrap();
        service
    }

    #[tokio::test]
    async fn remote_repo_filter_returns_repository_error_on_client_init_failure() {
        let (_lock, _temp_dir, _guard) = enter_temp_workspace();
        write_project_with_invalid_repo("broken");
        let service = injected_service(&_temp_dir.path().join("installed")).await;

        let result =
            execute_remote_search(sample_query(), Some("broken".to_string()), &service).await;
        match result {
            Err(SearchError::Repository(msg)) => {
                assert!(msg.contains("broken"), "unexpected message: {msg}");
            }
            other => panic!("expected repository error, got: {:?}", other),
        }
    }

    #[tokio::test]
    async fn remote_all_repos_reports_total_repository_failure() {
        let (_lock, _temp_dir, _guard) = enter_temp_workspace();
        write_project_with_invalid_repo("broken");
        let service = injected_service(&_temp_dir.path().join("installed")).await;

        let result = execute_remote_search(sample_query(), None, &service).await;
        match result {
            Err(SearchError::Repository(message)) => {
                assert!(message.contains("All requested repositories failed"));
                assert!(message.contains("broken"));
            }
            other => panic!("expected total repository failure, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn remote_search_without_repositories_is_not_zero_matches() {
        let (_lock, _temp_dir, _guard) = enter_temp_workspace();
        fs::write("skill-project.toml", "[dependencies]\n").unwrap();
        let service = injected_service(&_temp_dir.path().join("installed")).await;

        let result = execute_remote_search(sample_query(), None, &service).await;
        match result {
            Err(SearchError::Config(message)) => {
                assert!(message.contains("No repositories configured"));
            }
            other => panic!("expected missing repositories error, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn remote_search_rejects_missing_manager_and_unknown_filter() {
        let (_lock, temp_dir, _guard) = enter_temp_workspace();
        let mut unmanaged = FastSkillService::new(ServiceConfig {
            skill_storage_path: temp_dir.path().join("unmanaged"),
            ..Default::default()
        })
        .await
        .unwrap();
        unmanaged.initialize().await.unwrap();
        assert!(matches!(
            execute_remote_search(sample_query(), None, &unmanaged).await,
            Err(SearchError::Config(message)) if message.contains("No repositories configured")
        ));

        write_project_with_invalid_repo("known");
        let managed = injected_service(&temp_dir.path().join("managed")).await;
        assert!(matches!(
            execute_remote_search(sample_query(), Some("missing".to_string()), &managed).await,
            Err(SearchError::Config(message)) if message.contains("Repository 'missing' not found")
        ));
    }

    #[tokio::test]
    async fn remote_result_names_exact_install_command_and_partial_failures() {
        let (_lock, temp_dir, _guard) = enter_temp_workspace();
        let catalog = temp_dir.path().join("catalog");
        let skill = catalog.join("demo");
        fs::create_dir_all(&skill).unwrap();
        fs::write(
            skill.join("SKILL.md"),
            "---\nname: demo\nversion: \"1.2.3\"\ndescription: Test searchable demo\n---\n",
        )
        .unwrap();
        fs::write(
            "skill-project.toml",
            format!(
                r#"[dependencies]

[[tool.fastskill.repositories]]
name = "team"
type = "local"
priority = 0
path = "{}"

[[tool.fastskill.repositories]]
name = "broken"
type = "http-registry"
priority = 1
index_url = "not-a-valid-url"
"#,
                catalog.display()
            ),
        )
        .unwrap();
        let service = injected_service(&temp_dir.path().join("installed")).await;
        let unrelated = temp_dir.path().join("unrelated-cwd");
        fs::create_dir_all(&unrelated).unwrap();
        std::env::set_current_dir(&unrelated).unwrap();

        let execution = execute_remote_search(sample_query(), None, &service)
            .await
            .unwrap();
        assert_eq!(execution.results.len(), 1);
        assert_eq!(execution.results[0].id, "demo");
        assert_eq!(execution.results[0].version.as_deref(), Some("1.2.3"));
        assert_eq!(
            execution.results[0].install_command.as_deref(),
            Some("fastskill add demo@1.2.3 --repository team")
        );
        assert_eq!(execution.failures.len(), 1);
        assert_eq!(execution.failures[0].repository, "broken");
    }
}
