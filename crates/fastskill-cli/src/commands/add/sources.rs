//! Parsing helpers for explicit registry references.

use crate::error::{CliError, CliResult};

pub(super) fn parse_registry_scope_id(
    skill_id_input: &str,
) -> CliResult<(String, String, String, Option<String>)> {
    use crate::utils::parse_skill_id;
    use fastskill_core::security::path::validate_path_component;

    let (skill_id_full, version) = parse_skill_id(skill_id_input);
    let (scope, expected_id) = skill_id_full.split_once('/').ok_or_else(|| {
        CliError::Config(format!(
            "Registry skill ID must be in format 'scope/id', got: {skill_id_full}"
        ))
    })?;
    validate_path_component(scope)
        .map_err(|error| CliError::Config(format!("Invalid registry scope '{scope}': {error}")))?;
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
    fn registry_reference_requires_a_scope() {
        assert!(parse_registry_scope_id("no-scope").is_err());
    }

    #[test]
    fn registry_reference_rejects_unsafe_components() {
        assert!(parse_registry_scope_id("../evil").is_err());
        assert!(parse_registry_scope_id("bad\\scope/id").is_err());
        assert!(parse_registry_scope_id("safe/../evil").is_err());
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
