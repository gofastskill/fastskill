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

/// Cases parsed from a suite CSV plus counts of each recognized split tag.
///
/// `train_count` and `selection_count` mirror `aikit-textgrad`'s training-loop
/// semantics (`aikit-textgrad/src/training/mod.rs`'s `split_cases`): a case counts as
/// `train` unless its resolved split is exactly `selection` or `test` — including
/// untagged rows and any unrecognized split value, both of which the loop silently
/// treats as `train`.
#[derive(Debug)]
pub struct SuiteSplits {
    pub cases: Vec<EvalCase>,
    pub train_count: usize,
    pub selection_count: usize,
}

/// Load a suite CSV, resolve split tags, and return the cases plus split counts.
///
/// The CSV may have a dedicated `split` column or embed split info as `split:<value>`
/// in the `tags` column. If neither is present a row is assigned `train`.
pub fn load_suite_with_splits(suite_path: &Path) -> Result<SuiteSplits, String> {
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

fn parse_suite_csv_with_splits(content: &str) -> Result<SuiteSplits, String> {
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
    let mut train_count: usize = 0;
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

        // Count using aikit-textgrad's `split_cases` algorithm applied to the SAME
        // final tag vector the training loop will see -- not `split_val`.
        //
        // These can differ. `tags.retain` above only strips `split:`-prefixed tags, so a
        // bare recognized tag in the `tags` column (e.g. `tags=selection,split=train`)
        // survives and, because it was pushed first, is what upstream's `find` returns.
        // Counting `split_val` there would score the row `train` while the loop trains it
        // as `selection` -- letting a suite with no effective train cases slip past
        // OPTIMIZE_NO_TRAIN_CASES into exactly the silent no-op that guard exists to stop.
        match effective_split(&tags) {
            "selection" => selection_count += 1,
            "test" => {}
            _ => train_count += 1,
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

    Ok(SuiteSplits {
        cases,
        train_count,
        selection_count,
    })
}

/// Count entries recorded in `run_dir/history.json`, one `StepRecord` per training
/// step attempted (accepted or rejected).
///
/// A missing or unparseable file counts as zero steps rather than erroring — this is
/// a defensive, best-effort signal used only to decide whether to warn the user that
/// no training occurred, not a hard invariant.
pub fn count_history_steps(run_dir: &Path) -> usize {
    std::fs::read_to_string(run_dir.join("history.json"))
        .ok()
        .and_then(|s| serde_json::from_str::<Vec<serde_json::Value>>(&s).ok())
        .map(|steps| steps.len())
        .unwrap_or(0)
}

/// One `history.json` record, narrowed to the fields needed to locate skill versions.
#[derive(Debug, Deserialize)]
struct HistoryStep {
    global_step: u32,
    accepted: bool,
}

/// Which `skills/skill_v{NNNN}.md` documents bracket a given step.
#[derive(Debug, PartialEq, Eq)]
pub enum StepVersions {
    /// The step was accepted; diff these two version indices.
    Accepted { before: u32, after: u32 },
    /// The step ran and was rejected, so no new version was written.
    Rejected,
    /// The step is absent from `history.json`, or history is missing/unreadable.
    Unknown,
}

/// Resolve the skill-version indices bracketing `step` by reading `history.json`.
///
/// Version numbers advance only on **accepted** steps (upstream writes a new
/// `skill_v{NNNN}.md` solely in the accept branch), so a step index is *not* a version
/// index once any earlier step has been rejected. Assuming `step -> v{step}/v{step+1}`
/// silently renders a different step's diff: with step 0 rejected and steps 1 and 2
/// accepted, step 1's real diff is `v0000 -> v0001`, but the naive mapping shows
/// `v0001 -> v0002` — which is step 2's diff, displayed under step 1's name.
///
/// Counting accepted steps before `step` gives the exact "before" index instead.
pub fn resolve_step_versions(run_dir: &Path, step: u32) -> StepVersions {
    let Some(history) = std::fs::read_to_string(run_dir.join("history.json"))
        .ok()
        .and_then(|s| serde_json::from_str::<Vec<HistoryStep>>(&s).ok())
    else {
        return StepVersions::Unknown;
    };

    // Collapse to one outcome per step, last write winning.
    //
    // `append_history` (upstream `training/state.rs`) blindly pushes, and history is reset
    // to `[]` only when a run is *initialized* — not on resume. If a process dies between
    // the history append and the `runtime_state.json` save, resume re-runs that step and
    // appends a second record for the same `global_step`. Taking the first match would
    // then report the abandoned attempt, and counting raw records would inflate the
    // accepted-step count. The most recent record for a step is the real outcome.
    //
    // A BTreeMap also makes this independent of record order in the file.
    let mut outcomes: std::collections::BTreeMap<u32, bool> = std::collections::BTreeMap::new();
    for record in &history {
        outcomes.insert(record.global_step, record.accepted);
    }

    let Some(&accepted) = outcomes.get(&step) else {
        return StepVersions::Unknown;
    };

    if !accepted {
        return StepVersions::Rejected;
    }

    let accepted_before = outcomes
        .iter()
        .filter(|(&global_step, &accepted)| global_step < step && accepted)
        .count() as u32;

    StepVersions::Accepted {
        before: accepted_before,
        after: accepted_before + 1,
    }
}

/// Build the run-completion stdout line and an optional stderr warning.
///
/// A real run with at least one recorded training step prints the documented
/// `Run complete. Best skill: <path>` line and no warning. A run that completed with
/// zero recorded steps (e.g. every case fell into a split the training loop doesn't
/// step over) must not print that same success-shaped line — it gets a distinct
/// stdout message plus a stderr warning so the zero-step outcome is never silent.
pub fn completion_output(step_count: usize, best_artifact_path: &Path) -> (String, Option<String>) {
    if step_count == 0 {
        (
            format!(
                "No training steps were executed. Best skill (unchanged from input): {}",
                best_artifact_path.display()
            ),
            Some(
                "OPTIMIZE_ZERO_STEPS_WARN: training completed with zero recorded steps in \
                 history.json; no optimization occurred. Check that the suite has cases \
                 tagged 'train' and that n_epochs/batch_size/accumulation produce at least \
                 one step."
                    .to_string(),
            ),
        )
    } else {
        (
            format!("Run complete. Best skill: {}", best_artifact_path.display()),
            None,
        )
    }
}

/// Resolve a case's effective split exactly as `aikit-textgrad`'s `split_cases` does
/// (`aikit-textgrad/src/training/mod.rs`, pinned `a34e1e47`): the first tag that is
/// *exactly* `train`, `selection` or `test`, else `train`.
///
/// This must stay a mirror of upstream. Any divergence means fastskill's split counts
/// describe a different partition than the one the training loop actually runs, which is
/// how a zero-train suite can pass validation and then silently do nothing.
fn effective_split(tags: &[String]) -> &'static str {
    match tags
        .iter()
        .find(|t| t.as_str() == "train" || t.as_str() == "selection" || t.as_str() == "test")
        .map(|s| s.as_str())
    {
        Some("selection") => "selection",
        Some("test") => "test",
        _ => "train",
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_suite_counts_train_selection_and_test_splits() {
        let csv = "id,prompt,should_trigger,split\n\
                    c1,hello,true,train\n\
                    c2,world,true,selection\n\
                    c3,again,true,test\n";
        let splits = parse_suite_csv_with_splits(csv).expect("parse should succeed");
        assert_eq!(splits.cases.len(), 3);
        assert_eq!(splits.train_count, 1);
        assert_eq!(splits.selection_count, 1);
    }

    /// A suite with only `selection` + `test` rows — the undocumented-but-accepted
    /// shape from spec 013 finding #3 — must report zero train cases even though it
    /// passes the pre-existing `selection_count > 0` check.
    #[test]
    fn parse_suite_reports_zero_train_when_only_selection_and_test() {
        let csv = "id,prompt,should_trigger,split\n\
                    c1,hello,true,selection\n\
                    c2,world,true,test\n";
        let splits = parse_suite_csv_with_splits(csv).expect("parse should succeed");
        assert_eq!(splits.train_count, 0);
        assert_eq!(splits.selection_count, 1);
    }

    /// An absent `split` column defaults every row to `train`, matching
    /// `aikit-textgrad`'s own fallback for untagged cases.
    #[test]
    fn parse_suite_defaults_untagged_rows_to_train() {
        let csv = "id,prompt,should_trigger\nc1,hello,true\n";
        let splits = parse_suite_csv_with_splits(csv).expect("parse should succeed");
        assert_eq!(splits.train_count, 1);
        assert_eq!(splits.selection_count, 0);
    }

    /// An unrecognized split value (neither train/selection/test) falls through to
    /// `train`, mirroring aikit-textgrad's `split_cases` match fallback arm.
    #[test]
    fn parse_suite_counts_unrecognized_split_value_as_train() {
        let csv = "id,prompt,should_trigger,split\nc1,hello,true,bogus\n";
        let splits = parse_suite_csv_with_splits(csv).expect("parse should succeed");
        assert_eq!(splits.train_count, 1);
        assert_eq!(splits.selection_count, 0);
    }

    /// A bare recognized tag in the `tags` column wins over the `split` column, because
    /// `tags.retain` only strips `split:`-prefixed entries and upstream's `split_cases`
    /// takes the *first* recognized tag. Counting the `split` column instead would call
    /// this row `train` while the training loop treats it as `selection` — letting a
    /// suite with no effective train cases slip past OPTIMIZE_NO_TRAIN_CASES.
    #[test]
    fn parse_suite_prefers_bare_tag_over_split_column_like_upstream() {
        let csv = "id,prompt,should_trigger,tags,split\n\
                    c1,hello,true,selection,train\n";
        let splits = parse_suite_csv_with_splits(csv).expect("parse should succeed");
        assert_eq!(
            splits.selection_count, 1,
            "bare `selection` tag must win over split=train, matching split_cases"
        );
        assert_eq!(
            splits.train_count, 0,
            "row must not be double-counted as train"
        );
    }

    /// The whole-suite consequence of the above: every row resolves to `selection`, so
    /// there are zero train cases and `optimize run` must reject the suite.
    #[test]
    fn parse_suite_reports_zero_train_when_tags_override_split_column() {
        let csv = "id,prompt,should_trigger,tags,split\n\
                    c1,hello,true,selection,train\n\
                    c2,world,true,,selection\n";
        let splits = parse_suite_csv_with_splits(csv).expect("parse should succeed");
        assert_eq!(splits.train_count, 0);
        assert_eq!(splits.selection_count, 2);
    }

    #[test]
    fn effective_split_mirrors_upstream_precedence() {
        assert_eq!(effective_split(&["train".to_string()]), "train");
        assert_eq!(effective_split(&["selection".to_string()]), "selection");
        assert_eq!(effective_split(&["test".to_string()]), "test");
        assert_eq!(effective_split(&[]), "train");
        assert_eq!(effective_split(&["bogus".to_string()]), "train");
        // First recognized tag wins, regardless of what follows it.
        assert_eq!(
            effective_split(&["selection".to_string(), "train".to_string()]),
            "selection"
        );
        assert_eq!(
            effective_split(&["bogus".to_string(), "test".to_string()]),
            "test"
        );
    }

    #[test]
    fn count_history_steps_is_zero_for_missing_file() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        assert_eq!(count_history_steps(dir.path()), 0);
    }

    #[test]
    fn count_history_steps_is_zero_for_empty_array() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        std::fs::write(dir.path().join("history.json"), b"[]").expect("write");
        assert_eq!(count_history_steps(dir.path()), 0);
    }

    #[test]
    fn count_history_steps_counts_recorded_entries() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        std::fs::write(
            dir.path().join("history.json"),
            br#"[{"global_step":0},{"global_step":1}]"#,
        )
        .expect("write");
        assert_eq!(count_history_steps(dir.path()), 2);
    }

    fn write_history(dir: &Path, json: &str) {
        std::fs::write(dir.join("history.json"), json).expect("write");
    }

    #[test]
    fn resolve_step_versions_counts_accepted_steps_before_target() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        // Step 0 rejected, step 1 accepted -> step 1 is the FIRST accepted step,
        // so it maps to v0000 -> v0001, not v0001 -> v0002.
        write_history(
            dir.path(),
            r#"[{"global_step":0,"accepted":false},{"global_step":1,"accepted":true}]"#,
        );
        assert_eq!(
            resolve_step_versions(dir.path(), 1),
            StepVersions::Accepted {
                before: 0,
                after: 1
            }
        );
        assert_eq!(resolve_step_versions(dir.path(), 0), StepVersions::Rejected);
        assert_eq!(resolve_step_versions(dir.path(), 7), StepVersions::Unknown);
    }

    #[test]
    fn resolve_step_versions_is_unknown_without_history() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        assert_eq!(resolve_step_versions(dir.path(), 0), StepVersions::Unknown);
        write_history(dir.path(), "not json");
        assert_eq!(resolve_step_versions(dir.path(), 0), StepVersions::Unknown);
    }

    /// `append_history` never resets on resume, so a crash between the history append
    /// and the runtime-state save can leave two records for one `global_step`. The
    /// later record is the real outcome, and the duplicate must not inflate the
    /// accepted-step count used to locate versions.
    #[test]
    fn resolve_step_versions_takes_last_record_for_duplicated_step() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        write_history(
            dir.path(),
            r#"[{"global_step":0,"accepted":false},
                {"global_step":0,"accepted":true},
                {"global_step":1,"accepted":true}]"#,
        );
        // Step 0's retry accepted, so it is no longer Rejected...
        assert_eq!(
            resolve_step_versions(dir.path(), 0),
            StepVersions::Accepted {
                before: 0,
                after: 1
            }
        );
        // ...and step 1 sees exactly one accepted step before it, not two.
        assert_eq!(
            resolve_step_versions(dir.path(), 1),
            StepVersions::Accepted {
                before: 1,
                after: 2
            }
        );
    }

    /// Records are keyed by `global_step`, so file order does not matter.
    #[test]
    fn resolve_step_versions_ignores_record_order() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        write_history(
            dir.path(),
            r#"[{"global_step":2,"accepted":true},
                {"global_step":0,"accepted":true},
                {"global_step":1,"accepted":false}]"#,
        );
        assert_eq!(
            resolve_step_versions(dir.path(), 2),
            StepVersions::Accepted {
                before: 1,
                after: 2
            }
        );
    }

    #[test]
    fn completion_output_zero_steps_warns_and_uses_distinct_stdout_line() {
        let path = Path::new("/tmp/out/best_skill.md");
        let (stdout_line, warning) = completion_output(0, path);
        assert_ne!(
            stdout_line, "Run complete. Best skill: /tmp/out/best_skill.md",
            "a zero-step run must not print the same success-shaped line as a real run"
        );
        let warning = warning.expect("zero-step run must emit a stderr warning");
        assert!(warning.starts_with("OPTIMIZE_ZERO_STEPS_WARN:"));
    }

    #[test]
    fn completion_output_nonzero_steps_prints_documented_line_with_no_warning() {
        let path = Path::new("/tmp/out/best_skill.md");
        let (stdout_line, warning) = completion_output(3, path);
        assert_eq!(
            stdout_line,
            "Run complete. Best skill: /tmp/out/best_skill.md"
        );
        assert!(warning.is_none());
    }
}
