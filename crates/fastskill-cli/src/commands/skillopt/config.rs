//! SkillOpt config: TOML deserialization, validation, and shared helpers.

use crate::error::{CliError, CliResult};
use fastskill_evals::EvalCase;
use serde::Deserialize;
use std::path::{Path, PathBuf};

/// Deserialization target for `skillopt.toml`.
#[derive(Debug, Deserialize)]
pub struct SkillOptToml {
    // artifact & data
    pub skill: String,
    pub skill_name: String,
    pub suite: String,
    pub checks: Option<String>,
    pub out_dir: String,

    // agents
    pub target_agent: String,
    pub target_model: Option<String>,
    pub optimizer_agent: Option<String>,
    pub optimizer_model: Option<String>,

    // loop
    pub n_epochs: u32,
    pub batch_size: u32,
    pub accumulation: u32,
    pub aggregate_group_size: u32,
    pub lr_0: u32,
    pub pass_threshold: f64,

    // gate
    pub gate_metric: GateMetricToml,
    pub mixed_hard_weight: Option<f64>,
    pub gate_trials: u32,
    pub gate_epsilon: f64,

    // epoch-boundary
    pub slow_update_mode: SlowUpdateModeToml,
    pub protected_soft_cap_chars: u32,

    // execution
    pub timeout_seconds: u64,
    pub parallel: Option<u32>,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum GateMetricToml {
    Hard,
    Soft,
    Mixed,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SlowUpdateModeToml {
    Gated,
    ForceAccept,
}

/// Validate all parse-time invariants for a `SkillOptToml`.
///
/// Structural checks run before file-existence checks so pure config errors
/// (bad gate_metric, out-of-range fields) are caught without needing any files
/// to exist on disk.
pub fn validate_config(cfg: &SkillOptToml, base_dir: &Path) -> CliResult<()> {
    // -- Structural: gate_metric / mixed_hard_weight consistency --
    match (&cfg.gate_metric, &cfg.mixed_hard_weight) {
        (GateMetricToml::Mixed, None) => {
            return Err(CliError::Config(
                "SKILLOPT_MIXED_WEIGHT_MISSING: gate_metric is 'mixed' but mixed_hard_weight is not set".to_string(),
            ));
        }
        (metric, Some(_)) if *metric != GateMetricToml::Mixed => {
            return Err(CliError::Config(
                "SKILLOPT_MIXED_WEIGHT_SPURIOUS: mixed_hard_weight is set but gate_metric is not 'mixed'".to_string(),
            ));
        }
        _ => {}
    }

    // -- Structural: field range checks --
    if let Some(w) = cfg.mixed_hard_weight {
        if !(0.0..=1.0).contains(&w) {
            return Err(CliError::Config(format!(
                "SKILLOPT_FIELD_OUT_OF_RANGE: mixed_hard_weight must be in [0.0, 1.0], got {w}"
            )));
        }
    }
    if !(0.0..=1.0).contains(&cfg.pass_threshold) {
        return Err(CliError::Config(format!(
            "SKILLOPT_FIELD_OUT_OF_RANGE: pass_threshold must be in [0.0, 1.0], got {}",
            cfg.pass_threshold
        )));
    }
    if !(0.0..=1.0).contains(&cfg.gate_epsilon) {
        return Err(CliError::Config(format!(
            "SKILLOPT_FIELD_OUT_OF_RANGE: gate_epsilon must be in [0.0, 1.0], got {}",
            cfg.gate_epsilon
        )));
    }
    if cfg.batch_size < 1 {
        return Err(CliError::Config(
            "SKILLOPT_FIELD_OUT_OF_RANGE: batch_size must be >= 1".to_string(),
        ));
    }
    if cfg.accumulation < 1 {
        return Err(CliError::Config(
            "SKILLOPT_FIELD_OUT_OF_RANGE: accumulation must be >= 1".to_string(),
        ));
    }
    if cfg.lr_0 < 1 {
        return Err(CliError::Config(
            "SKILLOPT_FIELD_OUT_OF_RANGE: lr_0 must be >= 1".to_string(),
        ));
    }
    if cfg.n_epochs < 1 {
        return Err(CliError::Config(
            "SKILLOPT_FIELD_OUT_OF_RANGE: n_epochs must be >= 1".to_string(),
        ));
    }

    // -- File-existence checks --
    let skill_path = base_dir.join(&cfg.skill);
    if !skill_path.exists() {
        return Err(CliError::Config(format!(
            "SKILLOPT_SKILL_NOT_FOUND: skill file not found: {}",
            skill_path.display()
        )));
    }

    let suite_path = base_dir.join(&cfg.suite);
    if !suite_path.exists() {
        return Err(CliError::Config(format!(
            "SKILLOPT_SUITE_NOT_FOUND: suite CSV not found: {}",
            suite_path.display()
        )));
    }

    if let Some(checks_path) = &cfg.checks {
        let checks_path = base_dir.join(checks_path);
        if !checks_path.exists() {
            return Err(CliError::Config(format!(
                "SKILLOPT_CHECKS_PARSE_ERROR: checks file not found: {}",
                checks_path.display()
            )));
        }
    }

    Ok(())
}

/// Build an `aikit_skillopt::RunConfig` from `SkillOptToml` via serde_json,
/// avoiding direct imports of `GateMetric` and `SlowUpdateMode`.
pub fn build_run_config(
    cfg: &SkillOptToml,
    optimizer_agent: &str,
) -> anyhow::Result<aikit_skillopt::RunConfig> {
    let gate_metric_json = match cfg.gate_metric {
        GateMetricToml::Hard => serde_json::json!("Hard"),
        GateMetricToml::Soft => serde_json::json!("Soft"),
        GateMetricToml::Mixed => serde_json::json!({
            "Mixed": { "hard_weight": cfg.mixed_hard_weight.unwrap_or(0.5) }
        }),
    };

    let slow_update_json = match cfg.slow_update_mode {
        SlowUpdateModeToml::Gated => serde_json::json!("Gated"),
        SlowUpdateModeToml::ForceAccept => serde_json::json!("ForceAccept"),
    };

    let config_json = serde_json::json!({
        "n_epochs": cfg.n_epochs,
        "batch_size": cfg.batch_size,
        "accumulation": cfg.accumulation,
        "aggregate_group_size": cfg.aggregate_group_size,
        "lr_0": cfg.lr_0,
        "pass_threshold": cfg.pass_threshold,
        "gate_metric": gate_metric_json,
        "gate_trials": cfg.gate_trials,
        "gate_epsilon": cfg.gate_epsilon,
        "slow_update_mode": slow_update_json,
        "protected_soft_cap_chars": cfg.protected_soft_cap_chars,
        "target_agent": cfg.target_agent,
        "target_model": cfg.target_model,
        "optimizer_agent": optimizer_agent,
        "optimizer_model": cfg.optimizer_model,
        "timeout_seconds": cfg.timeout_seconds,
        "parallel": cfg.parallel,
        "artifact_stem": "skill",
    });

    Ok(serde_json::from_value(config_json)?)
}

/// Load a suite CSV, resolve split tags, and return the cases plus a selection count.
///
/// The CSV may have a dedicated `split` column or embed split info as `split:<value>`
/// in the `tags` column. If neither is present a row is assigned `train`.
pub fn load_suite_with_splits(suite_path: &Path) -> Result<(Vec<EvalCase>, usize), String> {
    if !suite_path.exists() {
        return Err(format!(
            "SKILLOPT_SUITE_NOT_FOUND: suite CSV not found: {}",
            suite_path.display()
        ));
    }

    let content = std::fs::read_to_string(suite_path)
        .map_err(|e| format!("SKILLOPT_SUITE_PARSE_ERROR: cannot read suite CSV: {e}"))?;

    parse_suite_csv_with_splits(&content)
}

fn parse_suite_csv_with_splits(content: &str) -> Result<(Vec<EvalCase>, usize), String> {
    let mut lines = content.lines();

    let header_line = lines
        .next()
        .ok_or("SKILLOPT_SUITE_PARSE_ERROR: CSV is empty")?;

    let headers: Vec<String> = parse_csv_line(header_line);

    let id_idx =
        find_col_idx(&headers, "id").ok_or("SKILLOPT_SUITE_PARSE_ERROR: missing 'id' column")?;
    let prompt_idx = find_col_idx(&headers, "prompt")
        .ok_or("SKILLOPT_SUITE_PARSE_ERROR: missing 'prompt' column")?;
    let should_trigger_idx = find_col_idx(&headers, "should_trigger")
        .ok_or("SKILLOPT_SUITE_PARSE_ERROR: missing 'should_trigger' column")?;
    let tags_idx = find_col_idx(&headers, "tags");
    let workspace_subdir_idx = find_col_idx(&headers, "workspace_subdir");
    let split_col_idx = find_col_idx(&headers, "split");

    let mut cases: Vec<EvalCase> = Vec::new();
    let mut selection_count: usize = 0;

    for line in lines {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let cols = parse_csv_line(line);

        let id = cols
            .get(id_idx)
            .cloned()
            .unwrap_or_default()
            .trim()
            .to_string();
        if id.is_empty() {
            continue;
        }

        let prompt = cols
            .get(prompt_idx)
            .cloned()
            .unwrap_or_default()
            .trim()
            .to_string();
        let should_trigger_str = cols
            .get(should_trigger_idx)
            .cloned()
            .unwrap_or_default()
            .trim()
            .to_lowercase();
        let should_trigger = should_trigger_str != "false" && !should_trigger_str.is_empty();

        let mut tags: Vec<String> = if let Some(ti) = tags_idx {
            let tags_str = cols.get(ti).cloned().unwrap_or_default();
            let tags_str = tags_str.trim().to_string();
            if tags_str.is_empty() {
                vec![]
            } else {
                tags_str.split_whitespace().map(|s| s.to_string()).collect()
            }
        } else {
            vec![]
        };

        // Determine split value
        let split_val = if let Some(si) = split_col_idx {
            let sv = cols.get(si).cloned().unwrap_or_default().trim().to_string();
            if sv.is_empty() {
                "train".to_string()
            } else {
                sv
            }
        } else {
            // Look for split:<value> tag
            let found = tags
                .iter()
                .find(|t| t.starts_with("split:"))
                .map(|t| t["split:".len()..].to_string());
            found.unwrap_or_else(|| "train".to_string())
        };

        // Normalise: remove any split:* tags and add the bare split value
        tags.retain(|t| !t.starts_with("split:"));
        if !tags.contains(&split_val) {
            tags.push(split_val.clone());
        }

        if split_val == "selection" {
            selection_count += 1;
        }

        let workspace_subdir = workspace_subdir_idx
            .and_then(|wi| cols.get(wi))
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .map(PathBuf::from);

        cases.push(EvalCase {
            id,
            prompt,
            should_trigger,
            tags,
            workspace_subdir,
        });
    }

    Ok((cases, selection_count))
}

fn find_col_idx(headers: &[String], name: &str) -> Option<usize> {
    headers.iter().position(|h| h.trim() == name)
}

/// Minimal RFC-4180-compatible CSV line parser (handles double-quoted fields).
fn parse_csv_line(line: &str) -> Vec<String> {
    let mut fields: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut chars = line.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            '"' if !in_quotes => in_quotes = true,
            '"' if in_quotes => {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    current.push('"');
                } else {
                    in_quotes = false;
                }
            }
            ',' if !in_quotes => {
                fields.push(current.clone());
                current.clear();
            }
            _ => current.push(c),
        }
    }
    fields.push(current);
    fields
}
