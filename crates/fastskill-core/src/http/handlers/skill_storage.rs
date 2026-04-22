//! Skill storage service for managing uploaded skills

use crate::core::service::FastSkillService;
use crate::security::validate_path_component;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::fs;
use uuid::Uuid;

/// Skill metadata stored locally
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillMetadata {
    pub id: String,
    pub name: String,
    pub description: String,
    pub directory: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub latest_version: String,
}

/// Skill version information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillVersion {
    pub id: String,
    pub skill_id: String,
    pub version: String, // epoch timestamp
    pub created_at: DateTime<Utc>,
    pub name: String,
    pub description: String,
    pub directory: String,
}

/// Skill storage service for managing uploaded skills
pub struct SkillStorage {
    #[allow(dead_code)] // Reserved for future functionality
    service: Arc<FastSkillService>,
    skills_dir: PathBuf,
}

impl SkillStorage {
    /// Create a new skill storage service
    pub fn new(service: Arc<FastSkillService>, skills_dir: PathBuf) -> Self {
        let canonical_skills_dir = Self::canonicalize_path(skills_dir);
        Self {
            service,
            skills_dir: canonical_skills_dir,
        }
    }

    /// Canonicalize a path if it exists, otherwise return as-is
    fn canonicalize_path(path: PathBuf) -> PathBuf {
        path.canonicalize().unwrap_or(path)
    }

    /// Generate a skill ID following Anthropic format (skill_01...)
    pub fn generate_skill_id() -> String {
        format!("skill_{}", Uuid::new_v4().simple())
    }

    /// Generate a version ID (epoch timestamp)
    pub fn generate_version_id() -> String {
        chrono::Utc::now().timestamp_millis().to_string()
    }

    /// Store a skill version from uploaded files
    pub async fn store_skill_version(
        &self,
        skill_id: &str,
        files: HashMap<String, Vec<u8>>,
    ) -> Result<SkillVersion, crate::http::errors::HttpError> {
        // Find SKILL.md file
        let skill_md_content = files
            .get("SKILL.md")
            .or_else(|| {
                // Look for files with SKILL.md in the path
                files
                    .keys()
                    .find(|k| k.ends_with("SKILL.md"))
                    .and_then(|k| files.get(k))
            })
            .ok_or_else(|| {
                crate::http::errors::HttpError::BadRequest(
                    "SKILL.md file not found in upload".to_string(),
                )
            })?;

        // Parse SKILL.md frontmatter
        let metadata = self.parse_skill_metadata(skill_md_content)?;

        // Extract directory name from file paths
        let directory_raw = self.extract_directory_name(&files)?;

        // Validate and get safe directory name for path construction
        let directory = validate_path_component(&directory_raw).map_err(|e| {
            crate::http::errors::HttpError::BadRequest(format!("Invalid directory name: {}", e))
        })?;

        // Create version
        let version_id = Self::generate_version_id();
        let version = SkillVersion {
            id: format!("skillver_{}", Uuid::new_v4().simple()),
            skill_id: skill_id.to_string(),
            version: version_id.clone(),
            created_at: Utc::now(),
            name: metadata.name,
            description: metadata
                .description
                .unwrap_or_else(|| "No description provided".to_string()),
            directory: directory.clone(),
        };

        // Store files - use validated directory string
        let skill_path = self.skills_dir.join(&directory);
        fs::create_dir_all(&skill_path).await.map_err(|e| {
            crate::http::errors::HttpError::InternalServerError(format!(
                "Failed to create skill directory: {}",
                e
            ))
        })?;

        for (filename, content) in files {
            // Validate each path component of filename to prevent path traversal
            let filename_safe = validate_path_component(&filename).map_err(|e| {
                crate::http::errors::HttpError::BadRequest(format!("Invalid filename: {}", e))
            })?;

            let file_path = skill_path.join(&filename_safe);

            // Ensure the file path is still under skill_path (prevent traversal)
            let canonical_skill_path = skill_path.canonicalize().map_err(|e| {
                crate::http::errors::HttpError::InternalServerError(format!(
                    "Failed to resolve skill path: {}",
                    e
                ))
            })?;

            // Ensure parent directories exist
            if let Some(parent) = file_path.parent() {
                fs::create_dir_all(parent).await.map_err(|e| {
                    crate::http::errors::HttpError::InternalServerError(format!(
                        "Failed to create parent directory: {}",
                        e
                    ))
                })?;
            }
            // Verify the resolved file path is under the canonical skill path
            if file_path.starts_with(&canonical_skill_path) {
                fs::write(file_path, content).await.map_err(|e| {
                    crate::http::errors::HttpError::InternalServerError(format!(
                        "Failed to write file: {}",
                        e
                    ))
                })?;
            } else {
                return Err(crate::http::errors::HttpError::BadRequest(format!(
                    "File path escapes skill directory: {}",
                    filename_safe
                )));
            }
        }

        // Update skill metadata
        self.update_skill_metadata(skill_id, &version).await?;

        Ok(version)
    }

