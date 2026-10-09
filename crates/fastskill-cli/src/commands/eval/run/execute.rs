//! Eval run subcommand - the run itself.
//!
//! Split from the argument surface in [`super`] only because the two together
//! outgrow the repo's per-file line cap; the seam is "describe the command"
//! versus "carry it out".

use crate::commands::common::{runtime_selection_error_to_cli, validate_eval_format_args};
use crate::error::{CliError, CliResult};
use crate::runtime_selector::RuntimeSelectionInput;
use aikit_sdk::is_agent_available;
use chrono::Utc;
use fastskill_core::core::project::resolve_project_file;
use fastskill_core::OutputFormat;
use fastskill_evals::artifacts::{
    allocate_run_dir, read_summary, skill_git_identity, write_summary, CaseStatus, CaseSummary,
    IsolationReport, SkillGitIdentity, SummaryResult,
};
use fastskill_evals::checks::load_checks;
use fastskill_evals::config::EvalConfig;
use fastskill_evals::judge::{JudgeRunOptions, SuitePassRule};
use fastskill_evals::resolve_eval_config;
use fastskill_evals::runner::{AikitEvalRunner, CaseRunOptions, EvalRunner, IsolationMode};
use fastskill_evals::suite::{load_suite, EvalSuite};
use fastskill_evals::CheckDefinition;

use crate::commands::eval::isolation::{render_isolation_line, resolve_isolation_mode};
use crate::commands::eval::observability::scoreable_runtimes;
use std::env;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::RunArgs;

mod case;

use case::CaseRun;

/// Pass rate over every case in a run.
fn case_rate(passed: usize, total_cases: usize) -> f64 {
    if total_cases == 0 {
        0.0
    } else {
        passed as f64 / total_cases as f64
    }
}

/// The verdict `eval run` reports and exits on.
///
/// Without `--judge` this is exactly the rule the run has always applied. With
/// it, `summary.suite_pass` has been rewritten by the engine, which asks a
/// deliberately narrower question — every *scored* case must pass, so a case
/// the agent never completed is outside its scope. `eval run` still fails on
/// such a case: adding `--judge` must not turn a red run green.
fn run_verdict(summary: &SummaryResult, ci: bool, pass_threshold: f64) -> bool {
    if ci {
        case_rate(summary.passed, summary.total_cases) >= pass_threshold
    } else {
        summary.failed == 0
    }
}

/// Execute the `eval run` command using the default aikit-backed runner.
pub async fn execute_run(args: RunArgs) -> CliResult<()> {
    execute_run_with_runner(args, Arc::new(AikitEvalRunner::new())).await
}

/// Execute `eval run` with an injectable [`EvalRunner`] (tests or future adapters).
pub async fn execute_run_with_runner<R: EvalRunner + 'static>(
    args: RunArgs,
    runner: Arc<R>,
) -> CliResult<()> {
    execute_run_with_shared_runner(args, runner).await
}

/// Everything settled before the first trial: what to run, where, and against
/// which thresholds. The same for every runtime in the invocation.
struct RunPlan {
    use_json: bool,
    runtimes: Vec<String>,
    project_root: PathBuf,
    skill_identity: Option<SkillGitIdentity>,
    eval_config: EvalConfig,
    isolation: IsolationMode,
    trials_per_case: u32,
    pass_threshold: f64,
    suite: EvalSuite,
    checks: Vec<CheckDefinition>,
    run_dir_base: PathBuf,
}

async fn execute_run_with_shared_runner(
    args: RunArgs,
    runner: Arc<dyn EvalRunner>,
) -> CliResult<()> {
    let plan = prepare_run(&args)?;

    let mut all_summaries: Vec<SummaryResult> = Vec::new();
    let mut any_agent_failed = false;
    let mut judge_errors: u32 = 0;

    for agent_key in &plan.runtimes {
        let (summary, agent_judge_errors) = run_agent(&plan, &args, &runner, agent_key).await?;
        judge_errors += agent_judge_errors;
        if !run_verdict(&summary, args.ci, plan.pass_threshold) {
            any_agent_failed = true;
        }
        all_summaries.push(summary);
    }

    print_results(&all_summaries, &args, &plan);
    final_verdict(&all_summaries, &args, &plan, judge_errors, any_agent_failed)
}

