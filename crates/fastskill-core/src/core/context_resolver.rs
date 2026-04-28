use crate::core::embedding::EmbeddingService;
use crate::core::metadata::MetadataService;
use crate::core::service::{EmbeddingConfig, ServiceError, SkillId};
use crate::core::skill_manager::SkillManagementService;
use crate::core::vector_index::VectorIndexService;
use crate::security::path::validate_path_within_root;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;

const MAX_CONTENT_SIZE: u64 = 512_000;
const PREVIEW_BODY_LINES: usize = 20;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ResolveScope {
    Local,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ContentMode {
    #[default]
    None,
    Preview,
    Full,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolveContextRequest {
    pub prompt: String,
    pub limit: usize,
    pub scope: ResolveScope,
    #[serde(default)]
    pub include_content: ContentMode,
    #[serde(default = "default_true")]
    pub resolve_paths: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedSkill {
    pub skill_id: String,
    pub name: String,
    pub description: String,
    pub score: f32,
    pub skill_md_path: Option<String>,
    pub skill_root_path: Option<String>,
    pub references_dir_path: Option<String>,
    pub assets_dir_path: Option<String>,
    pub content_preview: Option<String>,
    pub content_full: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolveContextResponse {
    pub query: String,
    pub scope: ResolveScope,
    pub results: Vec<ResolvedSkill>,
    pub allowed_roots: Vec<String>,
}

pub struct ContextResolver {
    skill_manager: Arc<dyn SkillManagementService>,
    metadata_service: Arc<dyn MetadataService>,
    vector_index_service: Option<Arc<dyn VectorIndexService>>,
    embedding_config: Option<EmbeddingConfig>,
    skills_root: PathBuf,
}

impl ContextResolver {
    pub fn new(
        skill_manager: Arc<dyn SkillManagementService>,
        metadata_service: Arc<dyn MetadataService>,
        vector_index_service: Option<Arc<dyn VectorIndexService>>,
        embedding_config: Option<EmbeddingConfig>,
        skills_root: PathBuf,
    ) -> Self {
        Self {
            skill_manager,
            metadata_service,
            vector_index_service,
            embedding_config,
            skills_root,
        }
    }

    pub async fn resolve_context(
        &self,
        request: ResolveContextRequest,
    ) -> Result<ResolveContextResponse, ServiceError> {
        if request.prompt.trim().is_empty() {
            return Err(ServiceError::Validation(
                "RESOLVE_EMPTY_PROMPT: prompt cannot be empty or whitespace-only".to_string(),
            ));
        }

        if request.limit == 0 {
            return Err(ServiceError::Validation(
                "RESOLVE_LIMIT_ZERO: limit must be greater than 0".to_string(),
            ));
        }

        let search_results = self.perform_search(&request).await?;

        let mut resolved = Vec::new();
        for (skill_id_str, name, description, score) in search_results {
            let skill_id = match SkillId::new(skill_id_str.clone()) {
                Ok(id) => id,
                Err(_) => continue,
            };

            let skill_def = match self.skill_manager.get_skill(&skill_id).await {
                Ok(Some(def)) => def,
                Ok(None) => {
                    tracing::warn!(
                        "RESOLVE_SKILL_NOT_INDEXED: skill '{}' found in search but missing from SkillManager",
                        skill_id_str
                    );
                    continue;
                }
                Err(e) => {
                    tracing::warn!(
                        "RESOLVE_SKILL_NOT_INDEXED: lookup failed for '{}': {}",
                        skill_id_str,
                        e
                    );
                    continue;
                }
            };

            let (skill_md_path, skill_root_path, references_dir_path, assets_dir_path) =
                if request.resolve_paths {
                    self.resolve_paths(&skill_def.skill_file)?
                } else {
                    (None, None, None, None)
                };

            let (content_preview, content_full) = self
                .read_content(&skill_def.skill_file, &request.include_content)
                .await?;

            resolved.push(ResolvedSkill {
                skill_id: skill_id_str,
                name,
                description,
                score,
                skill_md_path,
                skill_root_path,
                references_dir_path,
                assets_dir_path,
                content_preview,
                content_full,
            });

            if resolved.len() >= request.limit {
                break;
            }
        }

        let allowed_roots = vec![self.skills_root.to_string_lossy().to_string()];

        Ok(ResolveContextResponse {
            query: request.prompt,
            scope: request.scope,
            results: resolved,
            allowed_roots,
        })
    }

    async fn perform_search(
        &self,
        request: &ResolveContextRequest,
    ) -> Result<Vec<(String, String, String, f32)>, ServiceError> {
        let embedding_attempt = self
            .try_embedding_search(&request.prompt, request.limit)
            .await;

        if let Ok(results) = embedding_attempt {
            return Ok(results);
        }

        self.text_search(&request.prompt, request.limit).await
    }

    async fn try_embedding_search(
        &self,
        prompt: &str,
        limit: usize,
    ) -> Result<Vec<(String, String, String, f32)>, ServiceError> {
        let embedding_config = self
            .embedding_config
            .as_ref()
            .ok_or_else(|| ServiceError::Config("RESOLVE_EMBEDDING_CONFIG_MISSING".to_string()))?;

        let vector_service = self
            .vector_index_service
            .as_ref()
            .ok_or_else(|| ServiceError::Config("RESOLVE_EMBEDDING_CONFIG_MISSING".to_string()))?;

        let api_key = std::env::var("OPENAI_API_KEY")
            .map_err(|_| ServiceError::Config("RESOLVE_EMBEDDING_CONFIG_MISSING".to_string()))?;

        if api_key.trim().is_empty() {
            return Err(ServiceError::Config(
                "RESOLVE_EMBEDDING_CONFIG_MISSING".to_string(),
            ));
        }

        let embedding_service =
            crate::OpenAIEmbeddingService::from_config(embedding_config, api_key);
        let query_embedding = embedding_service
            .embed_query(prompt)
            .await
            .map_err(|e| ServiceError::Custom(format!("Embedding failed: {}", e)))?;

        let matches = vector_service
            .search_similar(&query_embedding, limit)
            .await
            .map_err(|e| ServiceError::Custom(format!("Vector search failed: {}", e)))?;

        Ok(matches
            .into_iter()
            .map(|m| {
                let name = m
                    .skill
                    .frontmatter_json
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or(&m.skill.id)
                    .to_string();
                let description = m
                    .skill
                    .frontmatter_json
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                (m.skill.id, name, description, m.similarity)
            })
            .collect())
    }

    async fn text_search(
        &self,
        prompt: &str,
        limit: usize,
    ) -> Result<Vec<(String, String, String, f32)>, ServiceError> {
        let meta_list = self.metadata_service.search_skills(prompt).await?;

        Ok(meta_list
            .into_iter()
            .take(limit)
            .map(|m| {
                let name = if m.name.is_empty() {
                    m.id.as_str().to_string()
                } else {
                    m.name
                };
                (m.id.into_string(), name, m.description, 1.0)
            })
            .collect())
    }

    #[allow(clippy::type_complexity)]
    fn resolve_paths(
        &self,
        skill_file: &Path,
    ) -> Result<
        (
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
        ),
        ServiceError,
    > {
        let skill_md = self.canonicalize_within_root(skill_file)?;
        let root = skill_file.parent();
        let skill_root = match root {
            Some(r) => self.canonicalize_within_root(r)?,
            None => None,
        };

        let references_dir = root.and_then(|r| {
            let p = r.join("references");
            if p.is_dir() {
                self.canonicalize_within_root(&p).ok().flatten()
            } else {
                None
            }
        });
        let assets_dir = root.and_then(|r| {
            let p = r.join("assets");
            if p.is_dir() {
                self.canonicalize_within_root(&p).ok().flatten()
            } else {
                None
            }
        });

        Ok((skill_md, skill_root, references_dir, assets_dir))
    }

    fn canonicalize_within_root(&self, path: &Path) -> Result<Option<String>, ServiceError> {
        match validate_path_within_root(path, &self.skills_root) {
            Ok(canonical) => Ok(Some(canonical.to_string_lossy().to_string())),
            Err(e) => {
                tracing::warn!(
                    "RESOLVE_PATH_ESCAPE: path '{}' validation failed: {}",
                    path.display(),
                    e
                );
                Ok(None)
            }
        }
    }

    async fn read_content(
        &self,
        skill_file: &Path,
        mode: &ContentMode,
    ) -> Result<(Option<String>, Option<String>), ServiceError> {
        if mode == &ContentMode::None {
            return Ok((None, None));
        }

        if !skill_file.exists() {
            tracing::warn!(
                "RESOLVE_READ_FAILED: skill file does not exist: {}",
                skill_file.display()
            );
            return Ok((None, None));
        }

        let metadata = match tokio::fs::metadata(skill_file).await {
            Ok(m) => m,
            Err(e) => {
                tracing::warn!(
                    "RESOLVE_READ_FAILED: cannot stat '{}': {}",
                    skill_file.display(),
                    e
                );
                return Ok((None, None));
            }
        };

        let content = match tokio::fs::read_to_string(skill_file).await {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(
                    "RESOLVE_READ_FAILED: cannot read '{}': {}",
                    skill_file.display(),
                    e
                );
                return Ok((None, None));
            }
        };

        match mode {
            ContentMode::None => Ok((None, None)),
            ContentMode::Preview => {
                let preview = self.extract_preview(&content);
                Ok((Some(preview), None))
            }
            ContentMode::Full => {
                if metadata.len() > MAX_CONTENT_SIZE {
                    tracing::warn!(
                        "RESOLVE_CONTENT_TOO_LARGE: '{}' exceeds {} bytes",
                        skill_file.display(),
                        MAX_CONTENT_SIZE
                    );
                    Ok((None, None))
                } else {
                    Ok((None, Some(content)))
                }
            }
        }
    }

    fn extract_preview(&self, content: &str) -> String {
        let lines: Vec<&str> = content.lines().collect();
        let mut frontmatter_end = 0usize;
        let mut in_fm = false;
        let mut fm_started = false;

        for (i, line) in lines.iter().enumerate() {
            if line.trim() == "---" {
                if !fm_started {
                    fm_started = true;
                    in_fm = true;
                } else if in_fm {
                    frontmatter_end = i + 1;
                    in_fm = false;
                }
            }
        }

        let start = if frontmatter_end > 0 {
            frontmatter_end
        } else {
            0
        };

        let mut result_lines: Vec<&str> = Vec::new();

        if frontmatter_end > 0 {
            result_lines.extend_from_slice(&lines[..frontmatter_end]);
        }

        let body_lines: Vec<&&str> = lines[start..].iter().take(PREVIEW_BODY_LINES).collect();
        result_lines.extend(body_lines.iter().map(|s| **s));

        result_lines.join("\n")
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn test_content_mode_default_is_none() {
        assert_eq!(ContentMode::default(), ContentMode::None);
    }

    #[test]
    fn test_resolve_scope_serde() {
        let scope = ResolveScope::Local;
        let json = serde_json::to_string(&scope).unwrap();
        assert_eq!(json, "\"local\"");
        let back: ResolveScope = serde_json::from_str(&json).unwrap();
        assert_eq!(back, ResolveScope::Local);
    }

    #[test]
    fn test_content_mode_serde() {
        let mode = ContentMode::Preview;
        let json = serde_json::to_string(&mode).unwrap();
        assert_eq!(json, "\"preview\"");
        let back: ContentMode = serde_json::from_str(&json).unwrap();
        assert_eq!(back, ContentMode::Preview);
    }

    #[test]
    fn test_extract_preview_frontmatter_plus_body() {
        let content = "---\nname: test\ndescription: a test\n---\nline1\nline2\nline3\n";
        let temp_dir = tempfile::TempDir::new().unwrap();
        let skills_root = temp_dir.path().to_path_buf();

        let resolver = ContextResolver::new(
            Arc::new(crate::core::skill_manager::SkillManager::new()),
            Arc::new(crate::core::metadata::MetadataServiceImpl::new(Arc::new(
                crate::core::skill_manager::SkillManager::new(),
            ))),
            None,
            None,
            skills_root,
        );

        let preview = resolver.extract_preview(content);
        assert!(preview.contains("name: test"));
        assert!(preview.contains("line1"));
        assert!(preview.contains("line2"));
        assert!(preview.contains("line3"));
    }

    #[test]
    fn test_extract_preview_truncates_at_20_body_lines() {
        let body: Vec<String> = (1..=50).map(|i| format!("body line {}", i)).collect();
        let content = format!("---\nname: test\n---\n{}", body.join("\n"));

        let temp_dir = tempfile::TempDir::new().unwrap();
        let skills_root = temp_dir.path().to_path_buf();

        let resolver = ContextResolver::new(
            Arc::new(crate::core::skill_manager::SkillManager::new()),
            Arc::new(crate::core::metadata::MetadataServiceImpl::new(Arc::new(
                crate::core::skill_manager::SkillManager::new(),
            ))),
            None,
            None,
            skills_root,
        );

        let preview = resolver.extract_preview(&content);
        assert!(preview.contains("body line 20"));
        assert!(!preview.contains("body line 21"));
    }

    #[tokio::test]
    async fn test_resolve_empty_prompt_returns_error() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let resolver = ContextResolver::new(
            Arc::new(crate::core::skill_manager::SkillManager::new()),
            Arc::new(crate::core::metadata::MetadataServiceImpl::new(Arc::new(
                crate::core::skill_manager::SkillManager::new(),
            ))),
            None,
            None,
            temp_dir.path().to_path_buf(),
        );

        let request = ResolveContextRequest {
            prompt: "  ".to_string(),
            limit: 5,
            scope: ResolveScope::Local,
            include_content: ContentMode::None,
            resolve_paths: true,
        };

        let result = resolver.resolve_context(request).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("RESOLVE_EMPTY_PROMPT"));
    }

    #[tokio::test]
    async fn test_resolve_limit_zero_returns_error() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let resolver = ContextResolver::new(
            Arc::new(crate::core::skill_manager::SkillManager::new()),
            Arc::new(crate::core::metadata::MetadataServiceImpl::new(Arc::new(
                crate::core::skill_manager::SkillManager::new(),
            ))),
            None,
            None,
            temp_dir.path().to_path_buf(),
        );

        let request = ResolveContextRequest {
            prompt: "test".to_string(),
            limit: 0,
            scope: ResolveScope::Local,
            include_content: ContentMode::None,
            resolve_paths: true,
        };

        let result = resolver.resolve_context(request).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("RESOLVE_LIMIT_ZERO"));
    }

    #[tokio::test]
    async fn test_resolve_context_returns_allowed_roots() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let skills_root = temp_dir.path().to_path_buf();

        let resolver = ContextResolver::new(
            Arc::new(crate::core::skill_manager::SkillManager::new()),
            Arc::new(crate::core::metadata::MetadataServiceImpl::new(Arc::new(
                crate::core::skill_manager::SkillManager::new(),
            ))),
            None,
            None,
            skills_root.clone(),
        );

        let request = ResolveContextRequest {
            prompt: "anything".to_string(),
            limit: 5,
            scope: ResolveScope::Local,
            include_content: ContentMode::None,
            resolve_paths: true,
        };

        let response = resolver.resolve_context(request).await.unwrap();
        assert_eq!(response.allowed_roots.len(), 1);
        assert_eq!(response.query, "anything");
        assert!(response.results.is_empty());
    }

    #[tokio::test]
    async fn test_resolve_paths_within_root() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let skills_dir = temp_dir.path().join("skills");
        std::fs::create_dir_all(&skills_dir).unwrap();

        let skill_dir = skills_dir.join("my-skill");
        std::fs::create_dir_all(&skill_dir).unwrap();
        let skill_file = skill_dir.join("SKILL.md");
        std::fs::write(
            &skill_file,
            "---\nname: test\ndescription: test\n---\ncontent",
        )
        .unwrap();

        let resolver = ContextResolver::new(
            Arc::new(crate::core::skill_manager::SkillManager::new()),
            Arc::new(crate::core::metadata::MetadataServiceImpl::new(Arc::new(
                crate::core::skill_manager::SkillManager::new(),
            ))),
            None,
            None,
            skills_dir.clone(),
        );

        let (md_path, root_path, refs, assets) = resolver.resolve_paths(&skill_file).unwrap();

        assert!(md_path.is_some());
        let md = md_path.unwrap();
        assert!(md.contains("my-skill"));

        assert!(root_path.is_some());
        assert!(refs.is_none());
        assert!(assets.is_none());
    }
}
