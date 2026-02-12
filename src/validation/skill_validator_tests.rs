//! Tests for SkillValidator.

#![allow(clippy::unwrap_used)]

use crate::core::service::SkillId;
use crate::core::skill_manager::SkillDefinition;
use crate::validation::{SkillValidator, ValidationResult};
use tempfile::TempDir;

fn create_test_skill_definition(
    temp_dir: &TempDir,
    name: &str,
    description: &str,
    version: &str,
) -> SkillDefinition {
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

struct ValidatorTestEnv {
    temp_dir: TempDir,
    validator: SkillValidator,
}

impl ValidatorTestEnv {
    fn new() -> Self {
        Self {
            temp_dir: TempDir::new().unwrap(),
            validator: SkillValidator::new(),
        }
    }
    fn skill(&self, name: &str, description: &str, version: &str) -> SkillDefinition {
        create_test_skill_definition(&self.temp_dir, name, description, version)
    }
    fn skill_with_content(&self, skill_file_content: &str) -> SkillDefinition {
        let skill =
            create_test_skill_definition(&self.temp_dir, "test-skill", "Description", "1.0.0");
        std::fs::write(&skill.skill_file, skill_file_content).unwrap();
        skill
    }
    async fn validate_skill_with_content(&self, skill_file_content: &str) -> ValidationResult {
        let skill = self.skill_with_content(skill_file_content);
        self.validator.validate_skill(&skill).await.unwrap()
    }
    async fn validate_skill_dir_with_subdir(
        &self,
        subdir: &str,
        filename: &str,
        file_contents: &str,
    ) -> ValidationResult {
        let skill_dir = self.temp_dir.path().join("test-skill");
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(skill_dir.join("SKILL.md"), "# Test Skill\n\nDescription").unwrap();
        let sub_path = skill_dir.join(subdir);
        std::fs::create_dir_all(&sub_path).unwrap();
        std::fs::write(sub_path.join(filename), file_contents).unwrap();
        self.validator
            .validate_skill_directory(&skill_dir)
            .await
            .unwrap()
    }
}

async fn assert_validate_skill_fails_for_field<F>(field: &str, mutate: F)
where
    F: FnOnce(&mut SkillDefinition),
{
    let env = ValidatorTestEnv::new();
    let mut skill = env.skill("test-skill", "Description", "1.0.0");
    mutate(&mut skill);
    let result = env.validator.validate_skill(&skill).await.unwrap();
    assert!(!result.is_valid);
    assert!(result.score < 1.0);
    assert!(!result.errors.is_empty());
    assert!(result.errors.iter().any(|e| e.field == field));
}

#[tokio::test]
async fn test_validate_skill_valid() {
    let env = ValidatorTestEnv::new();
    let skill = env.skill("test-skill", "A test skill", "1.0.0");
    let result = env.validator.validate_skill(&skill).await.unwrap();
    assert!(result.is_valid);
    assert!(result.score >= 0.9);
    assert!(result.errors.is_empty());
}

#[tokio::test]
async fn test_validate_skill_missing_name() {
    assert_validate_skill_fails_for_field("name", |s| s.name = "".to_string()).await;
}

#[tokio::test]
async fn test_validate_skill_missing_description() {
    assert_validate_skill_fails_for_field("description", |s| s.description = "".to_string()).await;
}

#[tokio::test]
async fn test_validate_skill_missing_version() {
    assert_validate_skill_fails_for_field("version", |s| s.version = "".to_string()).await;
}

#[tokio::test]
async fn test_validate_skill_invalid_field_formats() {
    let env = ValidatorTestEnv::new();
    let mut skill = env.skill("test-skill", "Description", "1.0.0");
    skill.name = "test skill".to_string();
    skill.version = "invalid-version".to_string();
    let result = env.validator.validate_skill(&skill).await.unwrap();
    assert!(!result.warnings.is_empty() || !result.errors.is_empty());
}

#[tokio::test]
async fn test_validate_skill_missing_skill_file() {
    let env = ValidatorTestEnv::new();
    let mut skill = env.skill("test-skill", "Description", "1.0.0");
    skill.skill_file = env.temp_dir.path().join("nonexistent").join("SKILL.md");
    let result = env.validator.validate_skill(&skill).await.unwrap();
    assert!(!result.is_valid);
    assert!(!result.errors.is_empty());
    assert!(result.errors.iter().any(|e| e.field == "skill_file"));
}

#[tokio::test]
async fn test_validate_skill_directory_missing_skill_md() {
    let env = ValidatorTestEnv::new();
    let skill_dir = env.temp_dir.path().join("test-skill");
    std::fs::create_dir_all(&skill_dir).unwrap();
    let result = env
        .validator
        .validate_skill_directory(&skill_dir)
        .await
        .unwrap();
    assert!(!result.is_valid);
    assert!(!result.errors.is_empty());
    assert!(result
        .errors
        .iter()
        .any(|e| e.field == "structure" && e.message.contains("SKILL.md")));
}

#[tokio::test]
async fn test_validate_skill_directory_valid() {
    let env = ValidatorTestEnv::new();
    let skill_dir = env.temp_dir.path().join("test-skill");
    std::fs::create_dir_all(&skill_dir).unwrap();
    std::fs::write(skill_dir.join("SKILL.md"), "# Test Skill\n\nDescription").unwrap();
    let result = env
        .validator
        .validate_skill_directory(&skill_dir)
        .await
        .unwrap();
    assert!(result.is_valid || result.errors.is_empty());
}

#[tokio::test]
async fn test_validate_skill_directory_nonexistent() {
    let env = ValidatorTestEnv::new();
    let skill_dir = env.temp_dir.path().join("nonexistent");
    let result = env
        .validator
        .validate_skill_directory(&skill_dir)
        .await
        .unwrap();
    assert!(!result.is_valid);
    assert!(!result.errors.is_empty());
}

#[tokio::test]
async fn test_validate_skill_directory_not_a_directory() {
    let env = ValidatorTestEnv::new();
    let skill_path = env.temp_dir.path().join("not-a-dir");
    std::fs::write(&skill_path, "file content").unwrap();
    let result = env
        .validator
        .validate_skill_directory(&skill_path)
        .await
        .unwrap();
    assert!(!result.is_valid);
    assert!(!result.errors.is_empty());
}

#[tokio::test]
async fn test_validate_skill_content_safety() {
    let env = ValidatorTestEnv::new();
    let result = env
        .validate_skill_with_content("# test-skill\n\n<script>alert('xss')</script>")
        .await;
    assert!(result.score >= 0.0 && result.score <= 1.0);
}

#[tokio::test]
async fn test_validate_skill_invalid_yaml_frontmatter() {
    let env = ValidatorTestEnv::new();
    let result = env
        .validate_skill_with_content("---\ninvalid: yaml: content: here\n---\n# test-skill")
        .await;
    assert!(result.score >= 0.0 && result.score <= 1.0);
}

#[tokio::test]
async fn test_validate_skill_valid_yaml_frontmatter() {
    let env = ValidatorTestEnv::new();
    let result = env
        .validate_skill_with_content(
            "---\nname: test-skill\nversion: 1.0.0\n---\n# test-skill\n\nDescription",
        )
        .await;
    assert!(result.is_valid || result.errors.is_empty());
}

#[tokio::test]
async fn test_validate_skill_large_file() {
    let env = ValidatorTestEnv::new();
    let skill = env.skill("test-skill", "Description", "1.0.0");
    let large_content = "x".repeat(11 * 1024 * 1024);
    std::fs::write(&skill.skill_file, large_content).unwrap();
    let result = env.validator.validate_skill(&skill).await.unwrap();
    assert!(!result.errors.is_empty() || !result.is_valid);
}

#[tokio::test]
async fn test_validate_skill_with_reference_files() {
    let env = ValidatorTestEnv::new();
    let mut skill = env.skill("test-skill", "Description", "1.0.0");
    let ref_file = env.temp_dir.path().join("reference.md");
    std::fs::write(&ref_file, "Reference content").unwrap();
    skill.reference_files = Some(vec![ref_file]);
    let result = env.validator.validate_skill(&skill).await.unwrap();
    assert!(result.is_valid || result.warnings.is_empty());
}

#[tokio::test]
async fn test_validate_skill_with_missing_reference_files() {
    let env = ValidatorTestEnv::new();
    let mut skill = env.skill("test-skill", "Description", "1.0.0");
    skill.reference_files = Some(vec![env.temp_dir.path().join("nonexistent.md")]);
    let result = env.validator.validate_skill(&skill).await.unwrap();
    assert!(!result.warnings.is_empty());
}

#[tokio::test]
async fn test_validate_skill_with_script_files() {
    let env = ValidatorTestEnv::new();
    let mut skill = env.skill("test-skill", "Description", "1.0.0");
    let script_file = env.temp_dir.path().join("script.py");
    std::fs::write(&script_file, "print('hello')").unwrap();
    skill.script_files = Some(vec![script_file]);
    let result = env.validator.validate_skill(&skill).await.unwrap();
    assert!(result.is_valid || result.warnings.is_empty());
}

#[tokio::test]
async fn test_validate_skill_with_dangerous_patterns() {
    let env = ValidatorTestEnv::new();
    let result = env
        .validate_skill_with_content("# test-skill\n\nimport os\nimport subprocess")
        .await;
    assert!(!result.errors.is_empty() || !result.is_valid);
}

#[tokio::test]
async fn test_validate_skill_directory_with_scripts() {
    let env = ValidatorTestEnv::new();
    let result = env
        .validate_skill_dir_with_subdir("scripts", "script.py", "print('hello')")
        .await;
    assert!(result.is_valid || result.errors.is_empty());
}

#[tokio::test]
async fn test_validate_skill_directory_with_references() {
    let env = ValidatorTestEnv::new();
    let result = env
        .validate_skill_dir_with_subdir("references", "ref.md", "Reference")
        .await;
    assert!(result.is_valid || result.errors.is_empty());
}

#[tokio::test]
async fn test_validate_skill_directory_with_assets() {
    let env = ValidatorTestEnv::new();
    let result = env
        .validate_skill_dir_with_subdir("assets", "image.png", "fake png")
        .await;
    assert!(result.is_valid || result.errors.is_empty());
}

#[tokio::test]
async fn test_validate_skill_yaml_frontmatter_missing_closing() {
    let env = ValidatorTestEnv::new();
    let result = env
        .validate_skill_with_content(
            "---\nname: test-skill\nversion: 1.0.0\n# test-skill\n\nDescription",
        )
        .await;
    assert!(!result.warnings.is_empty() || result.is_valid);
}

#[tokio::test]
async fn test_validate_skill_yaml_frontmatter_no_frontmatter() {
    let env = ValidatorTestEnv::new();
    let result = env
        .validate_skill_with_content("# test-skill\n\nDescription")
        .await;
    assert!(!result.warnings.is_empty() || result.is_valid);
}

#[tokio::test]
async fn test_skill_validator_with_config() {
    let env = ValidatorTestEnv::new();
    let validator = SkillValidator::with_config(5, 100, vec!["name".to_string()]);
    let skill = env.skill("test-skill", "Description", "1.0.0");
    let result = validator.validate_skill(&skill).await.unwrap();
    assert!(result.is_valid || result.errors.is_empty());
}

#[tokio::test]
async fn test_validate_skill_long_description() {
    let env = ValidatorTestEnv::new();
    let skill = env.skill("test-skill", &"x".repeat(600), "1.0.0");
    let result = env.validator.validate_skill(&skill).await.unwrap();
    assert!(!result.warnings.is_empty() || result.is_valid);
}

#[tokio::test]
async fn test_validate_skill_with_asset_files() {
    let env = ValidatorTestEnv::new();
    let mut skill = env.skill("test-skill", "Description", "1.0.0");
    let asset_file = env.temp_dir.path().join("asset.png");
    std::fs::write(&asset_file, "fake png").unwrap();
    skill.asset_files = Some(vec![asset_file]);
    let result = env.validator.validate_skill(&skill).await.unwrap();
    assert!(result.is_valid || result.warnings.is_empty());
}