fn prepare_run(args: &RunArgs) -> CliResult<RunPlan> {
    let format = validate_eval_format_args(&args.format, args.json)?;
    let use_json = format == OutputFormat::Json;

    // Resolve runtime selection first so missing --agent is caught before project-file checks.
    let runtimes = select_runtimes(args)?;

    let current_dir = env::current_dir()
        .map_err(|e| CliError::Config(format!("Failed to get current directory: {}", e)))?;

    let resolution = resolve_project_file(&current_dir);
    if !resolution.found {
        return Err(CliError::Config(
            "EVAL_CONFIG_MISSING: No skill-project.toml found. Run 'fastskill project init' first."
                .to_string(),
        ));
    }

    let project_root = resolution
        .path
        .parent()
        .unwrap_or(&current_dir)
        .to_path_buf();

    // Asked once, before anything runs: a later answer would describe a
    // checkout that has moved on. `None` when the project is not in git, never
    // a guess (scorecard R2).
    let skill_identity = skill_git_identity(&project_root);

    let eval_config = resolve_eval_config(&resolution.path, &project_root)
        .map_err(|e| CliError::Config(e.to_string()))?;

    let isolation = resolve_isolation_mode(args.no_isolation, &resolution.path, &project_root)?;

    let trials_per_case = validate_trials(args.trials, &eval_config)?;
    let pass_threshold = validate_threshold(args.threshold, &eval_config)?;

    let suite = load_filtered_suite(args, &eval_config)?;

    // Load checks if configured.
    let checks = match eval_config.checks_path {
        Some(ref checks_path) => {
            load_checks(checks_path).map_err(|e| CliError::Config(e.to_string()))?
        }
        None => vec![],
    };

    // R10: a required check whose evidence a backend never emits makes the
    // suite unscoreable there. Ask before the first trial — every one after
    // this point costs a provider call, and none of them would produce a score.
    // Runtimes that cannot score are dropped rather than failing the whole
    // invocation, so `--all` is not held hostage by one text-only decoder;
    // naming a backend with `--agent` leaves nothing to fall back to and fails.
    let (runtimes, exclusion_notice) = scoreable_runtimes(
        &runtimes,
        &suite.cases,
        &checks,
        matches!(isolation, IsolationMode::Isolated { .. }),
    )?;
    if let Some(notice) = exclusion_notice {
        // stderr even under --json: the machine-readable summary covers the
        // runtimes that ran, and the ones that did not must not vanish.
        eprintln!("{}", notice);
    }

    if !use_json {
        warn_on_cost(suite.cases.len(), trials_per_case, runtimes.len());
    }

    let run_dir_base = allocate_run_base(&args.output_dir)?;

    Ok(RunPlan {
        use_json,
        runtimes,
        project_root,
        skill_identity,
        eval_config,
        isolation,
        trials_per_case,
        pass_threshold,
        suite,
        checks,
        run_dir_base,
    })
}

fn select_runtimes(args: &RunArgs) -> CliResult<Vec<String>> {
    let input = RuntimeSelectionInput::from(args);
    let selection = crate::runtime_selector::resolve_runtime_selection(&input)
        .map_err(runtime_selection_error_to_cli)?;

    match selection {
        Some(sel) => Ok(sel.runtimes),
        None => Err(CliError::Config(
            "RUNTIME_NO_SELECTION: No runtime selected. Use --agent <id> or --all to \
             specify a target runtime."
                .to_string(),
        )),
    }
}

fn validate_trials(trials: Option<i64>, eval_config: &EvalConfig) -> CliResult<u32> {
    // Validate against the raw parsed value so the error echoes exactly what the
    // user typed (e.g. a negative `-3`), not a wrapped/clamped integer.
    let trials_raw = trials.unwrap_or(i64::from(eval_config.trials_per_case));
    if !(1..=1000).contains(&trials_raw) {
        return Err(CliError::Config(format!(
            "EVAL_INVALID_TRIALS_CONFIG: trials must be in range [1, 1000], got {}",
            trials_raw
        )));
    }
    // Safe: validated to be within [1, 1000] above.
    Ok(trials_raw as u32)
}

fn validate_threshold(threshold: Option<f64>, eval_config: &EvalConfig) -> CliResult<f64> {
    let pass_threshold = threshold.unwrap_or(eval_config.pass_threshold);
    if !(0.0..=1.0).contains(&pass_threshold) {
        return Err(CliError::Config(format!(
            "EVAL_INVALID_THRESHOLD: threshold must be in range [0.0, 1.0], got {}",
            pass_threshold
        )));
    }
    Ok(pass_threshold)
}

