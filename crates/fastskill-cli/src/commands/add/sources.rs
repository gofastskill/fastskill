//! Parsing helpers for explicit registry references.

use crate::error::{CliError, CliResult};

pub(super) fn parse_registry_scope_id(
    skill_id_input: &str,
) -> CliResult<(String, String, String, Option<String>)> {
    use crate::utils::parse_skill_id;
    use fastskill_core::security::path::validate_path_component;

    let (skill_id_full, version) = parse_skill_id(skill_id_input);
    // Only http-registry skills carry a scope (the publisher namespace);
    // git-marketplace and local repositories list bare ids.
    let (scope, expected_id) = match skill_id_full.split_once('/') {
        Some((scope, id)) => {
            validate_path_component(scope).map_err(|error| {
                CliError::Config(format!("Invalid registry scope '{scope}': {error}"))
            })?;
            (scope, id)
        }
        None => ("", skill_id_full.as_str()),
    };
    validate_path_component(expected_id).map_err(|error| {
        CliError::Config(format!(
            "Invalid registry skill id '{expected_id}': {error}"
        ))
    })?;
    Ok((
        skill_id_full.clone(),
        scope.to_string(),
        expected_id.to_string(),
        version,
    ))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn registry_reference_accepts_a_bare_id() {
        let (full, scope, id, version) = parse_registry_scope_id("cli-rust-dev@1.0.0").unwrap();
        assert_eq!(full, "cli-rust-dev");
        assert_eq!(scope, "");
        assert_eq!(id, "cli-rust-dev");
        assert_eq!(version.as_deref(), Some("1.0.0"));
    }

    #[test]
    fn registry_reference_rejects_empty_components() {
        assert!(parse_registry_scope_id("/id").is_err());
        assert!(parse_registry_scope_id("scope/").is_err());
    }

    #[test]
    fn registry_reference_rejects_unsafe_components() {
        assert!(parse_registry_scope_id("../evil").is_err());
        assert!(parse_registry_scope_id("bad\\scope/id").is_err());
        assert!(parse_registry_scope_id("safe/../evil").is_err());
        assert!(parse_registry_scope_id("a/b/c").is_err());
        assert!(parse_registry_scope_id("..").is_err());
    }

    #[test]
    fn registry_reference_preserves_constraint() {
        let (full, scope, id, version) =
            parse_registry_scope_id("my-scope/my-skill@1.2.3").unwrap();
        assert_eq!(full, "my-scope/my-skill");
        assert_eq!(scope, "my-scope");
        assert_eq!(id, "my-skill");
        assert_eq!(version.as_deref(), Some("1.2.3"));
    }
}
