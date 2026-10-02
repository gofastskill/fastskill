//! The user's own repositories file, `repositories.toml` in FastSkill's
//! config directory (ADR-0018).
//!
//! It holds repositories that belong to the person, not the project: a
//! repository with `command` auth can be configured only here. When a
//! repository of the same name is also in the project file, the user's
//! entry wins.

use super::{RepositoriesConfig, RepositoryDefinition};
use crate::core::service::ServiceError;
use std::path::{Path, PathBuf};

/// The file name of the user's repositories file.
pub const USER_REPOSITORIES_FILE: &str = "repositories.toml";

/// FastSkill's config directory: `$XDG_CONFIG_HOME/fastskill` when that is
/// absolute, else the platform config directory joined with `fastskill`.
/// The same directory that holds the global lock file.
pub fn user_config_dir() -> Option<PathBuf> {
    crate::core::lock::global_lock_path()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
}

/// Where the user's repositories file lives.
pub fn user_repositories_path() -> Option<PathBuf> {
    user_config_dir().map(|dir| dir.join(USER_REPOSITORIES_FILE))
}

/// The user's repositories, or none when the file does not exist. Each one
/// is validated; the file is never created here.
pub fn load_user_repositories() -> Result<Vec<RepositoryDefinition>, ServiceError> {
    match user_repositories_path() {
        Some(path) => load_repositories_file(&path),
        None => Ok(Vec::new()),
    }
}

/// Read and validate a `repositories.toml` file; a missing file is empty.
pub fn load_repositories_file(path: &Path) -> Result<Vec<RepositoryDefinition>, ServiceError> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let content = std::fs::read_to_string(path)?;
    let config: RepositoriesConfig = toml::from_str(&content)
        .map_err(|e| ServiceError::Config(format!("Failed to parse {}: {e}", path.display())))?;
    for repository in &config.repositories {
        repository
            .validate()
            .map_err(|e| ServiceError::Config(format!("{}: {e}", path.display())))?;
    }
    Ok(config.repositories)
}

/// The project's repositories plus the user's, where a user entry replaces a
/// project entry of the same name.
pub fn merge_repositories(
    project: Vec<RepositoryDefinition>,
    user: Vec<RepositoryDefinition>,
) -> Vec<RepositoryDefinition> {
    let mut merged: Vec<RepositoryDefinition> = project
        .into_iter()
        .filter(|repository| !user.iter().any(|mine| mine.name == repository.name))
        .collect();
    merged.extend(user);
    merged
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::core::repository::{RepositoryAuth, RepositoryConfig, RepositoryType};

    fn registry(name: &str, auth: Option<RepositoryAuth>) -> RepositoryDefinition {
        RepositoryDefinition {
            name: name.to_string(),
            repo_type: RepositoryType::HttpRegistry,
            priority: 0,
            config: RepositoryConfig::HttpRegistry {
                index_url: "https://registry.example/index".to_string(),
            },
            auth,
            storage: None,
        }
    }

    #[test]
    fn user_entry_replaces_project_entry_of_the_same_name() {
        let command = RepositoryAuth::Command {
            command: vec!["helper".to_string()],
        };
        let merged = merge_repositories(
            vec![registry("shared", None), registry("project-only", None)],
            vec![registry("shared", Some(command.clone()))],
        );
        assert_eq!(merged.len(), 2);
        let shared = merged.iter().find(|r| r.name == "shared").unwrap();
        assert_eq!(shared.auth, Some(command));
    }

    #[test]
    fn missing_file_is_empty_and_command_auth_loads() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(USER_REPOSITORIES_FILE);
        assert!(load_repositories_file(&path).unwrap().is_empty());
        assert!(!path.exists());
        std::fs::write(
            &path,
            "[[repositories]]\nname = \"private\"\ntype = \"http-registry\"\n\
             index_url = \"https://registry.example/index\"\n\
             auth = { type = \"command\", command = [\"helper\", \"token\"] }\n",
        )
        .unwrap();
        let loaded = load_repositories_file(&path).unwrap();
        assert_eq!(loaded[0].auth.as_ref().unwrap().kind(), "command");
    }

    #[test]
    fn invalid_entries_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(USER_REPOSITORIES_FILE);
        std::fs::write(
            &path,
            "[[repositories]]\nname = \"private\"\ntype = \"http-registry\"\n\
             index_url = \"https://registry.example/index\"\n\
             auth = { type = \"command\", command = [] }\n",
        )
        .unwrap();
        assert!(load_repositories_file(&path).is_err());
        std::fs::write(&path, "not toml [").unwrap();
        assert!(load_repositories_file(&path).is_err());
    }
}
