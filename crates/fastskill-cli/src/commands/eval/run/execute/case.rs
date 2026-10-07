//! One eval case: its trials run in parallel, then fold into a [`CaseSummary`].
//!
//! Split from [`super`] so neither the per-case work nor the run that drives it
//! outgrows the repo's per-file line cap.

use crate::error::{CliError, CliResult};
use fastskill_evals::artifacts::CaseResult;
use fastskill_evals::artifacts::{
    aggregate_trials, write_case_trials_summary, write_trial_artifacts, CaseSummary,
    IsolationReport, TrialArtifacts, TrialResult,
};
use fastskill_evals::runner::{CaseRunOptions, CaseRunOutput, EvalRunner};
use fastskill_evals::{CheckDefinition, EvalCase};
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

type TrialOutcome = (u32, CaseRunOutput, CaseResult, String);

/// Everything the cases of one agent's run share.
pub(super) struct CaseRun<'a> {
    pub runner: &'a Arc<dyn EvalRunner>,
    pub opts: &'a CaseRunOptions,
    pub checks: &'a [CheckDefinition],
    pub run_dir: &'a Path,
    pub trials_per_case: u32,
    pub pass_threshold: f64,
    pub parallel: Option<u32>,
    pub use_json: bool,
}

/// Usage summed over a case's trials. Each total stays `None` until at least
/// one trial reported that measure, so "not measured" never reads as zero.
#[derive(Default)]
struct UsageTotals {
    command_count: Option<usize>,
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
}

impl UsageTotals {
    fn add(&mut self, trial: &TrialResult) {
        if let Some(cc) = trial.command_count {
            self.command_count = Some(self.command_count.unwrap_or(0).saturating_add(cc));
        }
        if let Some(it) = trial.input_tokens {
            self.input_tokens = Some(self.input_tokens.unwrap_or(0).saturating_add(it));
        }
        if let Some(ot) = trial.output_tokens {
            self.output_tokens = Some(self.output_tokens.unwrap_or(0).saturating_add(ot));
        }
    }
}

impl CaseRun<'_> {
    /// Run every trial of `case`, write its artifacts and fold the trials into
    /// one summary. Also returns the first isolation report a trial produced.
    pub(super) async fn run(
        &self,
        case: &EvalCase,
    ) -> CliResult<(CaseSummary, Option<IsolationReport>)> {
        let mut join_set = self.spawn_trials(case);
        let mut trials: Vec<TrialResult> = Vec::with_capacity(self.trials_per_case as usize);
        let mut usage = UsageTotals::default();
        let mut isolation: Option<IsolationReport> = None;

        while let Some(joined) = join_set.join_next().await {
            let (trial_id, out, case_result, trace_jsonl) = joined.map_err(|e| {
                CliError::Config(format!(
                    "EVAL_PARALLEL_EXHAUSTION: trial task failed: {}",
                    e
                ))
            })??;

            if isolation.is_none() {
                isolation = out.isolation.clone();
            }
            let trial = trial_result(trial_id, &case_result);
            usage.add(&trial);
            self.write_trial(case, trial_id, &out, &trace_jsonl, &trial);
            trials.push(trial);
        }

        Ok((self.summarize(case, trials, usage), isolation))
    }

    fn spawn_trials(&self, case: &EvalCase) -> JoinSet<CliResult<TrialOutcome>> {
        let max_parallel = self
            .parallel
            .unwrap_or_else(|| num_cpus::get().max(1) as u32)
            .max(1) as usize;
        let semaphore = Arc::new(Semaphore::new(max_parallel));
        let mut join_set = JoinSet::new();

        for trial_id in 1..=self.trials_per_case {
            let permit = Arc::clone(&semaphore);
            let runner = Arc::clone(self.runner);
            let case_clone = case.clone();
            let opts_clone = self.opts.clone();
            let checks_vec = self.checks.to_vec();

            join_set.spawn(async move {
                let Ok(_permit) = permit.acquire().await else {
                    return Err(CliError::Config(
                        "EVAL_PARALLEL_EXHAUSTION: semaphore closed".to_string(),
                    ));
                };
                let (out, res, trace) =
                    runner.run_case(&case_clone, &opts_clone, &checks_vec).await;
                Ok((trial_id, out, res, trace))
            });
        }
        join_set
    }

    fn write_trial(
        &self,
        case: &EvalCase,
        trial_id: u32,
        out: &CaseRunOutput,
        trace_jsonl: &str,
        trial: &TrialResult,
    ) {
        let written = write_trial_artifacts(
            self.run_dir,
            &case.id,
            trial_id,
            &TrialArtifacts {
                stdout: &out.stdout,
                stderr: &out.stderr,
                trace_jsonl,
                // `None` when the trial had no seeded workspace to diff
                // against, and then no `workspace.diff` is written at
                // all: a judge must see "no evidence", never an empty
                // diff claiming nothing changed.
                workspace_diff: out.workspace_diff.as_deref(),
                result: trial,
            },
        );
        if let Err(e) = written {
            if !self.use_json {
                eprintln!(
                    "  warning: failed to write artifacts for case '{}' trial {}: {}",
                    case.id, trial_id, e
                );
            }
        }
    }

    fn summarize(
        &self,
        case: &EvalCase,
        trials: Vec<TrialResult>,
        usage: UsageTotals,
    ) -> CaseSummary {
        // R4: one fold, shared with the engine. Errored trials leave the
        // ratio entirely, and a case with none left scores `error` rather
        // than a 0% fail. Re-deriving the rate here would let the CLI and
        // the engine disagree about the same run.
        let aggregated =
            aggregate_trials(&case.id, trials, self.trials_per_case, self.pass_threshold);

        if let Err(e) = write_case_trials_summary(self.run_dir, &case.id, &aggregated) {
            if !self.use_json {
                eprintln!(
                    "  warning: failed to write aggregated summary for case '{}': {}",
                    case.id, e
                );
            }
        }

        CaseSummary {
            id: case.id.clone(),
            status: aggregated.aggregated_status.clone(),
            command_count: usage.command_count,
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
            pass_count: Some(aggregated.pass_count),
            total_trials: Some(aggregated.total_trials),
            pass_rate: Some(aggregated.pass_rate),
            error_count: Some(aggregated.error_count),
            scored_trials: Some(aggregated.scored_trials),
            // Recorded so `eval score` can rebuild the same effective check
            // list offline: under R7 this column generates an implicit
            // skill-invocation check, and a scorer that cannot see it drops
            // that check and reports a different verdict than the run.
            should_trigger: Some(case.should_trigger),
            judge_excluded_count: Some(aggregated.judge_excluded_count),
            scores: aggregated.scores.clone(),
            trials: aggregated.trials.clone(),
        }
    }
}

fn trial_result(trial_id: u32, case_result: &CaseResult) -> TrialResult {
    TrialResult {
        trial_id,
        status: case_result.status.clone(),
        command_count: case_result.command_count,
        input_tokens: case_result.input_tokens,
        output_tokens: case_result.output_tokens,
        check_results: case_result.check_results.clone(),
        error_message: case_result.error_message.clone(),
        // R5: every field the runner already held and the artifact
        // previously narrowed away. Copied verbatim — `eval run` is
        // the writer, and a writer that reshapes what the runner
        // measured is a second source of truth.
        exit_code: case_result.exit_code,
        terminal: case_result.terminal.clone(),
        cost_usd: case_result.cost_usd,
        tokens: case_result.tokens.clone(),
        skill_path: case_result.skill_path.clone(),
        // Set by `eval judge`, never by the runner: no judge has
        // seen this trial yet.
        judge_excluded: false,
    }
}
