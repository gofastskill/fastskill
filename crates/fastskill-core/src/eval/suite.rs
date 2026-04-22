//! Eval suite loading and CSV parsing

use std::path::{Path, PathBuf};
use thiserror::Error;

/// An individual eval case definition
#[derive(Debug, Clone)]
pub struct EvalCase {
    /// Unique case identifier (alphanumeric + hyphens)
    pub id: String,
    /// Prompt to send to the agent
    pub prompt: String,
    /// Whether the skill should trigger (documentation-only; checks.toml is authoritative for pass/fail)
    pub should_trigger: bool,
    /// Tags for filtering
    pub tags: Vec<String>,
    /// Optional workspace subdirectory (relative to skill project root)
    pub workspace_subdir: Option<PathBuf>,
}

/// A collection of eval cases
#[derive(Debug, Default)]
pub struct EvalSuite {
    pub cases: Vec<EvalCase>,
}

impl EvalSuite {
    pub fn new(cases: Vec<EvalCase>) -> Self {
        Self { cases }
    }

    /// Filter cases by ID
    pub fn filter_by_id(&self, id: &str) -> EvalSuite {
        EvalSuite {
            cases: self.cases.iter().filter(|c| c.id == id).cloned().collect(),
        }
    }

    /// Filter cases by tag
    pub fn filter_by_tag(&self, tag: &str) -> EvalSuite {
        EvalSuite {
            cases: self
                .cases
                .iter()
                .filter(|c| c.tags.iter().any(|t| t == tag))
                .cloned()
                .collect(),
        }
    }
}

/// Errors that can occur when loading an eval suite
#[derive(Debug, Error)]
pub enum SuiteError {
    #[error("EVAL_PROMPTS_NOT_FOUND: Prompts CSV file not found: {0}")]
    PromptsNotFound(PathBuf),
    #[error("EVAL_INVALID_CSV: {0}")]
    InvalidCsv(String),
    #[error("EVAL_INVALID_CSV: IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// Load an eval suite from a CSV file
///
/// Expected CSV columns: id,prompt,should_trigger,tags,workspace_subdir
pub fn load_suite(prompts_path: &Path) -> Result<EvalSuite, SuiteError> {
    if !prompts_path.exists() {
        return Err(SuiteError::PromptsNotFound(prompts_path.to_path_buf()));
    }

    let content = std::fs::read_to_string(prompts_path)?;
    parse_prompts_csv(&content)
}

/// Parse prompts CSV content
fn parse_prompts_csv(content: &str) -> Result<EvalSuite, SuiteError> {
    let mut lines = content.lines();

    // Parse header
    let header = lines
        .next()
        .ok_or_else(|| SuiteError::InvalidCsv("CSV is empty".to_string()))?;
    let headers: Vec<String> = parse_csv_line(header);

    // Find column indices
    let id_idx = find_col(&headers, "id")?;
    let prompt_idx = find_col(&headers, "prompt")?;
    let should_trigger_idx = find_col(&headers, "should_trigger")?;
    let tags_idx = headers.iter().position(|h| h.trim() == "tags");
    let workspace_subdir_idx = headers.iter().position(|h| h.trim() == "workspace_subdir");

    let mut cases = Vec::new();

    for (line_num, line) in lines.enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let cols = parse_csv_line(line);

        let id = cols
            .get(id_idx)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                SuiteError::InvalidCsv(format!("Missing id at line {}", line_num + 2))
            })?;

        let prompt = cols
            .get(prompt_idx)
            .map(|s| s.trim_matches('"').trim().to_string())
            .ok_or_else(|| {
                SuiteError::InvalidCsv(format!("Missing prompt at line {}", line_num + 2))
            })?;

        let should_trigger_str = cols
            .get(should_trigger_idx)
            .map(|s| s.trim().to_lowercase())
            .unwrap_or_else(|| "false".to_string());
        let should_trigger = should_trigger_str == "true" || should_trigger_str == "1";

        let tags = if let Some(idx) = tags_idx {
            cols.get(idx)
                .map(|s| {
                    s.trim()
                        .trim_matches('"')
                        .split(',')
                        .map(|t| t.trim().to_string())
                        .filter(|t| !t.is_empty())
                        .collect()
                })
                .unwrap_or_default()
        } else {
            vec![]
        };

        let workspace_subdir = if let Some(idx) = workspace_subdir_idx {
            cols.get(idx).and_then(|s| {
                let s = s.trim();
                if s.is_empty() {
                    None
                } else {
                    Some(PathBuf::from(s))
                }
            })
        } else {
            None
        };

        cases.push(EvalCase {
            id,
            prompt,
            should_trigger,
            tags,
            workspace_subdir,
        });
    }

    Ok(EvalSuite::new(cases))
}

fn find_col(headers: &[String], name: &str) -> Result<usize, SuiteError> {
    headers
        .iter()
        .position(|h| h.trim() == name)
        .ok_or_else(|| SuiteError::InvalidCsv(format!("Missing required column: {}", name)))
}

/// Simple CSV line parser that handles quoted fields
fn parse_csv_line(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut chars = line.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            '"' => {
                if in_quotes {
                    // Check for escaped quote ""
                    if chars.peek() == Some(&'"') {
                        chars.next();
                        current.push('"');
                    } else {
                        in_quotes = false;
                    }
                } else {
                    in_quotes = true;
                }
            }
            ',' if !in_quotes => {
                fields.push(current.clone());
                current.clear();
            }
            _ => {
                current.push(ch);
            }
        }
    }
    fields.push(current);
    fields
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_prompts_csv_basic() {
        let csv = "id,prompt,should_trigger,tags,workspace_subdir\n\
                   test-1,\"Do something\",true,\"basic\",\n\
                   test-2,\"Do nothing\",false,\"\",\n";
        let suite = parse_prompts_csv(csv).unwrap();
        assert_eq!(suite.cases.len(), 2);
        assert_eq!(suite.cases[0].id, "test-1");
        assert!(suite.cases[0].should_trigger);
        assert_eq!(suite.cases[1].id, "test-2");
        assert!(!suite.cases[1].should_trigger);
    }

    #[test]
    fn test_parse_prompts_csv_missing_required_col() {
        let csv = "id,prompt\ntest-1,hello\n";
        let result = parse_prompts_csv(csv);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("should_trigger"));
    }

    #[test]
    fn test_filter_by_id() {
        let cases = vec![
            EvalCase {
                id: "a".to_string(),
                prompt: "p1".to_string(),
                should_trigger: true,
                tags: vec![],
                workspace_subdir: None,
            },
            EvalCase {
                id: "b".to_string(),
                prompt: "p2".to_string(),
                should_trigger: false,
                tags: vec![],
                workspace_subdir: None,
            },
        ];
        let suite = EvalSuite::new(cases);
        let filtered = suite.filter_by_id("a");
        assert_eq!(filtered.cases.len(), 1);
        assert_eq!(filtered.cases[0].id, "a");
    }

    #[test]
    fn test_filter_by_tag() {
        let cases = vec![
            EvalCase {
                id: "a".to_string(),
                prompt: "p1".to_string(),
                should_trigger: true,
                tags: vec!["foo".to_string(), "bar".to_string()],
                workspace_subdir: None,
            },
            EvalCase {
                id: "b".to_string(),
                prompt: "p2".to_string(),
                should_trigger: false,
                tags: vec!["baz".to_string()],
                workspace_subdir: None,
            },
        ];
        let suite = EvalSuite::new(cases);
        let filtered = suite.filter_by_tag("foo");
        assert_eq!(filtered.cases.len(), 1);
        assert_eq!(filtered.cases[0].id, "a");
    }
}
