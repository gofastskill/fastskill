//! Runtime selection primitives shared across sync, eval run, and eval validate.

use std::collections::HashSet;
use thiserror::Error;

/// Input mirroring the two CLI flags directly.
#[derive(Debug, Clone, Default)]
pub struct RuntimeSelectionInput {
    /// Runtime IDs requested via `--agent` flags (may contain duplicates from CLI).
    pub agents: Vec<String>,
    /// Whether `--all` was specified.
    pub all: bool,
}

/// Resolved, validated, deterministically-ordered target set.
#[derive(Debug, Clone)]
pub struct RuntimeSelection {
    /// Deduplicated, ordered list of runtime IDs.
    pub runtimes: Vec<String>,
    /// How the selection was derived.
    pub source: SelectionSource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelectionSource {
    /// Populated from one or more `--agent` flags; preserves first-occurrence order.
    Explicit,
    /// Populated via `--all`; sorted alphabetically.
    All,
}

#[derive(Debug, Error)]
pub enum RuntimeSelectionError {
    #[error("--agent and --all are mutually exclusive; use one or the other")]
    ConflictingFlags,

    #[error(
        "Unknown runtime ID(s): {unknown}. Available: {available}",
        unknown = .unknown.join(", "),
        available = .available.join(", ")
    )]
    UnknownRuntimeIds {
        unknown: Vec<String>,
        available: Vec<String>,
    },

    #[error("--all was requested but no runtimes are available. {hint}")]
    EmptyRuntimeSet { hint: String },
}

const RUNNABLE_AGENTS: &[&str] = &["codex", "claude", "gemini", "opencode", "agent", "aikit"];

/// Resolve runtime targets from CLI flags.
///
/// Returns `Ok(Some(_))` when flags are provided and valid.
/// Returns `Ok(None)` when neither `--agent` nor `--all` was provided.
/// Returns `Err(_)` on validation failure.
pub fn resolve_runtime_selection(
    input: &RuntimeSelectionInput,
) -> Result<Option<RuntimeSelection>, RuntimeSelectionError> {
    resolve_with_discovery(input, &|| {
        RUNNABLE_AGENTS.iter().map(|s| s.to_string()).collect()
    })
}

