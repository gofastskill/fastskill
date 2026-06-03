//! Convenience constructors for `cli_framework` `ArgSpec` values.
//!
//! `ArgSpec` does not implement `Default` (it is `Debug + Clone` only), so we
//! provide a small set of named helpers that let command modules build specs
//! concisely without repeating every field.

use cli_framework::prelude::{ArgSpec, ArgValue};
use cli_framework::spec::{ArgKind, ArgValueType, Cardinality};

/// Build a base `ArgSpec` with `cardinality = Optional` and all optional
/// fields unset.
#[allow(dead_code)]
pub fn arg(name: &'static str, kind: ArgKind, value_type: ArgValueType) -> ArgSpec {
    ArgSpec {
        name,
        kind,
        value_type,
        short: None,
        long: None,
        cardinality: Cardinality::Optional,
        default: None,
        conflicts_with: vec![],
        requires: vec![],
        help: "",
        ..Default::default()
    }
}

/// Mark an arg as `Required`.
#[allow(dead_code)]
pub fn required(mut a: ArgSpec) -> ArgSpec {
    a.cardinality = Cardinality::Required;
    a
}

/// Add a short flag character (e.g. `'l'` → `-l`).
#[allow(dead_code)]
pub fn short(mut a: ArgSpec, s: char) -> ArgSpec {
    a.short = Some(s);
    a
}

/// Set a default value.
#[allow(dead_code)]
pub fn default_val(mut a: ArgSpec, v: ArgValue) -> ArgSpec {
    a.default = Some(v);
    a
}

/// Add conflict constraints (arg is mutually exclusive with each listed name).
#[allow(dead_code)]
pub fn conflicts(mut a: ArgSpec, with: &[&'static str]) -> ArgSpec {
    a.conflicts_with = with.to_vec();
    a
}

/// Set the help text.
#[allow(dead_code)]
pub fn help_text(mut a: ArgSpec, h: &'static str) -> ArgSpec {
    a.help = h;
    a
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_arg_helper_builds_optional_by_default() {
        let a = arg("query", ArgKind::Positional, ArgValueType::String);
        assert_eq!(a.name, "query");
        assert_eq!(a.cardinality, Cardinality::Optional);
        assert!(a.short.is_none());
        assert!(a.default.is_none());
    }

    #[test]
    fn test_required_changes_cardinality() {
        let a = required(arg("query", ArgKind::Positional, ArgValueType::String));
        assert_eq!(a.cardinality, Cardinality::Required);
    }

    #[test]
    fn test_short_sets_short_flag() {
        let a = short(arg("limit", ArgKind::Option, ArgValueType::Int), 'l');
        assert_eq!(a.short, Some('l'));
    }

    #[test]
    fn test_default_val() {
        let a = default_val(
            arg("limit", ArgKind::Option, ArgValueType::Int),
            ArgValue::Int(10),
        );
        assert!(matches!(a.default, Some(ArgValue::Int(10))));
    }

    #[test]
    fn test_conflicts() {
        let a = conflicts(arg("local", ArgKind::Flag, ArgValueType::Bool), &["remote"]);
        assert_eq!(a.conflicts_with, vec!["remote"]);
    }
}
