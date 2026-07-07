//! Version constraint parsing and evaluation
//!
//! Constraints are backed by [`semver::VersionReq`], which correctly handles
//! caret 0.x semantics, strict `<`/`>` bounds, two-component `^1.2`/`~1.2`,
//! and comma ranges (BUG-2/3/4/5).
//!
//! **CRITICAL — see [ADR-0004](../../../../docs/adr/0004-bare-version-is-exact.md):**
//! a *bare* `MAJOR.MINOR.PATCH` (no operator, no comma) is an **exact pin**, not a
//! caret range. `VersionReq::parse("1.2.3")` would apply Cargo caret semantics
//! (`>=1.2.3,<2.0.0`), so [`normalize_constraint`] rewrites a bare full version to
//! `=MAJOR.MINOR.PATCH` before parsing. Do **not** remove that normalization —
//! deleting it silently widens every committed bare pin from "exactly X" to a range.

use semver::{Version, VersionReq};
use thiserror::Error;

/// A version constraint, backed by a `semver::VersionReq`.
///
/// Parse via [`VersionConstraint::parse`]; test candidates via
/// [`VersionConstraint::satisfies`]. An empty string or `"*"` matches any version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionConstraint {
    req: VersionReq,
}

/// Errors related to version constraints
#[derive(Debug, Error)]
pub enum VersionError {
    #[error("Invalid version format: {0}")]
    InvalidVersion(String),

    #[error("Invalid constraint format: {0}")]
    InvalidConstraint(String),

    #[error("Failed to parse version: {0}")]
    ParseError(String),
}

/// Normalize a raw constraint string so that a bare `MAJOR.MINOR.PATCH` becomes an
/// exact pin (`=MAJOR.MINOR.PATCH`).
///
/// ⚠ **See [ADR-0004]. Do not remove this normalization.** Without it, a bare
/// `"1.2.3"` would be parsed by `VersionReq` as Cargo caret (`>=1.2.3,<2.0.0`),
/// silently widening every committed bare pin. Only a *full* three-component bare
/// version with no leading operator and no comma is rewritten — operators (`^ ~ > <
/// = *`), comma ranges, and two-component forms like `1.2` (caret) are left untouched
/// to preserve their existing meaning.
///
/// [ADR-0004]: ../../../../docs/adr/0004-bare-version-is-exact.md
fn normalize_constraint(constraint: &str) -> String {
    // Only a bare full version (no operator, no comma) is treated as an exact pin.
    let has_operator = constraint.starts_with(['^', '~', '>', '<', '=', '*']);
    if !has_operator && !constraint.contains(',') && Version::parse(constraint).is_ok() {
        return format!("={}", constraint);
    }
    constraint.to_string()
}

impl VersionConstraint {
    /// Parse a version constraint string.
    ///
    /// - empty / `"*"` → matches any version
    /// - bare `1.2.3` → **exact** pin (`=1.2.3`, per ADR-0004)
    /// - operators `^ ~ >= <= < >` and comma ranges → native `VersionReq` semantics
    pub fn parse(constraint: &str) -> Result<Self, VersionError> {
        let constraint = constraint.trim();

        if constraint.is_empty() || constraint == "*" {
            return Ok(VersionConstraint {
                req: VersionReq::STAR,
            });
        }

        let normalized = normalize_constraint(constraint);
        let req = VersionReq::parse(&normalized)
            .map_err(|_| VersionError::InvalidConstraint(constraint.to_string()))?;
        Ok(VersionConstraint { req })
    }

    /// Check if a version satisfies this constraint.
    pub fn satisfies(&self, version: &str) -> Result<bool, VersionError> {
        let ver = Version::parse(version).map_err(|e| {
            VersionError::ParseError(format!("Failed to parse version '{}': {}", version, e))
        })?;
        Ok(self.req.matches(&ver))
    }

    /// The underlying `semver::VersionReq`.
    pub fn as_req(&self) -> &VersionReq {
        &self.req
    }
}

/// Compare two version strings
pub fn compare_versions(v1: &str, v2: &str) -> Result<std::cmp::Ordering, VersionError> {
    let ver1 = Version::parse(v1).map_err(|e| {
        VersionError::ParseError(format!("Failed to parse version '{}': {}", v1, e))
    })?;
    let ver2 = Version::parse(v2).map_err(|e| {
        VersionError::ParseError(format!("Failed to parse version '{}': {}", v2, e))
    })?;
    Ok(ver1.cmp(&ver2))
}

