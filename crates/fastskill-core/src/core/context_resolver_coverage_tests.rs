#![allow(clippy::unwrap_used)]
use super::*;
use crate::core::metadata::MetadataServiceImpl;
use crate::core::origin::Origin;
use crate::core::skill_manager::{SkillDefinition, SkillManager};

fn resolver(root: &Path) -> ContextResolver {
    let manager = Arc::new(SkillManager::new());
    ContextResolver::new(
        manager.clone(),
        Arc::new(MetadataServiceImpl::new(manager)),
        None,
        None,
        root.into(),
    )
}

#[tokio::test]
async fn content_modes_handle_missing_invalid_large_and_preview_files() {
    let temp = tempfile::tempdir().unwrap();
    let resolver = resolver(temp.path());
    let file = temp.path().join("SKILL.md");
    assert_eq!(
        resolver
            .read_content(&file, &ContentMode::None)
            .await
            .unwrap(),
        (None, None)
    );
    assert_eq!(
        resolver
            .read_content(&file, &ContentMode::Full)
            .await
            .unwrap(),
        (None, None)
    );
    std::fs::write(&file, [0xff, 0xfe]).unwrap();
    assert_eq!(
        resolver
            .read_content(&file, &ContentMode::Full)
            .await
            .unwrap(),
        (None, None)
    );
    std::fs::write(&file, "plain body").unwrap();
    assert_eq!(
        resolver
            .read_content(&file, &ContentMode::Preview)
            .await
            .unwrap(),
        (Some("plain body".into()), None)
    );
    assert_eq!(
        resolver
            .read_content(&file, &ContentMode::Full)
            .await
            .unwrap(),
        (None, Some("plain body".into()))
    );
    std::fs::write(&file, "x".repeat(MAX_CONTENT_SIZE as usize + 1)).unwrap();
    assert_eq!(
        resolver
            .read_content(&file, &ContentMode::Full)
            .await
            .unwrap(),
        (None, None)
    );
    assert_eq!(
        resolver.extract_preview("---\nunclosed\nbody"),
        "---\nunclosed\nbody"
    );
}

#[tokio::test]
async fn resolve_includes_content_optional_directories_and_honors_path_toggle() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("sample");
    std::fs::create_dir_all(root.join("references")).unwrap();
    std::fs::create_dir_all(root.join("assets")).unwrap();
    let mut resolver = resolver(temp.path());
    let mut skill = SkillDefinition::new(
        SkillId::new("sample".into()).unwrap(),
        "Sample".into(),
        "sample description".into(),
        "1.0.0".into(),
        Origin::Local {
            path: root.clone(),
            editable: false,
        },
    );
    skill.skill_file = root.join("SKILL.md");
    std::fs::write(&skill.skill_file, "body").unwrap();
    resolver.skill_manager.register_skill(skill).await.unwrap();
    // A configured embedding endpoint without a vector index must fall back to text.
    resolver.embedding_config = Some(EmbeddingConfig {
        openai_base_url: "http://127.0.0.1:1".into(),
        embedding_model: "test".into(),
        index_path: None,
    });
    for resolve_paths in [true, false] {
        let result = resolver
            .resolve_context(ResolveContextRequest {
                prompt: "sample".into(),
                limit: 1,
                scope: ResolveScope::Local,
                include_content: ContentMode::Full,
                resolve_paths,
            })
            .await
            .unwrap();
        assert_eq!(result.results.len(), 1);
        let skill = &result.results[0];
        assert_eq!(skill.content_full.as_deref(), Some("body"));
        assert_eq!(skill.references_dir_path.is_some(), resolve_paths);
        assert_eq!(skill.assets_dir_path.is_some(), resolve_paths);
        assert_eq!(
            result.allowed_roots.len(),
            if resolve_paths { 2 } else { 1 }
        );
    }
}

#[test]
fn paths_reject_parent_components_and_unmanaged_locations() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("skills");
    std::fs::create_dir(&root).unwrap();
    let outside = temp.path().join("outside");
    std::fs::write(&outside, "secret").unwrap();
    let resolver = resolver(&root);
    for path in [outside, root.join("../outside")] {
        assert!(resolver.canonicalize_within_root(&path).unwrap().is_none());
    }
    // Containment also accepts a not-yet-created leaf under a valid root.
    assert!(resolver
        .canonicalize_within_root(&root.join("missing/SKILL.md"))
        .unwrap()
        .is_some());
}

