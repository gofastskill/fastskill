//! The metrics file: what a benchmark asks, and the identity of the question.
//!
//! A metrics file names the gates (`[[metric]]`) and, since spec
//! eval-scorecard-report R2, the suites they are asked over (`suites`). The
//! second is what makes [`benchmark_sha256`] possible: two scorecards are
//! comparable only when they were produced by the same question, and the hash
//! over the metrics file plus every file its suites select is what says so.

use crate::error::{CliError, CliResult};
use fastskill_evals::checks::load_checks_file;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

/// What a metric measures, and the bar it has to clear.
#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MetricKind {
    /// Passing check results over observed check results, for the named check
    /// types. The rate is over trials, not cases: a case passing three of five
    /// trials contributes 3/5, which is the whole reason for running more than
    /// one trial.
    CheckRate { checks: Vec<String>, min_rate: f64 },
    /// The 95th percentile of tool calls per trial. A ceiling, not a floor.
    ToolCallsP95 { max: usize },
    /// The mean of a judge's `overall` — or of one named criterion — over the
    /// latest judgment per trial (spec eval-scorecard-report R3).
    JudgeScore {
        judges: Vec<String>,
        /// Absent means `overall`.
        #[serde(default)]
        criterion: Option<String>,
        min_score: f64,
    },
}

#[derive(Debug, Deserialize)]
pub struct MetricSpec {
    pub name: String,
    /// Case-id patterns, `*` matching any run of characters. An empty list
    /// means every case, which is the only way to say "all" — omitting the
    /// field entirely does the same.
    #[serde(default)]
    pub cases: Vec<String>,
    #[serde(flatten)]
    pub kind: MetricKind,
}

/// The metrics file as written.
#[derive(Debug, Deserialize)]
pub struct MetricsFile {
    #[serde(rename = "metric", default)]
    pub metrics: Vec<MetricSpec>,
    /// Suite directories, relative to this file. Present makes the benchmark
    /// hashable; absent leaves `benchmark.sha256` null and refuses a progress
    /// comparison, which is the honest answer for a question nobody wrote down.
    #[serde(default)]
    pub suites: Vec<PathBuf>,
}

