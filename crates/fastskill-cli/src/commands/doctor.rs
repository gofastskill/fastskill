//! Doctor command - diagnose FastSkill configuration and environment

use crate::error::{CliError, CliResult};
use cli_framework::command::{FromArgValueMap, IntoCommandSpec};
use cli_framework::spec::arg_spec::{ArgKind, ArgSpec, ArgValueType, Cardinality};
use cli_framework::spec::command_tree::CommandSpec;
use cli_framework::spec::value::ArgValue;
use fastskill_core::FastSkillService;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub enum DoctorStatus {
    Pass,
    Warn,
    Fail,
}

impl std::fmt::Display for DoctorStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DoctorStatus::Pass => write!(f, "pass"),
            DoctorStatus::Warn => write!(f, "warn"),
            DoctorStatus::Fail => write!(f, "fail"),
        }
    }
}

#[derive(Debug)]
pub struct DoctorCheckResult {
    pub check: String,
    pub status: DoctorStatus,
    pub message: String,
}

/// Doctor command arguments
#[derive(Debug)]
pub struct DoctorArgs {
    /// Output as JSON
    pub json: bool,
}

impl IntoCommandSpec for DoctorArgs {
    fn command_spec() -> CommandSpec {
        CommandSpec {
            summary: "Diagnose FastSkill configuration and environment",
            syntax: Some("doctor [OPTIONS]"),
            category: Some("setup"),
            examples: vec!["fastskill doctor", "fastskill doctor --json"],
            args: vec![ArgSpec {
                name: "json",
                long: Some("json"),
                short: None,
                help: "Output results as JSON",
                kind: ArgKind::Flag,
                value_type: ArgValueType::Bool,
                cardinality: Cardinality::Optional,
                default: None,
                ..Default::default()
            }],
            ..Default::default()
        }
    }
}

impl FromArgValueMap for DoctorArgs {
    fn from_arg_value_map(map: &HashMap<String, ArgValue>) -> Self {
        Self {
            json: matches!(map.get("json"), Some(ArgValue::Bool(true))),
        }
    }
}

