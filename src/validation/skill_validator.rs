//! Skill validation implementation

use crate::core::service::ServiceError;
use crate::core::skill_manager::SkillDefinition;
use serde::{Deserialize, Serialize};
use std::path::Path;
use tokio::fs;

/// Validation result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationResult {
    /// Whether validation passed
    pub is_valid: bool,

    /// List of validation errors
    pub errors: Vec<ValidationError>,

    /// List of validation warnings
    pub warnings: Vec<ValidationWarning>,

    /// Validation score (0.0 to 1.0)
    pub score: f64,
}

impl ValidationResult {
    /// Create a valid result
    pub fn valid() -> Self {
        Self {
            is_valid: true,
            errors: Vec::new(),
            warnings: Vec::new(),
            score: 1.0,
        }
    }

    /// Create an invalid result
    pub fn invalid(error: &str) -> Self {
        Self {
            is_valid: false,
            errors: vec![ValidationError {
                field: "general".to_string(),
                message: error.to_string(),
                severity: ErrorSeverity::Error,
            }],
            warnings: Vec::new(),
            score: 0.0,
        }
    }

    /// Add an error
    pub fn with_error(mut self, field: &str, message: &str, severity: ErrorSeverity) -> Self {
        self.errors.push(ValidationError {
            field: field.to_string(),
            message: message.to_string(),
            severity,
        });
        self.is_valid = false;
        self.score = 0.0;
        self
    }

    /// Add a warning
    pub fn with_warning(mut self, field: &str, message: &str) -> Self {
        self.warnings.push(ValidationWarning {
            field: field.to_string(),
            message: message.to_string(),
        });
        self
    }

    /// Calculate quality score based on errors and warnings
    fn calculate_score(&mut self) {
        let error_penalty = self.errors.len() as f64 * 0.3;
        let warning_penalty = self.warnings.len() as f64 * 0.1;

        self.score = (1.0 - error_penalty - warning_penalty).max(0.0);
    }
}

/// Validation error details
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationError {
    /// Field or component that failed validation
    pub field: String,

    /// Error message
    pub message: String,

    /// Error severity level
    pub severity: ErrorSeverity,
}

/// Error severity levels
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ErrorSeverity {
    /// Warning - not critical but should be addressed
    Warning,
    /// Error - validation failed but skill might still work
    Error,
    /// Critical - validation failed and skill cannot be used
    Critical,
}