/// Load the suite and apply `--case` / `--tag` (same for all runtimes).
fn load_filtered_suite(args: &RunArgs, eval_config: &EvalConfig) -> CliResult<EvalSuite> {
    let mut suite =
        load_suite(&eval_config.prompts_path).map_err(|e| CliError::Config(e.to_string()))?;

    // Reject a suite that parsed to zero cases before any filtering (the
    // --case/--tag filters below already guard their own empty results). The
    // default verdict is `failed == 0`, so an empty suite — a header-only CSV
    // or a wrong prompts path pointing at a template — would otherwise report
    // `0/0 passed · PASSED` and exit 0, green-lighting CI while running
    // nothing at all.
    if suite.cases.is_empty() {
        return Err(CliError::Config(format!(
            "EVAL_EMPTY_SUITE: suite '{}' contains zero cases",
            eval_config.prompts_path.display()
        )));
    }

    if let Some(ref case_id) = args.case {
        suite = suite.filter_by_id(case_id);
        if suite.cases.is_empty() {
            return Err(CliError::Config(format!(
                "No case found with id '{}'",
                case_id
            )));
        }
    }
    if let Some(ref tag) = args.tag {
        suite = suite.filter_by_tag(tag);
        if suite.cases.is_empty() {
            return Err(CliError::Config(format!(
                "No cases found with tag '{}'",
                tag
            )));
        }
    }
    Ok(suite)
}

fn warn_on_cost(cases: usize, trials_per_case: u32, agents: usize) {
    let total_trial_runs = (cases as u64) * (trials_per_case as u64) * (agents as u64);
    if total_trial_runs >= 100 {
        eprintln!(
            "warning: EVAL_COST_WARNING: running {} case(s) × {} trial(s) × {} agent(s) = {} total trial runs",
            cases, trials_per_case, agents, total_trial_runs
        );
    }
}

/// Allocate the run base directory under `output_dir`.
fn allocate_run_base(output_dir: &Path) -> CliResult<PathBuf> {
    let run_id = Utc::now().format("%Y-%m-%dT%H-%M-%SZ").to_string();
    std::fs::create_dir_all(output_dir).map_err(|e| {
        CliError::Config(format!(
            "Failed to create output directory '{}': {}",
            output_dir.display(),
            e
        ))
    })?;
    allocate_run_dir(output_dir, &run_id).map_err(|e| CliError::Config(e.to_string()))
}

/// Run every case for one agent, write its summary and, under `--judge`, judge
/// it. Returns the summary to report and the number of failed judgments.
async fn run_agent(
    plan: &RunPlan,
    args: &RunArgs,
    runner: &Arc<dyn EvalRunner>,
    agent_key: &str,
) -> CliResult<(SummaryResult, u32)> {
    // Per-agent subdirectory.
    let run_dir = plan.run_dir_base.join(agent_key);
    std::fs::create_dir_all(&run_dir).map_err(|e| {
        CliError::Config(format!(
            "Failed to create run directory '{}': {}",
            run_dir.display(),
            e
        ))
    })?;

    // Check agent availability.
    if plan.eval_config.fail_on_missing_agent && !is_agent_available(agent_key) {
        return Err(CliError::Config(format!(
            "EVAL_AGENT_UNAVAILABLE: Agent '{}' is not available. Install it first.",
            agent_key
        )));
    }

    let run_opts = CaseRunOptions {
        agent_key: agent_key.to_string(),
        model: args.model.clone(),
        project_root: plan.project_root.clone(),
        timeout_seconds: plan.eval_config.timeout_seconds,
        pass_threshold: plan.pass_threshold,
        isolation: plan.isolation.clone(),
        // A failed case's scratch workspace is moved here so it survives
        // for debugging; successful workspaces are deleted.
        retain_workspace_in: Some(run_dir.join("workspaces")),
        // The harness capture isn't offered from the CLI yet.
        capture_harness: false,
    };

    if !plan.use_json {
        eprintln!(
            "Running {} eval case(s) with agent '{}' ({} trial(s) per case)...",
            plan.suite.cases.len(),
            agent_key,
            plan.trials_per_case
        );
    }

    let case_run = CaseRun {
        runner,
        opts: &run_opts,
        checks: &plan.checks,
        run_dir: &run_dir,
        trials_per_case: plan.trials_per_case,
        pass_threshold: plan.pass_threshold,
        parallel: plan.eval_config.parallel,
        use_json: plan.use_json,
    };

    let mut case_summaries = Vec::new();
    // First observed per-case isolation report stands in for the run: the
    // backend and mechanism are constant across an agent's run, and a
    // per-case copy lives in each trial's artifacts.
    let mut run_isolation: Option<IsolationReport> = None;

    for case in &plan.suite.cases {
        if !plan.use_json {
            eprintln!("  Running case '{}'...", case.id);
        }
        let (case_summary, isolation) = case_run.run(case).await?;
        if run_isolation.is_none() {
            run_isolation = isolation;
        }
        case_summaries.push(case_summary);
    }

    let summary = build_summary(
        plan,
        args,
        agent_key,
        &run_dir,
        run_isolation,
        case_summaries,
    );

    if let Err(e) = write_summary(&run_dir, &summary) {
        if !plan.use_json {
            eprintln!("warning: failed to write summary.json: {}", e);
        }
    }

    if args.judge {
        judge_agent(plan, args, agent_key, &run_dir).await
    } else {
        Ok((summary, 0))
    }
}