pub async fn execute_doctor(
    service: &FastSkillService,
    args: DoctorArgs,
    global: bool,
) -> CliResult<()> {
    let mut checks = Vec::new();

    // Check 1: Skills directory accessible
    let skills_dir = &service.config().skill_storage_path;
    let skills_dir_check = if skills_dir.exists() && skills_dir.is_dir() {
        DoctorCheckResult {
            check: "skills_dir".to_string(),
            status: DoctorStatus::Pass,
            message: format!("Skills directory found: {}", skills_dir.display()),
        }
    } else {
        DoctorCheckResult {
            check: "skills_dir".to_string(),
            status: DoctorStatus::Fail,
            message: format!(
                "Skills directory not found or not a directory: {}",
                skills_dir.display()
            ),
        }
    };
    checks.push(skills_dir_check);

    // Check 2: report the scope selected by the service edge. In global mode a
    // project Manifest is irrelevant even when the process happens to run in a
    // project checkout.
    let project_file_check = if global {
        DoctorCheckResult {
            check: "scope".to_string(),
            status: DoctorStatus::Pass,
            message: format!("Global scope selected: {}", skills_dir.display()),
        }
    } else {
        match service.project_root() {
            Some(project_root) => {
                let project_file = project_root.join("skill-project.toml");
                DoctorCheckResult {
                    check: "scope".to_string(),
                    status: DoctorStatus::Pass,
                    message: format!("Project scope selected: {}", project_file.display()),
                }
            }
            None => DoctorCheckResult {
                check: "scope".to_string(),
                status: DoctorStatus::Warn,
                message: format!(
                    "Custom skills scope selected without a project Manifest: {}",
                    skills_dir.display()
                ),
            },
        }
    };
    checks.push(project_file_check);

    if !global {
        let project_file_check = if let Some(project_root) = service.project_root() {
            DoctorCheckResult {
                check: "project_toml".to_string(),
                status: DoctorStatus::Pass,
                message: format!(
                    "skill-project.toml found: {}",
                    project_root.join("skill-project.toml").display()
                ),
            }
        } else {
            DoctorCheckResult {
                check: "project_toml".to_string(),
                status: DoctorStatus::Warn,
                message: "skill-project.toml not found. Run 'fastskill init' to create one."
                    .to_string(),
            }
        };
        checks.push(project_file_check);
    }

    // Check 3: Embedding configuration present
    let embedding_check = if let Some(embedding) = service.config().embedding.as_ref() {
        // Report the *effective* endpoint and model, not just that config
        // exists. These are environment-overridable (see `config_file`), so
        // "configuration found" alone cannot tell you whether you are talking to
        // OpenAI or to an internal gateway — which is exactly the question you
        // are asking when embeddings misbehave.
        DoctorCheckResult {
            check: "embedding_config".to_string(),
            status: DoctorStatus::Pass,
            message: format!(
                "Embedding configuration found (endpoint: {}, model: {}).",
                embedding.openai_base_url, embedding.embedding_model
            ),
        }
    } else {
        DoctorCheckResult {
            check: "embedding_config".to_string(),
            status: DoctorStatus::Warn,
            message: "No embedding configuration. Semantic search and reindex are disabled. Add [tool.fastskill.embedding] to skill-project.toml.".to_string(),
        }
    };
    checks.push(embedding_check);

    // Check 4: API key present (only if embedding configured)
    let api_key_check = if service.config().embedding.is_some() {
        if std::env::var("OPENAI_API_KEY").is_ok() {
            DoctorCheckResult {
                check: "api_key".to_string(),
                status: DoctorStatus::Pass,
                message: "OPENAI_API_KEY environment variable is set.".to_string(),
            }
        } else {
            DoctorCheckResult {
                check: "api_key".to_string(),
                status: DoctorStatus::Warn,
                message:
                    "OPENAI_API_KEY environment variable not set. Set it to enable embeddings."
                        .to_string(),
            }
        }
    } else {
        DoctorCheckResult {
            check: "api_key".to_string(),
            status: DoctorStatus::Warn,
            message: "No embedding config — API key check skipped.".to_string(),
        }
    };
    checks.push(api_key_check);

    // Check 5: only require the credential names declared by configured
    // repositories. Public/local repositories need no token.
    let required_credentials = service
        .repository_manager()
        .map(|manager| {
            manager
                .list_repositories()
                .into_iter()
                .filter_map(|repository| {
                    repository.auth.as_ref().map(|auth| match auth {
                        fastskill_core::core::repository::RepositoryAuth::Pat { env_var } => {
                            env_var.clone()
                        }
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let missing_credentials = required_credentials
        .iter()
        .filter(|name| std::env::var(name).is_err())
        .cloned()
        .collect::<Vec<_>>();
    let auth_check = if required_credentials.is_empty() {
        DoctorCheckResult {
            check: "repository_credentials".to_string(),
            status: DoctorStatus::Pass,
            message: "No configured repository requires credentials.".to_string(),
        }
    } else if missing_credentials.is_empty() {
        DoctorCheckResult {
            check: "repository_credentials".to_string(),
            status: DoctorStatus::Pass,
            message: format!(
                "All configured repository credentials are set: {}.",
                required_credentials.join(", ")
            ),
        }
    } else {
        DoctorCheckResult {
            check: "repository_credentials".to_string(),
            status: DoctorStatus::Warn,
            message: format!(
                "Missing credentials required by configured repositories: {}.",
                missing_credentials.join(", ")
            ),
        }
    };
    checks.push(auth_check);

    if args.json {
        print_json(&checks);
    } else {
        print_human(&checks);
    }

    // Exit 0 unless skills_dir is inaccessible
    let has_error = checks
        .iter()
        .any(|c| c.check == "skills_dir" && c.status == DoctorStatus::Fail);
    if has_error {
        return Err(CliError::Config(
            "Skills directory is inaccessible. Fix the skills directory to proceed.".to_string(),
        ));
    }

    Ok(())
}

fn print_human(checks: &[DoctorCheckResult]) {
    crate::outln!("FastSkill Doctor");
    crate::outln!("{}", "=".repeat(40));
    for check in checks {
        let icon = match check.status {
            DoctorStatus::Pass => "[PASS]",
            DoctorStatus::Warn => "[WARN]",
            DoctorStatus::Fail => "[FAIL]",
        };
        crate::outln!("{} {}: {}", icon, check.check, check.message);
    }
    crate::outln!();
    let errors = checks
        .iter()
        .filter(|c| c.status == DoctorStatus::Fail)
        .count();
    let warnings = checks
        .iter()
        .filter(|c| c.status == DoctorStatus::Warn)
        .count();
    if errors == 0 && warnings == 0 {
        crate::outln!("All checks passed.");
    } else {
        crate::outln!("{} error(s), {} warning(s).", errors, warnings);
    }
}

fn print_json(checks: &[DoctorCheckResult]) {
    let items: Vec<serde_json::Value> = checks
        .iter()
        .map(|c| {
            serde_json::json!({
                "check": c.check,
                "status": c.status.to_string(),
                "message": c.message,
            })
        })
        .collect();
    crate::outln!(
        "{}",
        serde_json::to_string_pretty(&items).unwrap_or_default()
    );
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use fastskill_core::core::repository::{
        RepositoryAuth, RepositoryConfig, RepositoryDefinition, RepositoryManager, RepositoryType,
    };
    use fastskill_core::{EmbeddingConfig, FastSkillService, ServiceConfig};
    use std::sync::Arc;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_execute_doctor_basic() {
        let temp_dir = TempDir::new().unwrap();
        std::fs::create_dir_all(temp_dir.path()).unwrap();
        let config = ServiceConfig {
            skill_storage_path: temp_dir.path().to_path_buf(),
            ..Default::default()
        };
        let mut service = FastSkillService::new(config).await.unwrap();
        service.initialize().await.unwrap();

        let args = DoctorArgs { json: false };
        // Should succeed (skills dir exists)
        let result = execute_doctor(&service, args, false).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_execute_doctor_json() {
        let temp_dir = TempDir::new().unwrap();
        let config = ServiceConfig {
            skill_storage_path: temp_dir.path().to_path_buf(),
            ..Default::default()
        };
        let mut service = FastSkillService::new(config).await.unwrap();
        service.initialize().await.unwrap();

        let args = DoctorArgs { json: true };
        let result = execute_doctor(&service, args, false).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_execute_doctor_missing_skills_dir() {
        let temp_dir = TempDir::new().unwrap();
        let skills_dir = temp_dir.path().join("skills");
        let config = ServiceConfig {
            skill_storage_path: skills_dir.clone(),
            ..Default::default()
        };
        let mut service = FastSkillService::new(config).await.unwrap();
        service.initialize().await.unwrap();
        std::fs::remove_dir_all(skills_dir).unwrap();

        let args = DoctorArgs { json: false };
        let result = execute_doctor(&service, args, false).await;
        assert!(
            matches!(result, Err(CliError::Config(message)) if message.contains("inaccessible"))
        );
    }

    #[tokio::test]
    async fn global_scope_and_public_repositories_do_not_require_a_project_or_token() {
        let temp_dir = TempDir::new().unwrap();
        let config = ServiceConfig {
            skill_storage_path: temp_dir.path().to_path_buf(),
            ..Default::default()
        };
        let mut service = FastSkillService::new(config)
            .await
            .unwrap()
            .with_repository_manager(Arc::new(RepositoryManager::from_definitions(vec![])));
        service.initialize().await.unwrap();

        let (result, output) =
            crate::output::capture(execute_doctor(&service, DoctorArgs { json: true }, true)).await;
        result.unwrap();
        let checks: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert!(checks.as_array().unwrap().iter().any(|check| {
            check["check"] == "scope" && check["message"].as_str().unwrap().contains("Global")
        }));
        assert!(checks.as_array().unwrap().iter().any(|check| {
            check["check"] == "repository_credentials" && check["status"] == "pass"
        }));
        assert!(!output.contains("FASTSKILL_AUTH_TOKEN"));
    }

    #[tokio::test]
    async fn project_scope_and_only_declared_repository_credentials_are_reported() {
        let temp_dir = TempDir::new().unwrap();
        let project_root = temp_dir.path().join("project");
        let skills = project_root.join("skills");
        std::fs::create_dir_all(&skills).unwrap();
        std::fs::write(project_root.join("skill-project.toml"), "[dependencies]\n").unwrap();
        let repository = RepositoryDefinition {
            name: "private".to_string(),
            repo_type: RepositoryType::HttpRegistry,
            priority: 0,
            config: RepositoryConfig::HttpRegistry {
                index_url: "https://example.invalid/index".to_string(),
            },
            auth: Some(RepositoryAuth::Pat {
                env_var: "FASTSKILL_DOCTOR_TEST_MISSING_TOKEN".to_string(),
            }),
            storage: None,
        };
        let mut service = FastSkillService::new(ServiceConfig {
            skill_storage_path: skills,
            ..Default::default()
        })
        .await
        .unwrap()
        .with_project_root(project_root.clone())
        .with_repository_manager(Arc::new(RepositoryManager::from_definitions(vec![
            repository,
        ])));
        service.initialize().await.unwrap();

        let (result, output) =
            crate::output::capture(execute_doctor(&service, DoctorArgs { json: true }, false))
                .await;
        result.unwrap();
        assert!(output.contains(project_root.join("skill-project.toml").to_str().unwrap()));
        assert!(output.contains("FASTSKILL_DOCTOR_TEST_MISSING_TOKEN"));
        assert!(!output.contains("FASTSKILL_AUTH_TOKEN"));
    }

    #[tokio::test]
    async fn declared_repository_credentials_pass_when_the_named_variable_exists() {
        let temp_dir = TempDir::new().unwrap();
        let repository = RepositoryDefinition {
            name: "private".to_string(),
            repo_type: RepositoryType::HttpRegistry,
            priority: 0,
            config: RepositoryConfig::HttpRegistry {
                index_url: "https://example.invalid/index".to_string(),
            },
            auth: Some(RepositoryAuth::Pat {
                env_var: "PATH".to_string(),
            }),
            storage: None,
        };
        let mut service = FastSkillService::new(ServiceConfig {
            skill_storage_path: temp_dir.path().to_path_buf(),
            ..Default::default()
        })
        .await
        .unwrap()
        .with_repository_manager(Arc::new(RepositoryManager::from_definitions(vec![
            repository,
        ])));
        service.initialize().await.unwrap();

        let (result, output) =
            crate::output::capture(execute_doctor(&service, DoctorArgs { json: true }, true)).await;
        result.unwrap();
        assert!(output.contains("All configured repository credentials are set: PATH"));
    }

    #[test]
    fn argument_map_and_status_display_cover_typed_boundaries() {
        let mut map = HashMap::new();
        map.insert("json".to_string(), ArgValue::Bool(true));
        assert!(DoctorArgs::from_arg_value_map(&map).json);
        assert_eq!(DoctorStatus::Pass.to_string(), "pass");
        assert_eq!(DoctorStatus::Warn.to_string(), "warn");
        assert_eq!(DoctorStatus::Fail.to_string(), "fail");
        let spec = DoctorArgs::command_spec();
        assert_eq!(spec.syntax, Some("doctor [OPTIONS]"));
        assert_eq!(spec.args[0].long, Some("json"));
    }

    #[tokio::test]
    async fn embedding_check_reports_the_effective_endpoint_and_model() {
        let temp_dir = TempDir::new().unwrap();
        let mut service = FastSkillService::new(ServiceConfig {
            skill_storage_path: temp_dir.path().to_path_buf(),
            embedding: Some(EmbeddingConfig {
                openai_base_url: "https://gateway.example.invalid".to_string(),
                embedding_model: "company-model".to_string(),
                index_path: Some(temp_dir.path().join("index.db")),
            }),
            ..Default::default()
        })
        .await
        .unwrap();
        service.initialize().await.unwrap();

        let (result, output) =
            crate::output::capture(execute_doctor(&service, DoctorArgs { json: true }, true)).await;
        result.unwrap();
        assert!(output.contains("https://gateway.example.invalid"));
        assert!(output.contains("company-model"));
    }
}