/// Check if version1 is newer than version2
pub fn is_newer(v1: &str, v2: &str) -> Result<bool, VersionError> {
    Ok(compare_versions(v1, v2)? == std::cmp::Ordering::Greater)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn test_exact_constraint() {
        let constraint = VersionConstraint::parse("1.2.3").unwrap();
        assert!(constraint.satisfies("1.2.3").unwrap());
        assert!(!constraint.satisfies("1.2.4").unwrap());
    }

    #[test]
    fn test_caret_constraint() {
        let constraint = VersionConstraint::parse("^1.2.3").unwrap();
        assert!(constraint.satisfies("1.2.3").unwrap());
        assert!(constraint.satisfies("1.3.0").unwrap());
        assert!(!constraint.satisfies("2.0.0").unwrap());
    }

    #[test]
    fn test_tilde_constraint() {
        let constraint = VersionConstraint::parse("~1.2.0").unwrap();
        assert!(constraint.satisfies("1.2.0").unwrap());
        assert!(constraint.satisfies("1.2.5").unwrap());
        assert!(!constraint.satisfies("1.3.0").unwrap());
    }

    #[test]
    fn test_greater_equal_constraint() {
        let constraint = VersionConstraint::parse(">=1.0.0").unwrap();
        assert!(constraint.satisfies("1.0.0").unwrap());
        assert!(constraint.satisfies("2.0.0").unwrap());
        assert!(!constraint.satisfies("0.9.0").unwrap());
    }

    #[test]
    fn test_compare_versions() {
        assert_eq!(
            compare_versions("1.2.3", "1.2.4").unwrap(),
            std::cmp::Ordering::Less
        );
        assert_eq!(
            compare_versions("2.0.0", "1.9.9").unwrap(),
            std::cmp::Ordering::Greater
        );
        assert_eq!(
            compare_versions("1.2.3", "1.2.3").unwrap(),
            std::cmp::Ordering::Equal
        );
    }

    #[test]
    fn test_is_newer() {
        assert!(is_newer("1.2.4", "1.2.3").unwrap());
        assert!(!is_newer("1.2.3", "1.2.4").unwrap());
    }

    // --- Regression tests for BUG-2/3/4/5 + ADR-0004 ---

    /// ADR-0004: a bare `1.2.3` is an EXACT pin, never a caret range.
    #[test]
    fn test_bare_version_is_exact_pin() {
        let constraint = VersionConstraint::parse("1.2.3").unwrap();
        assert!(constraint.satisfies("1.2.3").unwrap());
        // Must NOT accept any newer compatible version (would be caret behavior).
        assert!(!constraint.satisfies("1.2.4").unwrap());
        assert!(!constraint.satisfies("1.3.0").unwrap());
        assert!(!constraint.satisfies("2.0.0").unwrap());
        assert!(!constraint.satisfies("1.2.2").unwrap());
    }

    /// Explicit `=1.2.3` behaves the same as a bare pin.
    #[test]
    fn test_explicit_equals_is_exact() {
        let constraint = VersionConstraint::parse("=1.2.3").unwrap();
        assert!(constraint.satisfies("1.2.3").unwrap());
        assert!(!constraint.satisfies("1.2.4").unwrap());
    }

    /// BUG-2: caret on a 0.x version honors the semver 0.x rule
    /// (`^0.2.3` == `>=0.2.3, <0.3.0`), so `0.9.0` must be rejected.
    #[test]
    fn test_caret_zerox_rejects_incompatible() {
        let constraint = VersionConstraint::parse("^0.2.3").unwrap();
        assert!(constraint.satisfies("0.2.3").unwrap());
        assert!(constraint.satisfies("0.2.9").unwrap());
        // 0.9.0 is a breaking pre-1.0 change and must NOT satisfy.
        assert!(!constraint.satisfies("0.9.0").unwrap());
        assert!(!constraint.satisfies("0.3.0").unwrap());
    }

    /// BUG-2: `^0.0.3` is exactly `0.0.3` under semver.
    #[test]
    fn test_caret_zero_zero_x_is_exact() {
        let constraint = VersionConstraint::parse("^0.0.3").unwrap();
        assert!(constraint.satisfies("0.0.3").unwrap());
        assert!(!constraint.satisfies("0.0.4").unwrap());
    }

    /// BUG-3: a strict `<` upper bound is exclusive, so `2.0.0` is rejected.
    #[test]
    fn test_range_strict_upper_bound_excludes() {
        let constraint = VersionConstraint::parse(">=1.0.0,<2.0.0").unwrap();
        assert!(constraint.satisfies("1.0.0").unwrap());
        assert!(constraint.satisfies("1.9.9").unwrap());
        // Strict `<` must EXCLUDE the upper bound.
        assert!(!constraint.satisfies("2.0.0").unwrap());
        assert!(!constraint.satisfies("0.9.0").unwrap());
    }

    /// BUG-4: bare single strict `>` and `<` constraints parse and evaluate.
    #[test]
    fn test_bare_strict_greater_than() {
        let constraint = VersionConstraint::parse(">1.0.0").unwrap();
        assert!(constraint.satisfies("1.0.1").unwrap());
        assert!(!constraint.satisfies("1.0.0").unwrap());
        assert!(!constraint.satisfies("0.9.9").unwrap());
    }

    #[test]
    fn test_bare_strict_less_than() {
        let constraint = VersionConstraint::parse("<2.0.0").unwrap();
        assert!(constraint.satisfies("1.9.9").unwrap());
        assert!(!constraint.satisfies("2.0.0").unwrap());
        assert!(!constraint.satisfies("2.0.1").unwrap());
    }

    /// BUG-5: a two-component `^1.2` parses and matches (does not error at match time).
    #[test]
    fn test_two_component_caret() {
        let constraint = VersionConstraint::parse("^1.2").unwrap();
        assert!(constraint.satisfies("1.2.0").unwrap());
        assert!(constraint.satisfies("1.5.0").unwrap());
        assert!(!constraint.satisfies("2.0.0").unwrap());
        // Below the 1.2 floor is rejected.
        assert!(!constraint.satisfies("1.1.0").unwrap());
    }

    /// BUG-5: two-component `~1.2` parses and matches.
    #[test]
    fn test_two_component_tilde() {
        let constraint = VersionConstraint::parse("~1.2").unwrap();
        assert!(constraint.satisfies("1.2.0").unwrap());
        assert!(constraint.satisfies("1.2.9").unwrap());
        // ~1.2 == >=1.2.0, <1.3.0
        assert!(!constraint.satisfies("1.3.0").unwrap());
    }

    #[test]
    fn test_less_equal_constraint() {
        let constraint = VersionConstraint::parse("<=2.0.0").unwrap();
        assert!(constraint.satisfies("2.0.0").unwrap());
        assert!(constraint.satisfies("1.0.0").unwrap());
        assert!(!constraint.satisfies("2.0.1").unwrap());
    }

    #[test]
    fn test_any_and_star_match_everything() {
        let empty = VersionConstraint::parse("").unwrap();
        assert!(empty.satisfies("0.0.1").unwrap());
        assert!(empty.satisfies("9.9.9").unwrap());

        let star = VersionConstraint::parse("*").unwrap();
        assert!(star.satisfies("0.0.1").unwrap());
        assert!(star.satisfies("123.4.5").unwrap());
    }

    #[test]
    fn test_whitespace_is_trimmed() {
        let constraint = VersionConstraint::parse("  1.2.3  ").unwrap();
        assert!(constraint.satisfies("1.2.3").unwrap());
        assert!(!constraint.satisfies("1.2.4").unwrap());
    }

    #[test]
    fn test_invalid_constraint_errors() {
        let err = VersionConstraint::parse("not-a-version").unwrap_err();
        assert!(matches!(err, VersionError::InvalidConstraint(_)));
    }

    #[test]
    fn test_satisfies_rejects_unparseable_version() {
        let constraint = VersionConstraint::parse(">=1.0.0").unwrap();
        let err = constraint.satisfies("not-a-version").unwrap_err();
        assert!(matches!(err, VersionError::ParseError(_)));
    }

    #[test]
    fn test_comma_range_two_sided_inclusive() {
        let constraint = VersionConstraint::parse(">=1.0.0,<=1.9.9").unwrap();
        assert!(constraint.satisfies("1.0.0").unwrap());
        assert!(constraint.satisfies("1.9.9").unwrap());
        assert!(!constraint.satisfies("2.0.0").unwrap());
        assert!(!constraint.satisfies("0.9.0").unwrap());
    }

    #[test]
    fn test_as_req_exposes_underlying() {
        let constraint = VersionConstraint::parse("^1.2.3").unwrap();
        // Round-trips as a caret requirement.
        assert_eq!(constraint.as_req().to_string(), "^1.2.3");
    }
}