/// Match a case id against a `*`-wildcard pattern.
///
/// Greedy two-pointer scan with backtracking to the last `*`, which is the
/// standard linear-space algorithm for this grammar. There is no `?` and no
/// character class: case ids are slugs, and a richer grammar would be a
/// dependency and a surprise rather than a feature.
pub fn matches_pattern(pattern: &str, id: &str) -> bool {
    let (p, s): (Vec<char>, Vec<char>) = (pattern.chars().collect(), id.chars().collect());
    let (mut pi, mut si) = (0usize, 0usize);
    let (mut star, mut resume) = (None, 0usize);
    while si < s.len() {
        if pi < p.len() && (p[pi] == s[si]) {
            pi += 1;
            si += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            resume = si;
            pi += 1;
        } else if let Some(star_at) = star {
            pi = star_at + 1;
            resume += 1;
            si = resume;
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|c| *c == '*')
}

impl MetricSpec {
    pub fn covers(&self, case_id: &str) -> bool {
        self.cases.is_empty() || self.cases.iter().any(|p| matches_pattern(p, case_id))
    }
}

fn config_error(message: String) -> CliError {
    CliError::Config(format!("EVAL_SCORECARD_CONFIG: {}", message))
}

pub fn load_metrics(path: &Path) -> CliResult<MetricsFile> {
    let text = std::fs::read_to_string(path).map_err(|e| {
        config_error(format!(
            "cannot read metrics file '{}': {}",
            path.display(),
            e
        ))
    })?;
    let parsed: MetricsFile = toml::from_str(&text).map_err(|e| {
        config_error(format!(
            "cannot parse metrics file '{}': {}",
            path.display(),
            e
        ))
    })?;
    if parsed.metrics.is_empty() {
        return Err(config_error(format!(
            "metrics file '{}' declares no [[metric]] entries",
            path.display()
        )));
    }
    Ok(parsed)
}

/// Every file the benchmark is made of, in path order: the metrics file, and
/// per declared suite its `checks.toml`, its `prompts.csv` and every file a
/// `[[judge]]` in that checks file references.
///
/// A declared suite that is not on disk is an error rather than a skipped
/// entry. Skipping would let two different benchmarks hash the same, which is
/// the one failure this hash exists to prevent.
fn benchmark_files(metrics_path: &Path, suites: &[PathBuf]) -> CliResult<Vec<PathBuf>> {
    let base = metrics_path.parent().unwrap_or_else(|| Path::new("."));
    let mut files = vec![metrics_path.to_path_buf()];
    for suite in suites {
        let dir = base.join(suite);
        if !dir.is_dir() {
            return Err(config_error(format!(
                "metrics file '{}' declares suite '{}', which is not a directory at '{}'",
                metrics_path.display(),
                suite.display(),
                dir.display()
            )));
        }
        for name in ["checks.toml", "prompts.csv"] {
            let path = dir.join(name);
            if !path.is_file() {
                return Err(config_error(format!(
                    "suite '{}' has no {} at '{}'",
                    suite.display(),
                    name,
                    path.display()
                )));
            }
            files.push(path);
        }
        let checks_path = dir.join("checks.toml");
        let checks = load_checks_file(&checks_path)
            .map_err(|e| config_error(format!("cannot read '{}': {}", checks_path.display(), e)))?;
        for judge in &checks.judges {
            let referenced = [
                judge.prompt_file.as_deref(),
                judge.system_prompt_file.as_deref(),
                judge.retry_prompt_file.as_deref(),
            ];
            for relative in referenced.into_iter().flatten() {
                // `[[judge]]` file references are relative to checks.toml.
                let path = dir.join(relative);
                if !path.is_file() {
                    return Err(config_error(format!(
                        "judge '{}' in '{}' references '{}', which is not a file at '{}'",
                        judge.name,
                        checks_path.display(),
                        relative,
                        path.display()
                    )));
                }
                files.push(path);
            }
        }
    }
    files.sort();
    files.dedup();
    Ok(files)
}

/// The sha256 that identifies the question (R2). `None` when the metrics file
/// declares no `suites`: the benchmark then has no recorded extent, and saying
/// so beats hashing half of it.
///
/// Each file contributes its path relative to the metrics file, a NUL, its
/// length and its bytes, so renaming a file changes the hash and no
/// concatenation of two files can collide with a third.
pub fn benchmark_sha256(metrics_path: &Path, suites: &[PathBuf]) -> CliResult<Option<String>> {
    if suites.is_empty() {
        return Ok(None);
    }
    let base = metrics_path.parent().unwrap_or_else(|| Path::new("."));
    let files = benchmark_files(metrics_path, suites)?;
    let mut hasher = Sha256::new();
    for path in &files {
        let bytes = std::fs::read(path)
            .map_err(|e| config_error(format!("cannot read '{}': {}", path.display(), e)))?;
        let relative = path.strip_prefix(base).unwrap_or(path);
        hasher.update(relative.to_string_lossy().replace('\\', "/").as_bytes());
        hasher.update([0u8]);
        hasher.update((bytes.len() as u64).to_le_bytes());
        hasher.update(&bytes);
    }
    Ok(Some(fastskill_core::utils::to_hex_lower(
        &hasher.finalize(),
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wildcards_anchor_at_both_ends() {
        assert!(matches_pattern("op-*", "op-init"));
        assert!(!matches_pattern("op-*", "c-op-init"));
        assert!(matches_pattern("*-init", "op-init"));
        assert!(matches_pattern("op-init", "op-init"));
        assert!(!matches_pattern("op-init", "op-initialize"));
        assert!(matches_pattern("*", "anything"));
        assert!(matches_pattern("a*b*c", "azzbzzc"));
        assert!(!matches_pattern("a*b*c", "azzbzz"));
    }

    #[test]
    fn a_metrics_file_needs_at_least_one_metric() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("metrics.toml");
        std::fs::write(&path, "# nothing here\n").unwrap();
        let err = load_metrics(&path).unwrap_err().to_string();
        assert!(err.contains("EVAL_SCORECARD_CONFIG"), "{err}");
        assert!(err.contains("no [[metric]] entries"), "{err}");
    }

    #[test]
    fn metrics_parse_every_kind() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("metrics.toml");
        std::fs::write(
            &path,
            r#"
suites = ["suites/consultation"]

[[metric]]
name = "Skill-open rate"
kind = "check_rate"
cases = ["op-*"]
checks = ["skill_invoked"]
min_rate = 0.85

[[metric]]
name = "Efficiency"
kind = "tool_calls_p95"
cases = ["op-*"]
max = 25

[[metric]]
name = "Command correctness"
kind = "judge_score"
cases = ["c-*"]
judges = ["command-correctness"]
criterion = "correct_flags"
min_score = 0.8
"#,
        )
        .unwrap();
        let file = load_metrics(&path).expect("parsed");
        assert_eq!(file.metrics.len(), 3);
        assert_eq!(file.suites, vec![PathBuf::from("suites/consultation")]);
        assert!(matches!(file.metrics[0].kind, MetricKind::CheckRate { .. }));
        assert!(matches!(
            file.metrics[1].kind,
            MetricKind::ToolCallsP95 { max: 25 }
        ));
        match &file.metrics[2].kind {
            MetricKind::JudgeScore {
                judges,
                criterion,
                min_score,
            } => {
                assert_eq!(judges, &vec!["command-correctness".to_string()]);
                assert_eq!(criterion.as_deref(), Some("correct_flags"));
                assert_eq!(*min_score, 0.8);
            }
            other => panic!("wrong kind: {other:?}"),
        }
    }

    /// A benchmark file lives in a suite directory; write one.
    fn suite(root: &Path, name: &str, prompt: &str) -> PathBuf {
        let dir = root.join("suites").join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("prompts.csv"), "id,prompt\nc-1,hello\n").unwrap();
        std::fs::write(dir.join("judge-prompt.md"), prompt).unwrap();
        std::fs::write(
            dir.join("checks.toml"),
            r#"
[[judge]]
name = "quality"
prompt_file = "judge-prompt.md"
model = "judge-1"

[[judge.criterion]]
name = "clarity"
kind = "scale"
description = "Is it clear?"
"#,
        )
        .unwrap();
        dir
    }

    fn metrics_with_suites(root: &Path) -> PathBuf {
        let path = root.join("metrics.toml");
        std::fs::write(
            &path,
            r#"
suites = ["suites/consultation"]

[[metric]]
name = "Skill-open rate"
kind = "check_rate"
checks = ["skill_invoked"]
min_rate = 0.85
"#,
        )
        .unwrap();
        path
    }

    #[test]
    fn a_metrics_file_without_suites_has_no_benchmark_hash() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("metrics.toml");
        std::fs::write(
            &path,
            "[[metric]]\nname = \"a\"\nkind = \"check_rate\"\nchecks = [\"c\"]\nmin_rate = 0.5\n",
        )
        .unwrap();
        let file = load_metrics(&path).unwrap();
        assert_eq!(benchmark_sha256(&path, &file.suites).unwrap(), None);
    }

    /// The whole point of the hash: change any file the benchmark selects and
    /// the two scorecards stop being comparable.
    #[test]
    fn the_benchmark_hash_covers_every_file_the_suites_select() {
        let dir = tempfile::tempdir().unwrap();
        suite(dir.path(), "consultation", "Judge this.");
        let path = metrics_with_suites(dir.path());
        let file = load_metrics(&path).unwrap();
        let first = benchmark_sha256(&path, &file.suites).unwrap().unwrap();
        assert_eq!(first.len(), 64, "sha256 hex");
        assert_eq!(
            benchmark_sha256(&path, &file.suites).unwrap().unwrap(),
            first,
            "the same files must hash the same"
        );

        for changed in ["prompts.csv", "checks.toml", "judge-prompt.md"] {
            let target = dir.path().join("suites/consultation").join(changed);
            let before = std::fs::read_to_string(&target).unwrap();
            std::fs::write(&target, format!("{before}\n# edited\n")).unwrap();
            let after = benchmark_sha256(&path, &file.suites).unwrap().unwrap();
            assert_ne!(after, first, "editing {changed} must change the hash");
            std::fs::write(&target, before).unwrap();
        }
        assert_eq!(
            benchmark_sha256(&path, &file.suites).unwrap().unwrap(),
            first,
            "restoring every file must restore the hash"
        );
    }

    #[test]
    fn a_declared_suite_that_is_not_on_disk_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = metrics_with_suites(dir.path());
        let file = load_metrics(&path).unwrap();
        let err = benchmark_sha256(&path, &file.suites)
            .unwrap_err()
            .to_string();
        assert!(err.contains("EVAL_SCORECARD_CONFIG"), "{err}");
        assert!(err.contains("suites/consultation"), "{err}");
    }

    #[test]
    fn a_judge_prompt_file_that_is_not_on_disk_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let suite_dir = suite(dir.path(), "consultation", "Judge this.");
        std::fs::remove_file(suite_dir.join("judge-prompt.md")).unwrap();
        let path = metrics_with_suites(dir.path());
        let file = load_metrics(&path).unwrap();
        let err = benchmark_sha256(&path, &file.suites)
            .unwrap_err()
            .to_string();
        assert!(err.contains("judge-prompt.md"), "{err}");
    }
}