fn build_summary(
    plan: &RunPlan,
    args: &RunArgs,
    agent_key: &str,
    run_dir: &Path,
    run_isolation: Option<IsolationReport>,
    case_summaries: Vec<CaseSummary>,
) -> SummaryResult {
    let passed = case_summaries
        .iter()
        .filter(|r| r.status == CaseStatus::Passed)
        .count();
    let failed = case_summaries.len() - passed;
    let suite_pass_rate = case_rate(passed, case_summaries.len());
    let suite_pass = if args.ci {
        suite_pass_rate >= plan.pass_threshold
    } else {
        failed == 0
    };

    SummaryResult {
        suite_pass,
        suite_pass_rate: Some(suite_pass_rate),
        agent: agent_key.to_string(),
        model: args.model.clone(),
        total_cases: case_summaries.len(),
        passed,
        failed,
        trials_per_case: Some(plan.trials_per_case),
        parallel: plan.eval_config.parallel,
        pass_threshold: Some(plan.pass_threshold),
        run_dir: run_dir.to_path_buf(),
        checks_path: plan.eval_config.checks_path.clone(),
        skill_project_root: plan.project_root.clone(),
        isolation: run_isolation,
        // Judge totals belong to `eval judge`, which rewrites them into
        // this file after it has judged. The runner reports no judgment.
        judge_errors: None,
        judge_skipped_trials: None,
        judge_tokens: None,
        judge_cost_usd: None,
        // Recorded now, at run time: the skill on disk when a scorecard is
        // built later is not the skill that ran (scorecard R2).
        skill_git_sha: plan.skill_identity.as_ref().map(|i| i.sha.clone()),
        skill_dirty: plan.skill_identity.as_ref().map(|i| i.dirty),
        cases: case_summaries,
    }
}

/// R13: the same judging function `eval judge` calls, run right after this
/// agent's own scoring. It rewrites the run's artifacts in place, so the
/// summary reported from here on is re-read from the file the judge left
/// rather than the one held in memory.
async fn judge_agent(
    plan: &RunPlan,
    args: &RunArgs,
    agent_key: &str,
    run_dir: &Path,
) -> CliResult<(SummaryResult, u32)> {
    let opts = JudgeRunOptions {
        judge_model: args.judge_model.clone(),
        parallel: plan.eval_config.parallel,
        suite_rule: if args.ci {
            SuitePassRule::RateAtLeast(plan.pass_threshold)
        } else {
            SuitePassRule::AllCases
        },
        ..Default::default()
    };
    let report = crate::commands::eval::judge::judge_run(run_dir, &plan.suite, &opts).await?;
    if !plan.use_json {
        if report.judges.is_empty() {
            eprintln!("  no [[judge]] declared in the checks file; nothing was judged");
        } else {
            eprintln!("  judged agent '{}'", agent_key);
            crate::commands::eval::judge::render_report(&report);
        }
    }
    let summary = read_summary(run_dir).map_err(|e| {
        CliError::Config(format!(
            "EVAL_ARTIFACTS_CORRUPT: failed to re-read summary.json after judging: {}",
            e
        ))
    })?;
    Ok((summary, report.errors))
}

