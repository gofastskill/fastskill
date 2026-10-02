//! How a repository authenticates (ADR-0018).

use serde::{Deserialize, Serialize};

/// Unified authentication configuration for a repository.
///
/// `Pat` and `Bearer` name an environment variable that holds the token.
/// `Command` names a program FastSkill runs to obtain one. A project file
/// (`skill-project.toml`) can never carry `Command`: it is accepted only from
/// the user's own `repositories.toml` (ADR-0018, mirroring ADR-0016
/// decision 7).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type")]
pub enum RepositoryAuth {
    /// Legacy token, sent as `Authorization: token <value>`.
    #[serde(rename = "pat")]
    Pat { env_var: String },
    /// Token from an environment variable, sent as `Authorization: Bearer <value>`.
    #[serde(rename = "bearer")]
    Bearer { env_var: String },
    /// Token printed by a program, sent as `Authorization: Bearer <value>`.
    #[serde(rename = "command")]
    Command { command: Vec<String> },
}

impl RepositoryAuth {
    /// The short name shown by `repo list` and `repo show`.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Pat { .. } => "pat",
            Self::Bearer { .. } => "bearer",
            Self::Command { .. } => "command",
        }
    }

    /// The environment variable this auth reads, if it reads one.
    pub fn env_var(&self) -> Option<&str> {
        match self {
            Self::Pat { env_var } | Self::Bearer { env_var } => Some(env_var),
            Self::Command { .. } => None,
        }
    }

    /// The environment variable of a legacy `pat` auth. Sources that only
    /// understand `pat` use this; other types never reach them because
    /// [`super::RepositoryDefinition::validate`] refuses auth on those types.
    pub fn pat_env_var(&self) -> Option<&str> {
        match self {
            Self::Pat { env_var } => Some(env_var),
            _ => None,
        }
    }

    /// A one-line description that never contains a token.
    pub fn describe(&self) -> String {
        match self {
            Self::Pat { env_var } | Self::Bearer { env_var } => {
                format!("{} (env {env_var})", self.kind())
            }
            Self::Command { command } => format!(
                "command ({})",
                command.first().map(String::as_str).unwrap_or_default()
            ),
        }
    }

    /// Fail on an auth block that cannot work.
    pub(crate) fn validate(&self, repository: &str) -> Result<(), String> {
        match self {
            Self::Pat { env_var } | Self::Bearer { env_var } if env_var.trim().is_empty() => {
                Err(format!(
                    "Repository '{repository}' has {} auth with an empty `env_var`; name \
                     the environment variable that holds the token.",
                    self.kind()
                ))
            }
            Self::Command { command } if command.first().is_none_or(|p| p.trim().is_empty()) => {
                Err(format!(
                    "Repository '{repository}' has command auth with no program; set \
                     `command = [\"program\", \"arg\", ...]`."
                ))
            }
            _ => Ok(()),
        }
    }
}

/// Text for a project file that names a credential command.
pub fn project_command_refused(repository: &str) -> String {
    format!(
        "Repository '{repository}' sets auth type \"command\" in a project file. A project \
         file cannot name a command for FastSkill to run, because anyone who can change \
         the file could run code on your machine. Configure this repository in your user \
         repositories.toml instead (`fastskill repo add --user ... --auth-type command`)."
    )
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn describe_never_shows_more_than_kind_env_and_program() {
        let bearer = RepositoryAuth::Bearer {
            env_var: "REG_TOKEN".to_string(),
        };
        assert_eq!(bearer.describe(), "bearer (env REG_TOKEN)");
        assert_eq!(bearer.env_var(), Some("REG_TOKEN"));
        assert_eq!(bearer.pat_env_var(), None);
        let command = RepositoryAuth::Command {
            command: vec!["helper".to_string(), "--secret-flag".to_string()],
        };
        assert_eq!(command.describe(), "command (helper)");
        assert_eq!(command.env_var(), None);
        assert_eq!(command.kind(), "command");
    }

    #[test]
    fn validate_refuses_empty_env_var_and_empty_command() {
        let empty_env = RepositoryAuth::Bearer {
            env_var: " ".to_string(),
        };
        assert!(empty_env.validate("r").unwrap_err().contains("env_var"));
        let empty = RepositoryAuth::Command { command: vec![] };
        assert!(empty.validate("r").unwrap_err().contains("no program"));
        let blank = RepositoryAuth::Command {
            command: vec![String::new()],
        };
        assert!(blank.validate("r").is_err());
        let ok = RepositoryAuth::Command {
            command: vec!["helper".to_string()],
        };
        ok.validate("r").unwrap();
    }

    #[test]
    fn toml_shapes_round_trip() {
        #[derive(Deserialize)]
        struct Wrap {
            auth: RepositoryAuth,
        }
        let bearer: Wrap = toml::from_str("auth = { type = \"bearer\", env_var = \"T\" }").unwrap();
        assert_eq!(bearer.auth.kind(), "bearer");
        let command: Wrap =
            toml::from_str("auth = { type = \"command\", command = [\"helper\", \"token\"] }")
                .unwrap();
        assert_eq!(
            command.auth,
            RepositoryAuth::Command {
                command: vec!["helper".to_string(), "token".to_string()]
            }
        );
        assert!(project_command_refused("r").contains("cannot name a command"));
    }
}
