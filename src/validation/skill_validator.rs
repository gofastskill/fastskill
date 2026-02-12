//! Skill validation implementation

use crate::core::service::ServiceError;
use crate::core::skill_manager::SkillDefinition;
use crate::validation::content_safety;
use crate::validation::dir_structure;
use crate::validation::extension_check::{self, ExtensionCheckConfig, ExtensionPreset};
use crate::validation::field_validation;
use crate::validation::file_structure;
use crate::validation::frontmatter;
use crate::validation::result::{ErrorSeverity, ValidationResult};
use std::path::Path;
use tokio::fs;

/// Skill validator for comprehensive validation
pub struct SkillValidator {
    /// Maximum allowed file size (MB)
    max_file_size_mb: usize,

    /// Maximum allowed skill description length
    max_description_length: usize,

    /// Required fields that must be present
    required_fields: Vec<String>,

    /// Dangerous patterns to check for in content
    dangerous_patterns: Vec<String>,
}

impl Default for SkillValidator {
    fn default() -> Self {
        Self::new()
    }
}

impl SkillValidator {
    /// Create a new skill validator with default settings
    pub fn new() -> Self {
        Self {
            max_file_size_mb: 10,
            max_description_length: 500,
            required_fields: vec![
                "name".to_string(),
                "description".to_string(),
                "version".to_string(),
            ],
            dangerous_patterns: content_safety::default_dangerous_patterns(),
        }
    }

    /// Create a new skill validator with custom settings
    pub fn with_config(
        max_file_size_mb: usize,
        max_description_length: usize,
        required_fields: Vec<String>,
    ) -> Self {
        Self {
            max_file_size_mb,
            max_description_length,
            required_fields,
            dangerous_patterns: content_safety::default_dangerous_patterns(),
        }
    }

    /// Validate a skill definition comprehensively
    pub async fn validate_skill(
        &self,
        skill: &SkillDefinition,
    ) -> Result<ValidationResult, ServiceError> {
        let mut result = ValidationResult::valid();

        result = field_validation::validate_required_fields(
            skill,
            result,
            &self.required_fields,
            self.max_description_length,
        );
        result = field_validation::validate_field_formats(skill, result);
        result =
            file_structure::validate_file_structure(skill, result, self.max_file_size_mb).await?;

        // Validate content safety
        result = self.validate_content_safety(skill, result).await?;

        // Validate YAML frontmatter if present
        result = self.validate_yaml_frontmatter(skill, result).await?;

        // Calculate final score
        result.calculate_score();

        Ok(result)
    }

    /// Validate content for safety issues
    async fn validate_content_safety(
        &self,
        skill: &SkillDefinition,
        result: ValidationResult,
    ) -> Result<ValidationResult, ServiceError> {
        let result = if skill.skill_file.exists() {
            content_safety::validate_skill_file_content(
                &skill.skill_file,
                result,
                &self.dangerous_patterns,
            )
            .await?
        } else {
            result
        };
        let script_files = skill.script_files.as_deref().unwrap_or(&[]);
        let mut result = result;
        for script_file in script_files {
            if script_file.exists() {
                result = content_safety::validate_script_file_content(
                    script_file,
                    result,
                    &self.dangerous_patterns,
                )
                .await?;
            }
        }
        Ok(result)
    }

    /// Validate YAML frontmatter format
    async fn validate_yaml_frontmatter(
        &self,
        skill: &SkillDefinition,
        mut result: ValidationResult,
    ) -> Result<ValidationResult, ServiceError> {
        if !skill.skill_file.exists() {
            return Ok(result);
        }
        let content = match fs::read_to_string(&skill.skill_file).await {
            Ok(c) => c,
            Err(e) => {
                result = result.with_error(
                    "yaml_frontmatter",
                    &format!("Cannot read SKILL.md for frontmatter validation: {}", e),
                    ErrorSeverity::Error,
                );
                return Ok(result);
            }
        };
        result = frontmatter::validate_content(&content, result);
        Ok(result)
    }

    /// Validate a skill directory structure
    pub async fn validate_skill_directory(
        &self,
        skill_path: &Path,
    ) -> Result<ValidationResult, ServiceError> {
        if !skill_path.exists() {
            return Ok(ValidationResult::invalid("Skill directory does not exist"));
        }
        if !skill_path.is_dir() {
            return Ok(ValidationResult::invalid("Skill path is not a directory"));
        }
        let result = dir_structure::ensure_skill_md_exists(skill_path, ValidationResult::valid());
        let (has_scripts, has_references, has_assets, mut result) =
            dir_structure::scan_skill_directory_entries(skill_path, result).await?;
        if has_scripts {
            result = self
                .validate_scripts_directory(&skill_path.join("scripts"), result)
                .await?;
        }
        if has_references {
            result = self
                .validate_references_directory(&skill_path.join("references"), result)
                .await?;
        }
        if has_assets {
            result = self
                .validate_assets_directory(&skill_path.join("assets"), result)
                .await?;
        }
        result.calculate_score();
        Ok(result)
    }

