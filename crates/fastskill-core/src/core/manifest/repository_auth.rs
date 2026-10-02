//! Refuse repository auth a project file must not carry (ADR-0018).
//!
//! A project file is shared: anyone who can change it would otherwise choose
//! what runs on every machine that reads it. So `command` auth, which names a
//! program to run, is refused here before the typed parse, in the same way
//! ADR-0016 decision 7 keeps a project file from naming a command. A `bearer`
//! auth must name its environment variable.

use super::ManifestError;
use crate::core::repository::auth::project_command_refused;

/// Check every `[[tool.fastskill.repositories]]` entry's `auth` table.
///
/// Content that is not valid TOML is left to the typed parse, which reports it
/// with a better message.
pub(super) fn reject_unusable_repository_auth(content: &str) -> Result<(), ManifestError> {
    let Ok(raw) = toml::from_str::<toml::Value>(content) else {
        return Ok(());
    };
    let repositories = raw
        .get("tool")
        .and_then(|tool| tool.get("fastskill"))
        .and_then(|fastskill| fastskill.get("repositories"))
        .and_then(toml::Value::as_array);
    for repository in repositories.into_iter().flatten() {
        let Some(auth) = repository.get("auth") else {
            continue;
        };
        let name = repository
            .get("name")
            .and_then(toml::Value::as_str)
            .unwrap_or("<unnamed>");
        let kind = auth.get("type").and_then(toml::Value::as_str);
        let has_command = auth.get("command").is_some();
        if kind == Some("command") || has_command {
            return Err(ManifestError::Parse(project_command_refused(name)));
        }
        let env_var = auth.get("env_var").and_then(toml::Value::as_str);
        if kind == Some("bearer") && env_var.is_none_or(|v| v.trim().is_empty()) {
            return Err(ManifestError::Parse(format!(
                "Repository '{name}' has bearer auth without `env_var`; name the \
                 environment variable that holds the token, e.g. \
                 auth = {{ type = \"bearer\", env_var = \"REGISTRY_TOKEN\" }}."
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::super::SkillProjectToml;
    use super::*;

    fn manifest(auth: &str) -> String {
        format!(
            "[dependencies]\n\n[[tool.fastskill.repositories]]\nname = \"private\"\n\
             type = \"http-registry\"\npriority = 0\nindex_url = \"https://registry.example/index\"\n\
             auth = {auth}\n"
        )
    }

    #[test]
    fn project_file_with_command_auth_is_refused() {
        let content = manifest("{ type = \"command\", command = [\"helper\", \"token\"] }");
        let error = SkillProjectToml::from_toml_str(&content)
            .unwrap_err()
            .to_string();
        assert!(error.contains("cannot name a command"), "{error}");
        assert!(error.contains("'private'"), "{error}");
        // A `command` key under another type is refused too.
        let sneaky = manifest("{ type = \"bearer\", env_var = \"T\", command = [\"helper\"] }");
        assert!(SkillProjectToml::from_toml_str(&sneaky).is_err());
    }

    #[test]
    fn project_file_with_bearer_auth_parses_and_converts() {
        let content = manifest("{ type = \"bearer\", env_var = \"REGISTRY_TOKEN\" }");
        let project = SkillProjectToml::from_toml_str(&content).unwrap();
        let repositories = project
            .tool
            .unwrap()
            .fastskill
            .unwrap()
            .repositories
            .unwrap();
        let runtime = crate::core::repository::RepositoryDefinition::from(&repositories[0]);
        assert_eq!(
            runtime.auth,
            Some(crate::core::repository::RepositoryAuth::Bearer {
                env_var: "REGISTRY_TOKEN".to_string()
            })
        );
    }

    #[test]
    fn project_file_bearer_without_env_var_is_refused() {
        let content = manifest("{ type = \"bearer\" }");
        let error = SkillProjectToml::from_toml_str(&content)
            .unwrap_err()
            .to_string();
        assert!(error.contains("without `env_var`"), "{error}");
    }

    #[test]
    fn invalid_toml_is_left_to_the_typed_parse() {
        reject_unusable_repository_auth("not = [valid").unwrap();
        reject_unusable_repository_auth("[dependencies]\n").unwrap();
    }
}