    /// Parse SKILL.md frontmatter to extract metadata
    fn parse_skill_metadata(
        &self,
        content: &[u8],
    ) -> Result<SkillFrontmatter, crate::http::errors::HttpError> {
        let content_str = String::from_utf8(content.to_vec()).map_err(|e| {
            crate::http::errors::HttpError::BadRequest(format!("Invalid UTF-8 content: {}", e))
        })?;
        let lines: Vec<&str> = content_str.lines().collect();

        // Find YAML frontmatter (between ---)
        let mut in_frontmatter = false;
        let mut frontmatter_lines = Vec::new();

        for line in lines {
            if line.trim() == "---" {
                if in_frontmatter {
                    break; // End of frontmatter
                } else {
                    in_frontmatter = true;
                    continue;
                }
            }

            if in_frontmatter {
                frontmatter_lines.push(line);
            }
        }

        if frontmatter_lines.is_empty() {
            return Err(crate::http::errors::HttpError::BadRequest(
                "No YAML frontmatter found in SKILL.md".to_string(),
            ));
        }

        let yaml_content = frontmatter_lines.join("\n");
        let frontmatter: SkillFrontmatter = serde_yaml::from_str(&yaml_content).map_err(|e| {
            crate::http::errors::HttpError::BadRequest(format!("Invalid YAML frontmatter: {}", e))
        })?;

        Ok(frontmatter)
    }

    /// Extract directory name from uploaded file paths
    fn extract_directory_name(
        &self,
        files: &HashMap<String, Vec<u8>>,
    ) -> Result<String, crate::http::errors::HttpError> {
        // Find the common directory prefix
        let mut directories = Vec::new();

        for filename in files.keys() {
            if let Some(dir) = Path::new(filename).parent() {
                if let Some(dir_str) = dir.to_str() {
                    directories.push(dir_str.to_string());
                }
            }
        }

        // Find the most common directory (should be the skill directory)
        if directories.is_empty() {
            return Err(crate::http::errors::HttpError::BadRequest(
                "No directory structure found in uploaded files".to_string(),
            ));
        }

        // Use the first directory as the skill directory name
        // This assumes all files are under a single top-level directory
        let directory = directories.into_iter().next().ok_or_else(|| {
            crate::http::errors::HttpError::InternalServerError(
                "Unexpected empty directories list".to_string(),
            )
        })?;
        Ok(directory)
    }

    /// Update or create skill metadata
    async fn update_skill_metadata(
        &self,
        skill_id: &str,
        version: &SkillVersion,
    ) -> Result<(), crate::http::errors::HttpError> {
        let metadata_path = self.skills_dir.join("metadata.json");

        // Load existing metadata or create new
        let mut metadata: HashMap<String, SkillMetadata> = if metadata_path.exists() {
            let content = fs::read_to_string(&metadata_path).await.map_err(|e| {
                crate::http::errors::HttpError::InternalServerError(format!(
                    "Failed to read metadata: {}",
                    e
                ))
            })?;
            serde_json::from_str(&content).map_err(|e| {
                crate::http::errors::HttpError::InternalServerError(format!(
                    "Failed to parse metadata: {}",
                    e
                ))
            })?
        } else {
            HashMap::new()
        };

        // Update or create skill metadata
        let skill_meta = metadata
            .entry(skill_id.to_string())
            .or_insert(SkillMetadata {
                id: skill_id.to_string(),
                name: version.name.clone(),
                description: version.description.clone(),
                directory: version.directory.clone(),
                created_at: version.created_at,
                updated_at: version.created_at,
                latest_version: version.version.clone(),
            });

        skill_meta.updated_at = version.created_at;
        skill_meta.latest_version = version.version.clone();

        // Save metadata
        let content = serde_json::to_string_pretty(&metadata).map_err(|e| {
            crate::http::errors::HttpError::InternalServerError(format!(
                "Failed to serialize metadata: {}",
                e
            ))
        })?;
        fs::write(metadata_path, content).await.map_err(|e| {
            crate::http::errors::HttpError::InternalServerError(format!(
                "Failed to write metadata: {}",
                e
            ))
        })?;

        Ok(())
    }