fn print_results(all_summaries: &[SummaryResult], args: &RunArgs, plan: &RunPlan) {
    if !plan.use_json {
        for summary in all_summaries {
            print_agent_result(summary, args.ci, plan.pass_threshold);
        }
    } else if all_summaries.len() == 1 {
        crate::outln!(
            "{}",
            serde_json::to_string_pretty(&all_summaries[0]).unwrap_or_default()
        );
    } else {
        crate::outln!(
            "{}",
            serde_json::to_string_pretty(&all_summaries).unwrap_or_default()
        );
    }
}

fn print_agent_result(summary: &SummaryResult, ci: bool, pass_threshold: f64) {
    crate::outln!(
        "\nEval run complete for agent '{}': {}/{} passed",
        summary.agent,
        summary.passed,
        summary.total_cases
    );
    crate::outln!("  run_dir: {}", summary.run_dir.display());
    crate::outln!("  {}", render_isolation_line(summary.isolation.as_ref()));
    if let Some(iso) = &summary.isolation {
        if !iso.ambient_skills.is_empty() {
            crate::outln!(
                "  ambient skills visible to agent: {}",
                iso.ambient_skills.join(", ")
            );
        }
    }
    crate::outln!("  result: {}", result_line(summary, ci, pass_threshold));
}

/// The `result:` line for one agent.
fn result_line(summary: &SummaryResult, ci: bool, pass_threshold: f64) -> String {
    // Over every case in the run, which is the question `run_verdict` asks.
    // After judging `summary.suite_pass_rate` answers a narrower one (scored
    // cases only); the two must not be reported as one number.
    let suite_pass_rate = case_rate(summary.passed, summary.total_cases);
    let verdict = run_verdict(summary, ci, pass_threshold);
    match (verdict, ci) {
        (true, true) => format!(
            "PASSED (suite pass rate {:.0}% ≥ {:.0}% threshold)",
            suite_pass_rate * 100.0,
            pass_threshold * 100.0
        ),
        (true, false) => "PASSED".to_string(),
        (false, true) => format!(
            "FAILED (suite pass rate {:.0}% < {:.0}% threshold)",
            suite_pass_rate * 100.0,
            pass_threshold * 100.0
        ),
        (false, false) => format!("FAILED ({} case(s) failed)", summary.failed),
    }
}

fn final_verdict(
    all_summaries: &[SummaryResult],
    args: &RunArgs,
    plan: &RunPlan,
    judge_errors: u32,
    any_agent_failed: bool,
) -> CliResult<()> {
    // R13: a judge that could not render a judgment left a gap in the
    // measurement. `--no-fail` suppresses a failing verdict, never a missing
    // one — reporting an outage as a score is the one thing this must not do.
    if judge_errors > 0 {
        return Err(CliError::Config(format!(
            "EVAL_JUDGE_ERRORS: {} judgment(s) failed; see judgments.json under {} for the \
             recorded attempts",
            judge_errors,
            plan.run_dir_base.display()
        )));
    }

    if any_agent_failed && !args.no_fail {
        let total_passed: usize = all_summaries.iter().map(|s| s.passed).sum();
        let total_cases: usize = all_summaries.iter().map(|s| s.total_cases).sum();
        return Err(CliError::Config(format!(
            "Eval suite failed: {}/{} cases passed across {} agent(s) (threshold={})",
            total_passed,
            total_cases,
            all_summaries.len(),
            plan.pass_threshold
        )));
    }

    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod test_support;

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::await_holding_lock)]
mod tests {
    use super::test_support::{run_args, scaffold_project, CwdGuard, ScriptedRunner};
    use super::*;
    use std::path::PathBuf;
    use tempfile::TempDir;

    /// A summary as the engine leaves it after judging: one case passed, one
    /// never produced a score, and `suite_pass` is the engine's narrower
    /// question — every *scored* case passed, so it says true.
    fn judged_summary() -> SummaryResult {
        SummaryResult {
            suite_pass: true,
            suite_pass_rate: Some(1.0),
            agent: "aikit".to_string(),
            model: None,
            total_cases: 2,
            passed: 1,
            failed: 1,
            trials_per_case: Some(1),
            parallel: None,
            pass_threshold: Some(0.5),
            run_dir: PathBuf::from("run"),
            checks_path: None,
            skill_project_root: PathBuf::from("."),
            isolation: None,
            judge_errors: Some(0),
            judge_skipped_trials: Some(0),
            judge_tokens: None,
            judge_cost_usd: None,
            skill_git_sha: None,
            skill_dirty: None,
            cases: vec![],
        }
    }

