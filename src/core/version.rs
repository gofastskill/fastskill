//! Version constraint parsing and evaluation

use semver::{Version, VersionReq};
use thiserror::Error;

/// Version constraint types
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VersionConstraint {
    /// Exact version match (e.g., "1.2.3")
    Exact(String),
    /// Caret constraint (e.g., "^1.2.3" matches >=1.2.3 <2.0.0)
    Caret(String),
    /// Tilde constraint (e.g., "~1.2.0" matches >=1.2.0 <1.3.0)
    Tilde(String),
    /// Greater than or equal (e.g., ">=1.0.0")
    GreaterEqual(String),
    /// Less than or equal (e.g., "<=2.0.0")
    LessEqual(String),
    /// Range constraint (e.g., ">=1.0.0,<2.0.0")
    Range {
        min: Option<String>,
        max: Option<String>,
    },
    /// Any version (no constraint)
    Any,
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

impl VersionConstraint {
    /// Parse a version constraint string
    pub fn parse(constraint: &str) -> Result<Self, VersionError> {
        let constraint = constraint.trim();

        if constraint.is_empty() || constraint == "*" {
            return Ok(VersionConstraint::Any);
        }

        // Exact version
        if !constraint.starts_with('^')
            && !constraint.starts_with('~')
            && !constraint.starts_with('>')
            && !constraint.starts_with('<')
            && !constraint.contains(',')
        {
            // Try to parse as exact version
            if Version::parse(constraint).is_ok() {
                return Ok(VersionConstraint::Exact(constraint.to_string()));
            }
        }

        // Caret constraint (^1.2.3)
        if constraint.starts_with('^') {
            let version = constraint.trim_start_matches('^').trim();
            if Version::parse(version).is_ok() {
                return Ok(VersionConstraint::Caret(version.to_string()));
            }
        }

        // Tilde constraint (~1.2.0)
        if constraint.starts_with('~') {
            let version = constraint.trim_start_matches('~').trim();
            if Version::parse(version).is_ok() {
                return Ok(VersionConstraint::Tilde(version.to_string()));
            }
        }

        // Greater than or equal (>=1.0.0)
        if constraint.starts_with(">=") {
            let version = constraint.trim_start_matches(">=").trim();
            if Version::parse(version).is_ok() {
                return Ok(VersionConstraint::GreaterEqual(version.to_string()));
            }
        }

        // Less than or equal (<=2.0.0)
        if constraint.starts_with("<=") {
            let version = constraint.trim_start_matches("<=").trim();
            if Version::parse(version).is_ok() {
                return Ok(VersionConstraint::LessEqual(version.to_string()));
            }
        }

        // Range constraint (>=1.0.0,<2.0.0)
        if constraint.contains(',') {
            let parts: Vec<&str> = constraint.split(',').map(|s| s.trim()).collect();
            let mut min = None;
            let mut max = None;

            for part in parts {
                if part.starts_with(">=") {
                    let version = part.trim_start_matches(">=").trim();
                    if Version::parse(version).is_ok() {
                        min = Some(version.to_string());
                    }
                } else if part.starts_with("<=") {
                    let version = part.trim_start_matches("<=").trim();
                    if Version::parse(version).is_ok() {
                        max = Some(version.to_string());
                    }
                } else if part.starts_with('<') {
                    let version = part.trim_start_matches('<').trim();
                    if Version::parse(version).is_ok() {
                        max = Some(version.to_string());
                    }
                } else if part.starts_with('>') {
                    let version = part.trim_start_matches('>').trim();
                    if Version::parse(version).is_ok() {
                        min = Some(version.to_string());
                    }
                }
            }

            return Ok(VersionConstraint::Range { min, max });
        }

        // Try to parse as semver requirement
        if let Ok(req) = VersionReq::parse(constraint) {
            // Convert to our constraint type
            let req_str = req.to_string();
            if req_str.starts_with('^') {
                return Ok(VersionConstraint::Caret(
                    req_str.trim_start_matches('^').to_string(),
                ));
            } else if req_str.starts_with('~') {
                return Ok(VersionConstraint::Tilde(
                    req_str.trim_start_matches('~').to_string(),
                ));
            } else if req_str.starts_with(">=") {
                return Ok(VersionConstraint::GreaterEqual(
                    req_str.trim_start_matches(">=").to_string(),
                ));
            }
        }

        Err(VersionError::InvalidConstraint(constraint.to_string()))
    }

    /// Check if a version satisfies this constraint
    pub fn satisfies(&self, version: &str) -> Result<bool, VersionError> {
        let ver = Version::parse(version).map_err(|e| {
            VersionError::ParseError(format!("Failed to parse version '{}': {}", version, e))
        })?;

        match self {
            VersionConstraint::Exact(exact) => {
                let exact_ver = Version::parse(exact).map_err(|e| {
                    VersionError::ParseError(format!("Invalid exact version '{}': {}", exact, e))
                })?;
                Ok(ver == exact_ver)
            }
            VersionConstraint::Caret(base) => {
                let base_ver = Version::parse(base).map_err(|e| {
                    VersionError::ParseError(format!("Invalid caret base '{}': {}", base, e))
                })?;
                // ^1.2.3 means >=1.2.3 <2.0.0
                Ok(ver >= base_ver && ver.major == base_ver.major)
            }
            VersionConstraint::Tilde(base) => {
                let base_ver = Version::parse(base).map_err(|e| {
                    VersionError::ParseError(format!("Invalid tilde base '{}': {}", base, e))
                })?;
                // ~1.2.0 means >=1.2.0 <1.3.0
                Ok(ver >= base_ver && ver.major == base_ver.major && ver.minor == base_ver.minor)
            }
            VersionConstraint::GreaterEqual(min) => {
                let min_ver = Version::parse(min).map_err(|e| {
                    VersionError::ParseError(format!("Invalid min version '{}': {}", min, e))
                })?;
                Ok(ver >= min_ver)
            }
            VersionConstraint::LessEqual(max) => {
                let max_ver = Version::parse(max).map_err(|e| {
                    VersionError::ParseError(format!("Invalid max version '{}': {}", max, e))
                })?;
                Ok(ver <= max_ver)
            }
            VersionConstraint::Range { min, max } => {
                let mut satisfies = true;
                if let Some(min_str) = min {
                    let min_ver = Version::parse(min_str).map_err(|e| {
                        VersionError::ParseError(format!(
                            "Invalid min version '{}': {}",
                            min_str, e
                        ))
                    })?;
                    satisfies = satisfies && ver >= min_ver;
                }
                if let Some(max_str) = max {
                    let max_ver = Version::parse(max_str).map_err(|e| {
                        VersionError::ParseError(format!(
                            "Invalid max version '{}': {}",
                            max_str, e
                        ))
                    })?;
                    satisfies = satisfies && ver <= max_ver;
                }
                Ok(satisfies)
            }
            VersionConstraint::Any => Ok(true),
        }
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
}