    /// Get skill metadata by ID
    pub async fn get_skill(
        &self,
        skill_id: &str,
    ) -> Result<Option<SkillMetadata>, crate::http::errors::HttpError> {
        let metadata_path = self.skills_dir.join("metadata.json");

        if !metadata_path.exists() {
            return Ok(None);
        }

        let content = fs::read_to_string(metadata_path).await.map_err(|e| {
            crate::http::errors::HttpError::InternalServerError(format!(
                "Failed to read metadata: {}",
                e
            ))
        })?;
        let metadata: HashMap<String, SkillMetadata> =
            serde_json::from_str(&content).map_err(|e| {
                crate::http::errors::HttpError::InternalServerError(format!(
                    "Failed to parse metadata: {}",
                    e
                ))
            })?;

        Ok(metadata.get(skill_id).cloned())
    }

    /// List all skills
    pub async fn list_skills(&self) -> Result<Vec<SkillMetadata>, crate::http::errors::HttpError> {
        let metadata_path = self.skills_dir.join("metadata.json");

        if !metadata_path.exists() {
            return Ok(Vec::new());
        }

        let content = fs::read_to_string(metadata_path).await.map_err(|e| {
            crate::http::errors::HttpError::InternalServerError(format!(
                "Failed to read metadata: {}",
                e
            ))
        })?;
        let metadata: HashMap<String, SkillMetadata> =
            serde_json::from_str(&content).map_err(|e| {
                crate::http::errors::HttpError::InternalServerError(format!(
                    "Failed to parse metadata: {}",
                    e
                ))
            })?;

        Ok(metadata.values().cloned().collect())
    }

    /// Delete a skill and all its versions
    pub async fn delete_skill(&self, skill_id: &str) -> Result<(), crate::http::errors::HttpError> {
        // Get skill metadata
        let skill_meta = match self.get_skill(skill_id).await? {
            Some(meta) => meta,
            None => return Ok(()), // Skill doesn't exist
        };

        // Validate directory name and get safe string for path construction
        let directory_safe = validate_path_component(&skill_meta.directory).map_err(|e| {
            crate::http::errors::HttpError::InternalServerError(format!(
                "Invalid skill directory: {}",
                e
            ))
        })?;

        // Remove skill directory - use validated directory string
        let skill_path = self.skills_dir.join(&directory_safe);

        // Ensure the skill path is under skills_dir (prevent traversal)
        let canonical_skills_dir = self.skills_dir.canonicalize().map_err(|e| {
            crate::http::errors::HttpError::InternalServerError(format!(
                "Failed to resolve skills directory: {}",
                e
            ))
        })?;

        if skill_path.exists() {
            let canonical_skill_path = skill_path.canonicalize().map_err(|e| {
                crate::http::errors::HttpError::InternalServerError(format!(
                    "Failed to resolve skill path: {}",
                    e
                ))
            })?;

            if !canonical_skill_path.starts_with(&canonical_skills_dir) {
                return Err(crate::http::errors::HttpError::BadRequest(
                    "Skill path escapes skills directory".to_string(),
                ));
            }

            fs::remove_dir_all(canonical_skill_path)
                .await
                .map_err(|e| {
                    crate::http::errors::HttpError::InternalServerError(format!(
                        "Failed to remove skill directory: {}",
                        e
                    ))
                })?;
        }

        // Update metadata file
        let metadata_path = self.skills_dir.join("metadata.json");
        if metadata_path.exists() {
            let content = fs::read_to_string(&metadata_path).await.map_err(|e| {
                crate::http::errors::HttpError::InternalServerError(format!(
                    "Failed to read metadata: {}",
                    e
                ))
            })?;
            let mut metadata: HashMap<String, SkillMetadata> = serde_json::from_str(&content)
                .map_err(|e| {
                    crate::http::errors::HttpError::InternalServerError(format!(
                        "Failed to parse metadata: {}",
                        e
                    ))
                })?;
            metadata.remove(skill_id);

            let updated_content = serde_json::to_string_pretty(&metadata).map_err(|e| {
                crate::http::errors::HttpError::InternalServerError(format!(
                    "Failed to serialize metadata: {}",
                    e
                ))
            })?;
            fs::write(metadata_path, updated_content)
                .await
                .map_err(|e| {
                    crate::http::errors::HttpError::InternalServerError(format!(
                        "Failed to write metadata: {}",
                        e
                    ))
                })?;
        }

        Ok(())
    }
}

/// Frontmatter structure from SKILL.md
#[derive(Debug, Deserialize)]
struct SkillFrontmatter {
    name: String,
    description: Option<String>,
}

impl Default for SkillFrontmatter {
    fn default() -> Self {
        Self {
            name: "Unnamed Skill".to_string(),
            description: Some("No description provided".to_string()),
        }
    }
}