    /// spec eval-judge R13: `--judge` must not turn a red run green. A case
    /// the agent never completed is outside the engine's judged verdict, and
    /// `eval run` has always failed on one.
    #[test]
    fn test_run_verdict_fails_on_an_unscored_case_the_engine_left_out() {
        let summary = judged_summary();
        assert!(
            !run_verdict(&summary, false, 0.5),
            "an unscored case must still fail the run, whatever suite_pass says"
        );
        // Under --ci the rate is over every case, so 1 of 2 is 50%.
        assert!(run_verdict(&summary, true, 0.5));
        assert!(!run_verdict(&summary, true, 0.75));
    }

    #[test]
    fn test_run_verdict_passes_when_every_case_passed() {
        let mut summary = judged_summary();
        summary.passed = 2;
        summary.failed = 0;
        assert!(run_verdict(&summary, false, 0.5));
        assert!(run_verdict(&summary, true, 1.0));
        assert_eq!(case_rate(0, 0), 0.0);
    }

    #[tokio::test]
    async fn early_errors_are_specific_and_use_canonical_init_hint() {
        let _lock = fastskill_core::test_utils::DIR_MUTEX
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let temp = TempDir::new().unwrap();
        let empty = temp.path().join("empty");
        std::fs::create_dir(&empty).unwrap();
        let cwd = CwdGuard::enter(&empty);
        let runner = Arc::new(ScriptedRunner {
            status: CaseStatus::Passed,
            report_isolation: true,
            block_artifact_directory: false,
        });

        let mut no_selection = run_args(temp.path().join("no-selection"));
        no_selection.agent.clear();
        assert!(execute_run_with_runner(no_selection, Arc::clone(&runner))
            .await
            .unwrap_err()
            .to_string()
            .contains("RUNTIME_NO_SELECTION"));
        assert!(execute_run_with_runner(
            run_args(temp.path().join("missing-project")),
            Arc::clone(&runner),
        )
        .await
        .unwrap_err()
        .to_string()
        .contains("Run 'fastskill project init' first"));
        drop(cwd);

        let project = temp.path().join("demo-project");
        std::fs::create_dir(&project).unwrap();
        scaffold_project(&project);
        let _cwd = CwdGuard::enter(&project);

        let mut invalid_format = run_args(temp.path().join("invalid-format"));
        invalid_format.format = Some(OutputFormat::Grid);
        assert!(execute_run_with_runner(invalid_format, Arc::clone(&runner))
            .await
            .is_err());

        let mut invalid_trials = run_args(temp.path().join("invalid-trials"));
        invalid_trials.trials = Some(0);
        assert!(execute_run_with_runner(invalid_trials, Arc::clone(&runner))
            .await
            .unwrap_err()
            .to_string()
            .contains("EVAL_INVALID_TRIALS_CONFIG"));

        let mut invalid_threshold = run_args(temp.path().join("invalid-threshold"));
        invalid_threshold.threshold = Some(1.1);
        assert!(
            execute_run_with_runner(invalid_threshold, Arc::clone(&runner))
                .await
                .unwrap_err()
                .to_string()
                .contains("EVAL_INVALID_THRESHOLD")
        );

        let mut missing_case = run_args(temp.path().join("missing-case"));
        missing_case.case = Some("unknown".to_string());
        assert!(execute_run_with_runner(missing_case, Arc::clone(&runner))
            .await
            .unwrap_err()
            .to_string()
            .contains("No case found"));

        let mut missing_tag = run_args(temp.path().join("missing-tag"));
        missing_tag.tag = Some("unknown".to_string());
        assert!(execute_run_with_runner(missing_tag, Arc::clone(&runner))
            .await
            .unwrap_err()
            .to_string()
            .contains("No cases found"));

        std::fs::write(
            project.join("evals/prompts.csv"),
            "id,prompt,should_trigger,tags\n",
        )
        .unwrap();
        assert!(execute_run_with_runner(
            run_args(temp.path().join("empty-suite")),
            Arc::clone(&runner),
        )
        .await
        .unwrap_err()
        .to_string()
        .contains("EVAL_EMPTY_SUITE"));

        std::fs::write(
            project.join("evals/prompts.csv"),
            "id,prompt,should_trigger,tags\ncase-1,say hello,true,smoke\n",
        )
        .unwrap();
        std::fs::write(project.join("evals/checks.toml"), "[[check]\n").unwrap();
        assert!(execute_run_with_runner(
            run_args(temp.path().join("bad-checks")),
            Arc::clone(&runner),
        )
        .await
        .is_err());
    }