#[test]
fn editable_entry_checks_support_a_symlinked_managed_parent() {
    let temp = tempfile::tempdir().unwrap();
    let managed = temp.path().join("managed");
    let source = temp.path().join("source");
    let alias = temp.path().join("alias");
    std::fs::create_dir(&managed).unwrap();
    std::fs::create_dir(&source).unwrap();
    std::fs::write(source.join("SKILL.md"), "body").unwrap();
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&managed, &alias).unwrap();
        std::os::unix::fs::symlink(&source, managed.join("sample")).unwrap();
    }
    #[cfg(windows)]
    {
        std::os::windows::fs::symlink_dir(&managed, &alias).unwrap();
        std::os::windows::fs::symlink_dir(&source, managed.join("sample")).unwrap();
    }
    let resolver = resolver(&alias);
    assert_eq!(
        resolver
            .canonicalize_within_root(&alias.join("sample/SKILL.md"))
            .unwrap(),
        Some(
            source
                .join("SKILL.md")
                .canonicalize()
                .unwrap()
                .display()
                .to_string()
        )
    );
    assert!(resolver
        .canonicalize_within_root(&alias.join("sample/../../outside"))
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn embedding_results_and_failed_http_fallback_are_resolved() {
    // Scope the API key to a child process, never mutate shared process env.
    if std::env::var_os("FASTSKILL_RESOLVER_TEST_CHILD").is_none() {
        let result = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "core::context_resolver::coverage_tests::embedding_results_and_failed_http_fallback_are_resolved", "--nocapture"])
            .env("FASTSKILL_RESOLVER_TEST_CHILD", "1")
            .env("OPENAI_API_KEY", "fixture-only")
            .status().unwrap();
        assert!(result.success());
        return;
    }
    use crate::core::vector_index::VectorIndexServiceImpl;
    use wiremock::{matchers::path, Mock, MockServer, ResponseTemplate};
    let temp = tempfile::tempdir().unwrap();
    let server = MockServer::start().await;
    let index = Arc::new(VectorIndexServiceImpl::new(temp.path().join("index.db")));
    for (id, metadata) in [
        (
            "sample",
            serde_json::json!({"name":"Sample", "description":"description"}),
        ),
        ("unregistered", serde_json::json!({})),
        ("../invalid", serde_json::json!({})),
    ] {
        index
            .add_or_update_skill(id, temp.path().into(), metadata, vec![1.0, 0.0], "hash")
            .await
            .unwrap();
    }
    let mut resolver = resolver(temp.path());
    let mut skill = SkillDefinition::new(
        SkillId::new("sample".into()).unwrap(),
        "Sample".into(),
        "sample description".into(),
        "1.0.0".into(),
        Origin::Local {
            path: temp.path().into(),
            editable: false,
        },
    );
    skill.skill_file = temp.path().join("SKILL.md");
    std::fs::write(&skill.skill_file, "body").unwrap();
    resolver.skill_manager.register_skill(skill).await.unwrap();
    resolver.vector_index_service = Some(index);
    resolver.embedding_config = Some(EmbeddingConfig {
        openai_base_url: server.uri(),
        embedding_model: "fixture".into(),
        index_path: None,
    });
    Mock::given(path("/embeddings"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"data":[{"embedding":[1.0,0.0]}]})),
        )
        .mount(&server)
        .await;
    let matches = resolver.try_embedding_search("query", 10).await.unwrap();
    assert_eq!(matches.len(), 3);
    assert!(matches
        .iter()
        .any(|m| m.1 == "unregistered" && m.2.is_empty()));
    let request = ResolveContextRequest {
        prompt: "sample".into(),
        limit: 10,
        scope: ResolveScope::Local,
        include_content: ContentMode::None,
        resolve_paths: false,
    };
    let response = resolver.resolve_context(request.clone()).await.unwrap();
    assert_eq!(response.results.len(), 1);
    assert_eq!(response.results[0].skill_id, "sample");
    server.reset().await;
    Mock::given(path("/embeddings"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;
    assert!(resolver
        .try_embedding_search("query", 10)
        .await
        .unwrap_err()
        .to_string()
        .contains("Embedding failed"));
    assert_eq!(
        resolver
            .resolve_context(request)
            .await
            .unwrap()
            .results
            .len(),
        1
    );
}