fn resolve_with_discovery(
    input: &RuntimeSelectionInput,
    discover: &dyn Fn() -> Vec<String>,
) -> Result<Option<RuntimeSelection>, RuntimeSelectionError> {
    // Step 1: Conflict check.
    if input.all && !input.agents.is_empty() {
        return Err(RuntimeSelectionError::ConflictingFlags);
    }

    // Step 2: --all path.
    if input.all {
        let mut available = discover();
        if available.is_empty() {
            return Err(RuntimeSelectionError::EmptyRuntimeSet {
                hint: "Install at least one aikit-supported runtime (e.g. codex, claude). \
                       See https://github.com/goaikit/aikit."
                    .to_string(),
            });
        }
        available.sort();
        return Ok(Some(RuntimeSelection {
            runtimes: available,
            source: SelectionSource::All,
        }));
    }

    // Step 3: --agent path.
    if !input.agents.is_empty() {
        let available = discover();

        // Deduplicate while preserving first-occurrence order.
        let mut seen = HashSet::new();
        let mut deduplicated: Vec<String> = Vec::new();
        for id in &input.agents {
            if seen.insert(id.clone()) {
                deduplicated.push(id.clone());
            }
        }

        let available_set: HashSet<&str> = available.iter().map(|s| s.as_str()).collect();
        let unknown: Vec<String> = deduplicated
            .iter()
            .filter(|id| !available_set.contains(id.as_str()))
            .cloned()
            .collect();

        if !unknown.is_empty() {
            return Err(RuntimeSelectionError::UnknownRuntimeIds { unknown, available });
        }

        return Ok(Some(RuntimeSelection {
            runtimes: deduplicated,
            source: SelectionSource::Explicit,
        }));
    }

    // Step 4: No flags provided.
    Ok(None)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn mock_runtimes() -> Vec<String> {
        vec![
            "codex".to_string(),
            "claude".to_string(),
            "gemini".to_string(),
            "agent".to_string(),
        ]
    }

    fn mock_empty() -> Vec<String> {
        vec![]
    }

    fn resolve(
        input: RuntimeSelectionInput,
    ) -> Result<Option<RuntimeSelection>, RuntimeSelectionError> {
        resolve_with_discovery(&input, &mock_runtimes)
    }

    #[test]
    fn test_single_valid_agent() {
        let input = RuntimeSelectionInput {
            agents: vec!["codex".to_string()],
            all: false,
        };
        let result = resolve(input).unwrap().unwrap();
        assert_eq!(result.runtimes, vec!["codex"]);
        assert_eq!(result.source, SelectionSource::Explicit);
    }

    #[test]
    fn test_repeated_duplicated_agent() {
        let input = RuntimeSelectionInput {
            agents: vec![
                "claude".to_string(),
                "claude".to_string(),
                "codex".to_string(),
            ],
            all: false,
        };
        let result = resolve(input).unwrap().unwrap();
        // Deduplicated, first-occurrence order
        assert_eq!(result.runtimes, vec!["claude", "codex"]);
        assert_eq!(result.source, SelectionSource::Explicit);
    }

    #[test]
    fn test_unknown_agent_returns_error() {
        let input = RuntimeSelectionInput {
            agents: vec!["unknown-xyz".to_string()],
            all: false,
        };
        let err = resolve(input).unwrap_err();
        match err {
            RuntimeSelectionError::UnknownRuntimeIds { unknown, available } => {
                assert_eq!(unknown, vec!["unknown-xyz"]);
                assert!(available.contains(&"codex".to_string()));
            }
            _ => panic!("expected UnknownRuntimeIds"),
        }
    }

    #[test]
    fn test_all_returns_full_runtime_list() {
        let input = RuntimeSelectionInput {
            agents: vec![],
            all: true,
        };
        let result = resolve(input).unwrap().unwrap();
        assert_eq!(result.source, SelectionSource::All);
        assert_eq!(result.runtimes.len(), 4);
        // All known mock runtimes should be present
        assert!(result.runtimes.contains(&"codex".to_string()));
        assert!(result.runtimes.contains(&"claude".to_string()));
    }

    #[test]
    fn test_agent_and_all_conflict() {
        let input = RuntimeSelectionInput {
            agents: vec!["codex".to_string()],
            all: true,
        };
        let err = resolve(input).unwrap_err();
        assert!(matches!(err, RuntimeSelectionError::ConflictingFlags));
    }

    #[test]
    fn test_deterministic_alphabetical_ordering_for_all() {
        // Mock returns in non-alphabetical order.
        fn shuffled_runtimes() -> Vec<String> {
            vec![
                "zebra".to_string(),
                "apple".to_string(),
                "mango".to_string(),
            ]
        }
        let input = RuntimeSelectionInput {
            agents: vec![],
            all: true,
        };
        let result = resolve_with_discovery(&input, &shuffled_runtimes)
            .unwrap()
            .unwrap();
        assert_eq!(result.runtimes, vec!["apple", "mango", "zebra"]);
        assert_eq!(result.source, SelectionSource::All);
    }

    #[test]
    fn test_empty_runtime_discovery_returns_error() {
        let input = RuntimeSelectionInput {
            agents: vec![],
            all: true,
        };
        let err = resolve_with_discovery(&input, &mock_empty).unwrap_err();
        match err {
            RuntimeSelectionError::EmptyRuntimeSet { hint } => {
                assert!(hint.contains("Install"));
            }
            _ => panic!("expected EmptyRuntimeSet"),
        }
    }

    #[test]
    fn test_no_flags_returns_none() {
        let input = RuntimeSelectionInput {
            agents: vec![],
            all: false,
        };
        let result = resolve(input).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_first_occurrence_order_preserved() {
        let input = RuntimeSelectionInput {
            agents: vec![
                "codex".to_string(),
                "agent".to_string(),
                "claude".to_string(),
            ],
            all: false,
        };
        let result = resolve(input).unwrap().unwrap();
        assert_eq!(result.runtimes, vec!["codex", "agent", "claude"]);
    }
}