    #[tokio::test]
    async fn scripted_runs_persist_metrics_and_apply_failure_policy() {
        let _lock = fastskill_core::test_utils::DIR_MUTEX
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let temp = TempDir::new().unwrap();
        let project = temp.path().join("demo-project");
        std::fs::create_dir(&project).unwrap();
        scaffold_project(&project);
        let _cwd = CwdGuard::enter(&project);
        let passing = Arc::new(ScriptedRunner {
            status: CaseStatus::Passed,
            report_isolation: true,
            block_artifact_directory: false,
        });

        let table_dir = temp.path().join("table-pass");
        execute_run_with_runner(run_args(table_dir.clone()), Arc::clone(&passing))
            .await
            .unwrap();
        let summary_path = std::fs::read_dir(table_dir)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path()
            .join("aikit/summary.json");
        let summary: SummaryResult =
            serde_json::from_str(&std::fs::read_to_string(summary_path).unwrap()).unwrap();
        assert_eq!(summary.passed, 1);
        assert_eq!(summary.cases[0].command_count, Some(2));
        assert_eq!(summary.cases[0].input_tokens, Some(3));
        assert_eq!(summary.cases[0].output_tokens, Some(5));
        assert_eq!(summary.isolation.unwrap().ambient_skills, ["ambient-demo"]);

        let mut ci_pass = run_args(temp.path().join("ci-pass"));
        ci_pass.ci = true;
        ci_pass.threshold = Some(1.0);
        execute_run_with_runner(ci_pass, Arc::clone(&passing))
            .await
            .unwrap();

        let mut json = run_args(temp.path().join("json-output"));
        json.json = true;
        let (result, output) =
            crate::output::capture(execute_run_with_runner(json, Arc::clone(&passing))).await;
        result.unwrap();
        let summary: SummaryResult = serde_json::from_str(&output).unwrap();
        assert_eq!(summary.passed, 1);

        let failing = Arc::new(ScriptedRunner {
            status: CaseStatus::Failed,
            report_isolation: true,
            block_artifact_directory: false,
        });
        let error = execute_run_with_runner(
            run_args(temp.path().join("table-fail")),
            Arc::clone(&failing),
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("Eval suite failed: 0/1"));

        let mut allowed_failure = run_args(temp.path().join("allowed-fail"));
        allowed_failure.no_fail = true;
        execute_run_with_runner(allowed_failure, Arc::clone(&failing))
            .await
            .unwrap();

        let mut ci_failure = run_args(temp.path().join("ci-fail"));
        ci_failure.ci = true;
        ci_failure.threshold = Some(1.0);
        assert!(execute_run_with_runner(ci_failure, failing)
            .await
            .unwrap_err()
            .to_string()
            .contains("Eval suite failed"));

        let no_report = Arc::new(ScriptedRunner {
            status: CaseStatus::Passed,
            report_isolation: false,
            block_artifact_directory: false,
        });
        execute_run_with_runner(run_args(temp.path().join("no-isolation-report")), no_report)
            .await
            .unwrap();

        let broken_artifacts = Arc::new(ScriptedRunner {
            status: CaseStatus::Passed,
            report_isolation: true,
            block_artifact_directory: true,
        });
        execute_run_with_runner(
            run_args(temp.path().join("blocked-artifacts")),
            broken_artifacts,
        )
        .await
        .unwrap();

        let mut judged = run_args(temp.path().join("judged"));
        judged.judge = true;
        execute_run_with_runner(judged, Arc::clone(&passing))
            .await
            .unwrap();

        let blocked_output = temp.path().join("blocked-output");
        std::fs::write(&blocked_output, "file").unwrap();
        assert!(
            execute_run_with_runner(run_args(blocked_output), Arc::clone(&passing))
                .await
                .unwrap_err()
                .to_string()
                .contains("Failed to create output directory")
        );

        let mut many_trials = run_args(temp.path().join("cost-warning"));
        many_trials.trials = Some(100);
        execute_run_with_runner(many_trials, passing).await.unwrap();
    }
}