    /// Validate a directory by checking file extensions against an allowed list.
    async fn validate_extension_directory(
        &self,
        dir_path: &Path,
        mut result: ValidationResult,
        config: ExtensionCheckConfig<'_>,
    ) -> Result<ValidationResult, ServiceError> {
        if !dir_path.exists() {
            return Ok(result);
        }
        let mut entries = fs::read_dir(dir_path).await?;
        while let Some(entry) = entries.next_entry().await? {
            extension_check::process_file_extension(&entry.path(), &mut result, &config);
        }
        Ok(result)
    }

    /// Validate scripts directory
    async fn validate_scripts_directory(
        &self,
        scripts_path: &Path,
        result: ValidationResult,
    ) -> Result<ValidationResult, ServiceError> {
        self.validate_extension_directory(
            scripts_path,
            result,
            extension_check::extension_config(ExtensionPreset::Scripts),
        )
        .await
    }

    /// Validate references directory
    async fn validate_references_directory(
        &self,
        references_path: &Path,
        result: ValidationResult,
    ) -> Result<ValidationResult, ServiceError> {
        self.validate_extension_directory(
            references_path,
            result,
            extension_check::extension_config(ExtensionPreset::References),
        )
        .await
    }

    /// Validate assets directory
    async fn validate_assets_directory(
        &self,
        assets_path: &Path,
        mut result: ValidationResult,
    ) -> Result<ValidationResult, ServiceError> {
        if !assets_path.exists() {
            return Ok(result);
        }

        let mut entries = fs::read_dir(assets_path).await?;

        while let Some(entry) = entries.next_entry().await? {
            // Assets can be any type of file, just check if they exist and are readable
            let path = entry.path();

            if path.is_file() {
                // Check if file is readable
                match fs::metadata(&path).await {
                    Ok(_) => {}
                    Err(e) => {
                        result = result.with_warning(
                            "assets",
                            &format!("Cannot access asset file {}: {}", path.display(), e),
                        );
                    }
                }
            }
        }

        Ok(result)
    }

    fn apply_content_quality_checks(
        content: &str,
        description: &str,
        mut result: ValidationResult,
    ) -> ValidationResult {
        const IMPERATIVE: &[&str] = &[
            "To", "Use", "Run", "Execute", "Create", "Generate", "Process",
        ];
        let imperative_count = IMPERATIVE
            .iter()
            .filter(|i| content.contains(&format!("{} ", i)))
            .count();
        if imperative_count < 2 {
            result = result.with_warning(
                "content_quality",
                "Content may not follow imperative style (use verb-first instructions)",
            );
        }
        if !content.contains("Example") && !content.contains("example") {
            result = result.with_warning(
                "content_quality",
                "Consider adding examples to help users understand how to use the skill",
            );
        }
        const TRIGGER_WORDS: &[&str] = &[
            "extract", "process", "convert", "generate", "create", "send", "parse",
        ];
        let trigger_count = TRIGGER_WORDS
            .iter()
            .filter(|w| description.to_lowercase().contains(*w))
            .count();
        if trigger_count == 0 {
            result = result.with_warning(
                "content_quality",
                "Description should include clear trigger words indicating what the skill does",
            );
        }
        result
    }

    /// Qualitative validation (user-requested function)
    pub async fn qualitatively_validate_skill(
        &self,
        skill: &SkillDefinition,
    ) -> Result<ValidationResult, ServiceError> {
        let mut result = field_validation::validate_required_fields(
            skill,
            ValidationResult::valid(),
            &self.required_fields,
            self.max_description_length,
        );
        result =
            file_structure::validate_file_structure(skill, result, self.max_file_size_mb).await?;
        if !skill.skill_file.exists() {
            result.calculate_score();
            return Ok(result);
        }
        let content = match fs::read_to_string(&skill.skill_file).await {
            Ok(c) => c,
            Err(_) => {
                result = result.with_error(
                    "content_quality",
                    "Cannot read skill content for quality validation",
                    ErrorSeverity::Error,
                );
                result.calculate_score();
                return Ok(result);
            }
        };
        result = Self::apply_content_quality_checks(&content, &skill.description, result);
        result.calculate_score();
        Ok(result)
    }
}
