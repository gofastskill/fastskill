//! Semantic version bumping for skills

use crate::core::service::ServiceError;
use semver::Version;
use std::path::Path;

/// Version bump type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BumpType {
    Major,
    Minor,
    Patch,
}

/// Parse a version string into a SemVer version
pub fn parse_version(version: &str) -> Result<Version, ServiceError> {
    // Remove 'v' prefix if present
    let version_str = version.strip_prefix('v').unwrap_or(version);

    Version::parse(version_str)
        .map_err(|e| ServiceError::Custom(format!("Invalid version '{}': {}", version, e)))
}

/// Bump a version based on bump type
pub fn bump_version(version: &Version, bump_type: BumpType) -> Version {
    match bump_type {
        BumpType::Major => Version {
            major: version.major + 1,
            minor: 0,
            patch: 0,
            pre: semver::Prerelease::EMPTY,
            build: semver::BuildMetadata::EMPTY,
        },
        BumpType::Minor => Version {
            major: version.major,
            minor: version.minor + 1,
            patch: 0,
            pre: semver::Prerelease::EMPTY,
            build: semver::BuildMetadata::EMPTY,
        },
        BumpType::Patch => Version {
            major: version.major,
            minor: version.minor,
            patch: version.patch + 1,
            pre: semver::Prerelease::EMPTY,
            build: semver::BuildMetadata::EMPTY,
        },
    }
}

/// Update skill-project.toml with new version
pub fn update_skill_version(skill_path: &Path, new_version: &str) -> Result<(), ServiceError> {
    let skill_project_path = skill_path.join("skill-project.toml");

    if !skill_project_path.exists() {
        // Create new skill-project.toml with [metadata] section
        #[derive(serde::Serialize)]
        struct SkillProjectToml {
            metadata: SkillProjectMetadata,
        }

        #[derive(serde::Serialize)]
        struct SkillProjectMetadata {
            version: String,
        }

        let new_skill_project = SkillProjectToml {
            metadata: SkillProjectMetadata {
                version: new_version.to_string(),
            },
        };
        let content = toml::to_string_pretty(&new_skill_project).map_err(|e| {
            ServiceError::Custom(format!("Failed to serialize skill-project.toml: {}", e))
        })?;
        std::fs::write(&skill_project_path, content).map_err(ServiceError::Io)?;
        return Ok(());
    }

    let content = std::fs::read_to_string(&skill_project_path).map_err(ServiceError::Io)?;

    // Parse the TOML structure with [metadata] section
    #[derive(serde::Deserialize, serde::Serialize)]
    struct SkillProjectToml {
        #[serde(default)]
        metadata: Option<SkillProjectMetadata>,
        #[serde(default)]
        dependencies: Option<toml::Value>,
    }

    #[derive(serde::Deserialize, serde::Serialize)]
    struct SkillProjectMetadata {
        version: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        author: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        tags: Option<Vec<String>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        capabilities: Option<Vec<String>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        download_url: Option<String>,
    }

    if let Ok(mut skill_project) = toml::from_str::<SkillProjectToml>(&content) {
        if let Some(ref mut metadata) = skill_project.metadata {
            metadata.version = new_version.to_string();
        } else {
            skill_project.metadata = Some(SkillProjectMetadata {
                version: new_version.to_string(),
                name: None,
                description: None,
                author: None,
                tags: None,
                capabilities: None,
                download_url: None,
            });
        }
        let content = toml::to_string_pretty(&skill_project).map_err(|e| {
            ServiceError::Custom(format!("Failed to serialize skill-project.toml: {}", e))
        })?;
        std::fs::write(&skill_project_path, content).map_err(ServiceError::Io)?;
        Ok(())
    } else {
        // If parsing fails, create new structure with [metadata] section
        let new_skill_project = SkillProjectToml {
            metadata: Some(SkillProjectMetadata {
                version: new_version.to_string(),
                name: None,
                description: None,
                author: None,
                tags: None,
                capabilities: None,
                download_url: None,
            }),
            dependencies: None,
        };
        let content = toml::to_string_pretty(&new_skill_project).map_err(|e| {
            ServiceError::Custom(format!("Failed to serialize skill-project.toml: {}", e))
        })?;
        std::fs::write(&skill_project_path, content).map_err(ServiceError::Io)?;
        Ok(())
    }
}

/// Get current version from skill-project.toml
pub fn get_current_version(skill_path: &Path) -> Result<Option<String>, ServiceError> {
    let skill_project_path = skill_path.join("skill-project.toml");

    if !skill_project_path.exists() {
        return Ok(None);
    }

    let content = std::fs::read_to_string(&skill_project_path).map_err(ServiceError::Io)?;

    #[derive(serde::Deserialize)]
    struct SkillProjectToml {
        metadata: Option<SkillProjectMetadata>,
    }

    #[derive(serde::Deserialize)]
    struct SkillProjectMetadata {
        version: Option<String>,
    }

    if let Ok(skill_project) = toml::from_str::<SkillProjectToml>(&content) {
        if let Some(metadata) = skill_project.metadata {
            if let Some(version) = metadata.version {
                if !version.is_empty() {
                    return Ok(Some(version));
                }
            }
        }
    }

    Ok(None)
}