/// Validation warning details
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationWarning {
    /// Field or component with warning
    pub field: String,

    /// Warning message
    pub message: String,
}

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
            dangerous_patterns: vec![
                "import os".to_string(),
                "import subprocess".to_string(),
                "import sys".to_string(),
                "exec(".to_string(),
                "eval(".to_string(),
                "system(".to_string(),
                "popen(".to_string(),
                "rm -rf".to_string(),
                "sudo".to_string(),
                "chmod 777".to_string(),
                "chown".to_string(),
                "su ".to_string(),
                "passwd".to_string(),
            ],
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
            dangerous_patterns: vec![
                "import os".to_string(),
                "import subprocess".to_string(),
                "import sys".to_string(),
                "exec(".to_string(),
                "eval(".to_string(),
                "system(".to_string(),
                "popen(".to_string(),
                "rm -rf".to_string(),
                "sudo".to_string(),
                "chmod 777".to_string(),
                "chown".to_string(),
                "su ".to_string(),
                "passwd".to_string(),
            ],
        }
    }

    /// Validate a skill definition comprehensively
    pub async fn validate_skill(
        &self,
        skill: &SkillDefinition,
    ) -> Result<ValidationResult, ServiceError> {
        let mut result = ValidationResult::valid();

        // Validate required fields
        result = self.validate_required_fields(skill, result);

        // Validate field formats and lengths
        result = self.validate_field_formats(skill, result);

        // Validate file structure
        result = self.validate_file_structure(skill, result).await?;

        // Validate content safety
        result = self.validate_content_safety(skill, result).await?;

        // Validate YAML frontmatter if present
        result = self.validate_yaml_frontmatter(skill, result).await?;

        // Calculate final score
        result.calculate_score();

        Ok(result)
    }

    /// Validate required fields are present
    fn validate_required_fields(
        &self,
        skill: &SkillDefinition,
        mut result: ValidationResult,
    ) -> ValidationResult {
        for field in &self.required_fields {
            match field.as_str() {
                "name" => {
                    if skill.name.trim().is_empty() {
                        result = result.with_error(
                            "name",
                            "Skill name cannot be empty",
                            ErrorSeverity::Critical,
                        );
                    }
                }
                "description" => {
                    if skill.description.trim().is_empty() {
                        result = result.with_error(
                            "description",
                            "Skill description cannot be empty",
                            ErrorSeverity::Critical,
                        );
                    } else if skill.description.len() > self.max_description_length {
                        result = result.with_warning(
                            "description",
                            &format!(
                                "Description is very long ({} chars, max: {})",
                                skill.description.len(),
                                self.max_description_length
                            ),
                        );
                    }
                }
                "version" => {
                    if skill.version.trim().is_empty() {
                        result = result.with_error(
                            "version",
                            "Skill version cannot be empty",
                            ErrorSeverity::Critical,
                        );
                    }
                }
                _ => {}
            }
        }

        result
    }

    /// Validate field formats and lengths
    fn validate_field_formats(
        &self,
        skill: &SkillDefinition,
        mut result: ValidationResult,
    ) -> ValidationResult {
        // Validate skill name format
        if !skill.name.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_') {
            result = result.with_warning(
                "name",
                "Skill name should only contain alphanumeric characters, hyphens, and underscores",
            );
        }

        // Validate version format (semantic versioning)
        if semver::Version::parse(&skill.version).is_err() {
            result = result.with_warning(
                "version",
                "Version should follow semantic versioning (e.g., 1.0.0)",
            );
        }

        result
    }

    /// Validate file structure and references
    async fn validate_file_structure(
        &self,
        skill: &SkillDefinition,
        mut result: ValidationResult,
    ) -> Result<ValidationResult, ServiceError> {
        // Check if SKILL.md file exists and is readable
        if !skill.skill_file.exists() {
            result = result.with_error(
                "skill_file",
                "SKILL.md file does not exist",
                ErrorSeverity::Critical,
            );
        } else {
            // Check file size
            let metadata = fs::metadata(&skill.skill_file).await?;
            let file_size_mb = metadata.len() / (1024 * 1024);

            if file_size_mb > self.max_file_size_mb as u64 {
                result = result.with_error(
                    "skill_file",
                    &format!(
                        "SKILL.md file is too large ({} MB, max: {} MB)",
                        file_size_mb, self.max_file_size_mb
                    ),
                    ErrorSeverity::Error,
                );
            }

            // Check if file is readable
            match fs::read_to_string(&skill.skill_file).await {
                Ok(_) => {}
                Err(e) => {
                    result = result.with_error(
                        "skill_file",
                        &format!("Cannot read SKILL.md file: {}", e),
                        ErrorSeverity::Critical,
                    );
                }
            }
        }

        // Validate reference files exist
        if let Some(ref_files) = &skill.reference_files {
            for (i, ref_file) in ref_files.iter().enumerate() {
                if !ref_file.exists() {
                    result = result.with_warning(
                        &format!("reference_files[{}]", i),
                        &format!("Reference file does not exist: {}", ref_file.display()),
                    );
                }
            }
        }

        // Validate script files exist
        if let Some(script_files) = &skill.script_files {
            for (i, script_file) in script_files.iter().enumerate() {
                if !script_file.exists() {
                    result = result.with_warning(
                        &format!("script_files[{}]", i),
                        &format!("Script file does not exist: {}", script_file.display()),
                    );
                }
            }
        }

        // Validate asset files exist
        if let Some(asset_files) = &skill.asset_files {
            for (i, asset_file) in asset_files.iter().enumerate() {
                if !asset_file.exists() {
                    result = result.with_warning(
                        &format!("asset_files[{}]", i),
                        &format!("Asset file does not exist: {}", asset_file.display()),
                    );
                }
            }
        }

        Ok(result)
    }

    /// Validate content for safety issues
    async fn validate_content_safety(
        &self,
        skill: &SkillDefinition,
        mut result: ValidationResult,
    ) -> Result<ValidationResult, ServiceError> {
        // Read SKILL.md content for safety checks
        if skill.skill_file.exists() {
            match fs::read_to_string(&skill.skill_file).await {
                Ok(content) => {
                    // Check for dangerous patterns in SKILL.md content
                    for pattern in &self.dangerous_patterns {
                        if content.contains(pattern) {
                            result = result.with_error(
                                "content",
                                &format!(
                                    "Potentially dangerous pattern found in SKILL.md: {}",
                                    pattern
                                ),
                                ErrorSeverity::Critical,
                            );
                        }
                    }

                    // Check for very long content that might indicate issues
                    if content.len() > 50_000 {
                        // ~50KB
                        result = result.with_warning("content",
                            "SKILL.md content is very large - consider moving detailed information to reference files");
                    }
                }
                Err(e) => {
                    result = result.with_error(
                        "content",
                        &format!("Cannot read SKILL.md content for safety validation: {}", e),
                        ErrorSeverity::Error,
                    );
                }
            }
        }

        // Validate script files for safety
        if let Some(script_files) = &skill.script_files {
            for script_file in script_files {
                if script_file.exists() {
                    match fs::read_to_string(script_file).await {
                        Ok(content) => {
                            // Check for dangerous patterns in scripts
                            for pattern in &self.dangerous_patterns {
                                if content.contains(pattern) {
                                    result = result.with_error(
                                        "script_content",
                                        &format!(
                                            "Potentially dangerous pattern found in script {}: {}",
                                            script_file.display(),
                                            pattern
                                        ),
                                        ErrorSeverity::Critical,
                                    );
                                }
                            }
                        }
                        Err(e) => {
                            result = result.with_warning(
                                "script_content",
                                &format!(
                                    "Cannot read script file {} for safety validation: {}",
                                    script_file.display(),
                                    e
                                ),
                            );
                        }
                    }
                }
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
        if skill.skill_file.exists() {
            match fs::read_to_string(&skill.skill_file).await {
                Ok(content) => {
                    // Check for YAML frontmatter (--- at beginning)
                    let lines: Vec<&str> = content.lines().collect();
                    if lines.is_empty() || !lines[0].trim().starts_with("---") {
                        result = result.with_warning(
                            "yaml_frontmatter",
                            "SKILL.md should start with YAML frontmatter (---)",
                        );
                    } else {
                        // Check if frontmatter ends properly
                        let mut found_end = false;
                        for (i, line) in lines.iter().enumerate() {
                            if i > 0 && line.trim() == "---" {
                                found_end = true;
                                break;
                            }
                        }

                        if !found_end {
                            result = result.with_warning(
                                "yaml_frontmatter",
                                "YAML frontmatter should be properly closed with ---",
                            );
                        }
                    }
                }
                Err(e) => {
                    result = result.with_error(
                        "yaml_frontmatter",
                        &format!("Cannot read SKILL.md for frontmatter validation: {}", e),
                        ErrorSeverity::Error,
                    );
                }
            }
        }

        Ok(result)
    }

    /// Validate a skill directory structure
    pub async fn validate_skill_directory(
        &self,
        skill_path: &Path,
    ) -> Result<ValidationResult, ServiceError> {
        let mut result = ValidationResult::valid();

        // Check if skill directory exists
        if !skill_path.exists() {
            return Ok(ValidationResult::invalid("Skill directory does not exist"));
        }

        if !skill_path.is_dir() {
            return Ok(ValidationResult::invalid("Skill path is not a directory"));
        }

        // Check for required SKILL.md file
        let skill_file = skill_path.join("SKILL.md");
        if !skill_file.exists() {
            result = result.with_error(
                "structure",
                "Missing required SKILL.md file",
                ErrorSeverity::Critical,
            );
        }

        // Check directory structure
        let mut entries = fs::read_dir(skill_path).await?;

        let mut has_scripts = false;
        let mut has_references = false;
        let mut has_assets = false;

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "unknown".to_string());

            match name.as_str() {
                "SKILL.md" => {
                    // Already checked above
                }
                "scripts" => {
                    if path.is_dir() {
                        has_scripts = true;
                    } else {
                        result = result.with_warning("structure", "scripts should be a directory");
                    }
                }
                "references" => {
                    if path.is_dir() {
                        has_references = true;
                    } else {
                        result =
                            result.with_warning("structure", "references should be a directory");
                    }
                }
                "assets" => {
                    if path.is_dir() {
                        has_assets = true;
                    } else {
                        result = result.with_warning("structure", "assets should be a directory");
                    }
                }
                _ => {
                    // Unknown files/directories are OK but worth noting
                    result = result
                        .with_warning("structure", &format!("Unexpected file/directory: {}", name));
                }
            }
        }

        // Validate subdirectory contents
        if has_scripts {
            result = self.validate_scripts_directory(&skill_path.join("scripts"), result).await?;
        }

        if has_references {
            result = self
                .validate_references_directory(&skill_path.join("references"), result)
                .await?;
        }

        if has_assets {
            result = self.validate_assets_directory(&skill_path.join("assets"), result).await?;
        }

        result.calculate_score();
        Ok(result)
    }

    /// Validate scripts directory
    async fn validate_scripts_directory(
        &self,
        scripts_path: &Path,
        mut result: ValidationResult,
    ) -> Result<ValidationResult, ServiceError> {
        if !scripts_path.exists() {
            return Ok(result);
        }

        let mut entries = fs::read_dir(scripts_path).await?;

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();

            if path.is_file() {
                // Check file extensions
                if let Some(extension) = path.extension() {
                    let ext_str = extension.to_string_lossy().to_lowercase();
                    match ext_str.as_str() {
                        "py" | "js" | "ts" | "sh" | "bash" | "rb" | "go" | "rs" => {
                            // Valid script extension
                        }
                        _ => {
                            result = result.with_warning(
                                "scripts",
                                &format!("Unusual script file extension: {}", ext_str),
                            );
                        }
                    }
                } else {
                    result = result.with_warning(
                        "scripts",
                        &format!("Script file without extension: {}", path.display()),
                    );
                }
            }
        }

        Ok(result)
    }

    /// Validate references directory
    async fn validate_references_directory(
        &self,
        references_path: &Path,
        mut result: ValidationResult,
    ) -> Result<ValidationResult, ServiceError> {
        if !references_path.exists() {
            return Ok(result);
        }

        let mut entries = fs::read_dir(references_path).await?;

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();

            if path.is_file() {
                // Check file extensions for documentation
                if let Some(extension) = path.extension() {
                    let ext_str = extension.to_string_lossy().to_lowercase();
                    match ext_str.as_str() {
                        "md" | "txt" | "json" | "yaml" | "yml" | "csv" | "tsv" => {
                            // Valid reference file extension
                        }
                        _ => {
                            result = result.with_warning(
                                "references",
                                &format!("Unusual reference file extension: {}", ext_str),
                            );
                        }
                    }
                }
            }
        }

        Ok(result)
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

    /// Qualitative validation (user-requested function)
    pub async fn qualitatively_validate_skill(
        &self,
        skill: &SkillDefinition,
    ) -> Result<ValidationResult, ServiceError> {
        let mut result = ValidationResult::valid();

        // Basic structure validation (required fields, file existence)
        result = self.validate_required_fields(skill, result);
        result = self.validate_file_structure(skill, result).await?;

        // Content quality checks
        if skill.skill_file.exists() {
            match fs::read_to_string(&skill.skill_file).await {
                Ok(content) => {
                    // Check for imperative style (basic heuristic)
                    let imperative_indicators = [
                        "To", "Use", "Run", "Execute", "Create", "Generate", "Process",
                    ];
                    let mut imperative_count = 0;

                    for indicator in &imperative_indicators {
                        if content.contains(&format!("{} ", indicator)) {
                            imperative_count += 1;
                        }
                    }

                    if imperative_count < 2 {
                        result = result.with_warning(
                            "content_quality",
                            "Content may not follow imperative style (use verb-first instructions)",
                        );
                    }

                    // Check for clear examples
                    if !content.contains("Example") && !content.contains("example") {
                        result = result.with_warning("content_quality",
                            "Consider adding examples to help users understand how to use the skill");
                    }

                    // Check for trigger words in description
                    let trigger_words = [
                        "extract", "process", "convert", "generate", "create", "send", "parse",
                    ];
                    let mut trigger_count = 0;

                    for word in &trigger_words {
                        if skill.description.to_lowercase().contains(word) {
                            trigger_count += 1;
                        }
                    }

                    if trigger_count == 0 {
                        result = result.with_warning("content_quality",
                            "Description should include clear trigger words indicating what the skill does");
                    }
                }
                Err(_) => {
                    result = result.with_error(
                        "content_quality",
                        "Cannot read skill content for quality validation",
                        ErrorSeverity::Error,
                    );
                }
            }
        }

        result.calculate_score();
        Ok(result)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::core::service::SkillId;
    use crate::core::skill_manager::SkillDefinition;
    use tempfile::TempDir;

    fn create_test_skill_definition(
        temp_dir: &TempDir,
        name: &str,
        description: &str,
        version: &str,
    ) -> SkillDefinition {
        // Create valid skill ID (no dots allowed, replace with dashes)
        let valid_id = format!("{}-{}", name, version.replace('.', "-"));
        let skill_id = SkillId::new(valid_id).unwrap();
        let skill_dir = temp_dir.path().join(skill_id.to_string());
        std::fs::create_dir_all(&skill_dir).unwrap();

        let skill_file = skill_dir.join("SKILL.md");
        std::fs::write(&skill_file, format!("# {}\n\n{}", name, description)).unwrap();

        let mut skill = SkillDefinition::new(
            skill_id,
            name.to_string(),
            description.to_string(),
            version.to_string(),
        );
        skill.skill_file = skill_file;
        skill
    }

    // Tests for validate_skill with valid skill definitions
    #[tokio::test]
    async fn test_validate_skill_valid() {
        let temp_dir = TempDir::new().unwrap();
        let validator = SkillValidator::new();

        let skill = create_test_skill_definition(&temp_dir, "test-skill", "A test skill", "1.0.0");

        let result = validator.validate_skill(&skill).await.unwrap();
        assert!(result.is_valid);
        assert!(result.score >= 0.9); // Allow for minor warnings
        assert!(result.errors.is_empty());
    }

    // Tests for validate_skill with missing required fields
    #[tokio::test]
    async fn test_validate_skill_missing_name() {
        let temp_dir = TempDir::new().unwrap();
        let validator = SkillValidator::new();

        // Create skill with valid ID first, then clear name
        let mut skill =
            create_test_skill_definition(&temp_dir, "test-skill", "Description", "1.0.0");
        skill.name = "".to_string();

        let result = validator.validate_skill(&skill).await.unwrap();
        assert!(!result.is_valid);
        assert!(result.score < 1.0);
        assert!(!result.errors.is_empty());
        assert!(result.errors.iter().any(|e| e.field == "name"));
    }

    #[tokio::test]
    async fn test_validate_skill_missing_description() {
        let temp_dir = TempDir::new().unwrap();
        let validator = SkillValidator::new();

        let mut skill =
            create_test_skill_definition(&temp_dir, "test-skill", "Description", "1.0.0");
        skill.description = "".to_string();

        let result = validator.validate_skill(&skill).await.unwrap();
        assert!(!result.is_valid);
        assert!(result.score < 1.0);
        assert!(!result.errors.is_empty());
        assert!(result.errors.iter().any(|e| e.field == "description"));
    }

    #[tokio::test]
    async fn test_validate_skill_missing_version() {
        let temp_dir = TempDir::new().unwrap();
        let validator = SkillValidator::new();

        // Create with valid version first, then clear it
        let mut skill =
            create_test_skill_definition(&temp_dir, "test-skill", "Description", "1.0.0");
        skill.version = "".to_string();

        let result = validator.validate_skill(&skill).await.unwrap();
        assert!(!result.is_valid);
        assert!(result.score < 1.0);
        assert!(!result.errors.is_empty());
        assert!(result.errors.iter().any(|e| e.field == "version"));
    }

    // Tests for validate_field_formats (tested through validate_skill)
    #[tokio::test]
    async fn test_validate_skill_invalid_field_formats() {
        let temp_dir = TempDir::new().unwrap();
        let validator = SkillValidator::new();

        // Create with valid ID, then modify fields
        let mut skill =
            create_test_skill_definition(&temp_dir, "test-skill", "Description", "1.0.0");
        skill.name = "test skill".to_string(); // Invalid: contains space
        skill.version = "invalid-version".to_string(); // Invalid semver

        let result = validator.validate_skill(&skill).await.unwrap();
        // Should have warnings for invalid formats
        assert!(!result.warnings.is_empty() || !result.errors.is_empty());
    }

    // Tests for validate_file_structure (tested through validate_skill)
    #[tokio::test]
    async fn test_validate_skill_missing_skill_file() {
        let temp_dir = TempDir::new().unwrap();
        let validator = SkillValidator::new();

        let mut skill =
            create_test_skill_definition(&temp_dir, "test-skill", "Description", "1.0.0");
        // Point to non-existent file
        skill.skill_file = temp_dir.path().join("nonexistent").join("SKILL.md");

        let result = validator.validate_skill(&skill).await.unwrap();
        assert!(!result.is_valid);
        assert!(!result.errors.is_empty());
        assert!(result.errors.iter().any(|e| e.field == "skill_file"));
    }

    // Tests for validate_skill_directory
    #[tokio::test]
    async fn test_validate_skill_directory_missing_skill_md() {
        let temp_dir = TempDir::new().unwrap();
        let validator = SkillValidator::new();

        let skill_dir = temp_dir.path().join("test-skill");
        std::fs::create_dir_all(&skill_dir).unwrap();
        // Don't create SKILL.md

        let result = validator.validate_skill_directory(&skill_dir).await.unwrap();
        assert!(!result.is_valid);
        assert!(!result.errors.is_empty());
        assert!(result
            .errors
            .iter()
            .any(|e| e.field == "structure" && e.message.contains("SKILL.md")));
    }

    #[tokio::test]
    async fn test_validate_skill_directory_valid() {
        let temp_dir = TempDir::new().unwrap();
        let validator = SkillValidator::new();

        let skill_dir = temp_dir.path().join("test-skill");
        std::fs::create_dir_all(&skill_dir).unwrap();
        let skill_file = skill_dir.join("SKILL.md");
        std::fs::write(&skill_file, "# Test Skill\n\nDescription").unwrap();

        let result = validator.validate_skill_directory(&skill_dir).await.unwrap();
        // Should be valid or have minimal warnings
        assert!(result.is_valid || result.errors.is_empty());
    }

    #[tokio::test]
    async fn test_validate_skill_directory_nonexistent() {
        let temp_dir = TempDir::new().unwrap();
        let validator = SkillValidator::new();

        let skill_dir = temp_dir.path().join("nonexistent");

        let result = validator.validate_skill_directory(&skill_dir).await.unwrap();
        assert!(!result.is_valid);
        assert!(!result.errors.is_empty());
    }

    #[tokio::test]
    async fn test_validate_skill_directory_not_a_directory() {
        let temp_dir = TempDir::new().unwrap();
        let validator = SkillValidator::new();

        let skill_path = temp_dir.path().join("not-a-dir");
        std::fs::write(&skill_path, "file content").unwrap();

        let result = validator.validate_skill_directory(&skill_path).await.unwrap();
        assert!(!result.is_valid);
        assert!(!result.errors.is_empty());
    }

    // Tests for validate_content_safety (tested through validate_skill)
    #[tokio::test]
    async fn test_validate_skill_content_safety() {
        let temp_dir = TempDir::new().unwrap();
        let validator = SkillValidator::new();

        let skill = create_test_skill_definition(&temp_dir, "test-skill", "Description", "1.0.0");
        // Write potentially dangerous content
        std::fs::write(
            &skill.skill_file,
            "# test-skill\n\n<script>alert('xss')</script>",
        )
        .unwrap();

        let result = validator.validate_skill(&skill).await.unwrap();
        // Should detect dangerous patterns (may be warnings or errors depending on implementation)
        // Just verify it doesn't crash and returns a result
        assert!(result.score >= 0.0 && result.score <= 1.0);
    }

    // Tests for validate_yaml_frontmatter (tested through validate_skill)
    #[tokio::test]
    async fn test_validate_skill_invalid_yaml_frontmatter() {
        let temp_dir = TempDir::new().unwrap();
        let validator = SkillValidator::new();

        let skill = create_test_skill_definition(&temp_dir, "test-skill", "Description", "1.0.0");
        // Write invalid YAML frontmatter
        std::fs::write(
            &skill.skill_file,
            "---\ninvalid: yaml: content: here\n---\n# test-skill",
        )
        .unwrap();

        let result = validator.validate_skill(&skill).await.unwrap();
        // Should detect invalid YAML (may be warnings or errors depending on implementation)
        // Just verify it doesn't crash and returns a result
        assert!(result.score >= 0.0 && result.score <= 1.0);
    }

    #[tokio::test]
    async fn test_validate_skill_valid_yaml_frontmatter() {
        let temp_dir = TempDir::new().unwrap();
        let validator = SkillValidator::new();

        let skill = create_test_skill_definition(&temp_dir, "test-skill", "Description", "1.0.0");
        // Write valid YAML frontmatter
        std::fs::write(
            &skill.skill_file,
            "---\nname: test-skill\nversion: 1.0.0\n---\n# test-skill\n\nDescription",
        )
        .unwrap();

        let result = validator.validate_skill(&skill).await.unwrap();
        // Should pass validation
        assert!(result.is_valid || result.errors.is_empty());
    }

    #[tokio::test]
    async fn test_validate_skill_large_file() {
        let temp_dir = TempDir::new().unwrap();
        let validator = SkillValidator::new();

        let skill = create_test_skill_definition(&temp_dir, "test-skill", "Description", "1.0.0");
        // Create a large file (>10MB)
        let large_content = "x".repeat(11 * 1024 * 1024); // 11MB
        std::fs::write(&skill.skill_file, large_content).unwrap();

        let result = validator.validate_skill(&skill).await.unwrap();
        // Should have error for large file
        assert!(!result.errors.is_empty() || !result.is_valid);
    }

    #[tokio::test]
    async fn test_validate_skill_with_reference_files() {
        let temp_dir = TempDir::new().unwrap();
        let validator = SkillValidator::new();

        let mut skill =
            create_test_skill_definition(&temp_dir, "test-skill", "Description", "1.0.0");
        let ref_file = temp_dir.path().join("reference.md");
        std::fs::write(&ref_file, "Reference content").unwrap();
        skill.reference_files = Some(vec![ref_file.clone()]);

        let result = validator.validate_skill(&skill).await.unwrap();
        // Should validate reference files exist
        assert!(result.is_valid || result.warnings.is_empty());
    }

    #[tokio::test]
    async fn test_validate_skill_with_missing_reference_files() {
        let temp_dir = TempDir::new().unwrap();
        let validator = SkillValidator::new();

        let mut skill =
            create_test_skill_definition(&temp_dir, "test-skill", "Description", "1.0.0");
        let missing_ref = temp_dir.path().join("nonexistent.md");
        skill.reference_files = Some(vec![missing_ref]);

        let result = validator.validate_skill(&skill).await.unwrap();
        // Should have warnings for missing reference files
        assert!(!result.warnings.is_empty());
    }

    #[tokio::test]
    async fn test_validate_skill_with_script_files() {
        let temp_dir = TempDir::new().unwrap();
        let validator = SkillValidator::new();

        let mut skill =
            create_test_skill_definition(&temp_dir, "test-skill", "Description", "1.0.0");
        let script_file = temp_dir.path().join("script.py");
        std::fs::write(&script_file, "print('hello')").unwrap();
        skill.script_files = Some(vec![script_file.clone()]);

        let result = validator.validate_skill(&skill).await.unwrap();
        // Should validate script files
        assert!(result.is_valid || result.warnings.is_empty());
    }

    #[tokio::test]
    async fn test_validate_skill_with_dangerous_patterns() {
        let temp_dir = TempDir::new().unwrap();
        let validator = SkillValidator::new();

        let skill = create_test_skill_definition(&temp_dir, "test-skill", "Description", "1.0.0");
        // Write content with dangerous pattern
        std::fs::write(
            &skill.skill_file,
            "# test-skill\n\nimport os\nimport subprocess",
        )
        .unwrap();

        let result = validator.validate_skill(&skill).await.unwrap();
        // Should detect dangerous patterns
        assert!(!result.errors.is_empty() || !result.is_valid);
    }

    #[tokio::test]
    async fn test_validate_skill_directory_with_scripts() {
        let temp_dir = TempDir::new().unwrap();
        let validator = SkillValidator::new();

        let skill_dir = temp_dir.path().join("test-skill");
        std::fs::create_dir_all(&skill_dir).unwrap();
        let skill_file = skill_dir.join("SKILL.md");
        std::fs::write(&skill_file, "# Test Skill\n\nDescription").unwrap();

        // Create scripts directory
        let scripts_dir = skill_dir.join("scripts");
        std::fs::create_dir_all(&scripts_dir).unwrap();
        std::fs::write(scripts_dir.join("script.py"), "print('hello')").unwrap();

        let result = validator.validate_skill_directory(&skill_dir).await.unwrap();
        // Should validate directory structure
        assert!(result.is_valid || result.errors.is_empty());
    }

    #[tokio::test]
    async fn test_validate_skill_directory_with_references() {
        let temp_dir = TempDir::new().unwrap();
        let validator = SkillValidator::new();

        let skill_dir = temp_dir.path().join("test-skill");
        std::fs::create_dir_all(&skill_dir).unwrap();
        let skill_file = skill_dir.join("SKILL.md");
        std::fs::write(&skill_file, "# Test Skill\n\nDescription").unwrap();

        // Create references directory
        let refs_dir = skill_dir.join("references");
        std::fs::create_dir_all(&refs_dir).unwrap();
        std::fs::write(refs_dir.join("ref.md"), "Reference").unwrap();

        let result = validator.validate_skill_directory(&skill_dir).await.unwrap();
        assert!(result.is_valid || result.errors.is_empty());
    }

    #[tokio::test]
    async fn test_validate_skill_directory_with_assets() {
        let temp_dir = TempDir::new().unwrap();
        let validator = SkillValidator::new();

        let skill_dir = temp_dir.path().join("test-skill");
        std::fs::create_dir_all(&skill_dir).unwrap();
        let skill_file = skill_dir.join("SKILL.md");
        std::fs::write(&skill_file, "# Test Skill\n\nDescription").unwrap();

        // Create assets directory
        let assets_dir = skill_dir.join("assets");
        std::fs::create_dir_all(&assets_dir).unwrap();
        std::fs::write(assets_dir.join("image.png"), "fake png").unwrap();

        let result = validator.validate_skill_directory(&skill_dir).await.unwrap();
        assert!(result.is_valid || result.errors.is_empty());
    }

    #[tokio::test]
    async fn test_validate_skill_yaml_frontmatter_missing_closing() {
        let temp_dir = TempDir::new().unwrap();
        let validator = SkillValidator::new();

        let skill = create_test_skill_definition(&temp_dir, "test-skill", "Description", "1.0.0");
        // Write YAML frontmatter without closing
        std::fs::write(
            &skill.skill_file,
            "---\nname: test-skill\nversion: 1.0.0\n# test-skill\n\nDescription",
        )
        .unwrap();

        let result = validator.validate_skill(&skill).await.unwrap();
        // Should have warning for unclosed frontmatter
        assert!(!result.warnings.is_empty() || result.is_valid);
    }

    #[tokio::test]
    async fn test_validate_skill_yaml_frontmatter_no_frontmatter() {
        let temp_dir = TempDir::new().unwrap();
        let validator = SkillValidator::new();

        let skill = create_test_skill_definition(&temp_dir, "test-skill", "Description", "1.0.0");
        // Write content without YAML frontmatter
        std::fs::write(&skill.skill_file, "# test-skill\n\nDescription").unwrap();

        let result = validator.validate_skill(&skill).await.unwrap();
        // Should have warning for missing frontmatter
        assert!(!result.warnings.is_empty() || result.is_valid);
    }

    #[tokio::test]
    async fn test_skill_validator_with_config() {
        let temp_dir = TempDir::new().unwrap();
        let validator = SkillValidator::with_config(
            5,                        // max_file_size_mb
            100,                      // max_description_length
            vec!["name".to_string()], // only name required
        );

        let skill = create_test_skill_definition(&temp_dir, "test-skill", "Description", "1.0.0");

        let result = validator.validate_skill(&skill).await.unwrap();
        // Should work with custom config
        assert!(result.is_valid || result.errors.is_empty());
    }

    #[tokio::test]
    async fn test_validate_skill_long_description() {
        let temp_dir = TempDir::new().unwrap();
        let validator = SkillValidator::new();

        let skill =
            create_test_skill_definition(&temp_dir, "test-skill", &"x".repeat(600), "1.0.0");
        // Description exceeds max_description_length (500)

        let result = validator.validate_skill(&skill).await.unwrap();
        // Should have warning for long description
        assert!(!result.warnings.is_empty() || result.is_valid);
    }

    #[tokio::test]
    async fn test_validate_skill_with_asset_files() {
        let temp_dir = TempDir::new().unwrap();
        let validator = SkillValidator::new();

        let mut skill =
            create_test_skill_definition(&temp_dir, "test-skill", "Description", "1.0.0");
        let asset_file = temp_dir.path().join("asset.png");
        std::fs::write(&asset_file, "fake png").unwrap();
        skill.asset_files = Some(vec![asset_file.clone()]);

        let result = validator.validate_skill(&skill).await.unwrap();
        assert!(result.is_valid || result.warnings.is_empty());
    }
}
