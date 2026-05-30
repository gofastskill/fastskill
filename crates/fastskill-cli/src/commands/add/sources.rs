//! Source type resolution for the add command.

use crate::error::{CliError, CliResult};
use crate::utils::{detect_skill_source, SkillSource};
use std::path::{Path, PathBuf};

/// Resolved source type for the add operation
#[allow(dead_code)]
pub enum ResolvedSource {
    /// Local directory path
    Local(PathBuf),
    /// Git URL
    Git(String),
    /// ZIP file path
    Zip(PathBuf),
    /// Registry skill ID
    Registry(String),
}

/// Resolve source type from an explicit flag value or autodetect from the path/URL.
///
/// Returns `Err(CliError::Config)` for an unrecognized `--source-type` value,
/// preventing silent fallthrough to autodetection.
#[allow(dead_code)]
pub fn resolve_source_type(source: &str, source_type: Option<&str>) -> CliResult<ResolvedSource> {
    let Some(stype) = source_type else {
        // Autodetect
        return Ok(autodetect_source(source));
    };

    match stype {
        "registry" => Ok(ResolvedSource::Registry(source.to_string())),
        "github" | "git" => Ok(ResolvedSource::Git(source.to_string())),
        "local" => {
            let path = PathBuf::from(source);
            if path.extension().and_then(|s| s.to_str()) == Some("zip") {
                Ok(ResolvedSource::Zip(path))
            } else {
                Ok(ResolvedSource::Local(path))
            }
        }
        other => Err(CliError::Config(format!(
            "Unrecognized --source-type value: '{}'. Expected one of: registry, git, github, local",
            other
        ))),
    }
}

/// Autodetect source type from path/URL (no error on unknown type — returns Registry as default).
#[allow(dead_code)]
pub fn autodetect_source(source: &str) -> ResolvedSource {
    match detect_skill_source(source) {
        SkillSource::GitUrl(url) => ResolvedSource::Git(url),
        SkillSource::ZipFile(path) => ResolvedSource::Zip(path),
        SkillSource::Folder(path) => ResolvedSource::Local(path),
        SkillSource::SkillId(id) => ResolvedSource::Registry(id),
    }
}

/// Check if a path is a local path that could be a skill directory
#[allow(dead_code)]
pub fn is_local_path(source: &str) -> bool {
    let path = Path::new(source);
    path.exists() || source.starts_with('.') || source.starts_with('/')
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_invalid_source_type_returns_error() {
        let result = resolve_source_type("/some/path", Some("invalid-type"));
        assert!(
            result.is_err(),
            "invalid source type must return CliError::Config"
        );
        if let Err(CliError::Config(msg)) = result {
            assert!(
                msg.contains("invalid-type"),
                "error message must mention the invalid value"
            );
        } else {
            panic!("Expected CliError::Config");
        }
    }

    #[test]
    fn test_resolve_registry_source_type() {
        let result = resolve_source_type("my-skill@1.0.0", Some("registry")).unwrap();
        assert!(matches!(result, ResolvedSource::Registry(_)));
    }

    #[test]
    fn test_resolve_git_source_type() {
        let result = resolve_source_type("https://github.com/org/skill.git", Some("git")).unwrap();
        assert!(matches!(result, ResolvedSource::Git(_)));
    }

    #[test]
    fn test_resolve_local_source_type_directory() {
        let result = resolve_source_type("/some/local/path", Some("local")).unwrap();
        assert!(matches!(result, ResolvedSource::Local(_)));
    }

    #[test]
    fn test_resolve_local_source_type_zip() {
        let result = resolve_source_type("/some/skill.zip", Some("local")).unwrap();
        assert!(matches!(result, ResolvedSource::Zip(_)));
    }

    #[test]
    fn test_resolve_no_source_type_autodetects() {
        // With no source_type, should autodetect (no error)
        let result = resolve_source_type("my-skill@1.0.0", None).unwrap();
        assert!(matches!(result, ResolvedSource::Registry(_)));
    }
}
