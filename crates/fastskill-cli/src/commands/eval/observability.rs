//! Pre-flight: refuse a suite the chosen backend cannot score (R10).
//!
//! A check whose evidence the backend's decoder never produces returns a typed
//! *not observable* result rather than a pass or a fail — both would be lies.
//! That keeps a single trial honest, but it does not stop the run: without a
//! gate, `eval run` pays for every trial in the suite and then reports that the
//! thing it was asked to measure was unmeasurable all along.
//!
//! So `eval validate` and `eval run` ask the question up front, off the same
//! [`CheckContext`] the runner will build, and refuse before the first token is
//! spent. `skipped` is deliberately not used for this: a skipped case reads as
//! "we chose not to run it", and this is "we cannot score it".
//!
//! ## Why this partitions instead of aborting
//!
//! `--all` targets every runtime aikit knows about, including ones the operator
//! has never installed and whose decoder they have no control over. Failing the
//! whole invocation because one of those cannot see tool frames would make
//! `--all` permanently red on any suite that checks skill invocation, and a
//! check that is always red is a check nobody reads.
//!
//! So the question is asked for every selected runtime *before any of them
//! runs*, and the answer splits them:
//!
//! - **No runtime can score the suite** — hard error. With a single `--agent`
//!   this is the plain "this backend cannot score this suite" refusal.
//! - **Some can** — the rest are excluded, named on stderr with the reason, and
//!   the scoreable ones run. Nothing is spent on a backend that would produce
//!   no measurement, and nothing is dropped quietly.
//!
//! Naming a backend with `--agent` is therefore how an operator says "I require
//! a score here": that selection has nothing to fall back to, so it fails.

use crate::error::{CliError, CliResult};
use aikit_sdk::runner::Backend;
use fastskill_evals::checks::{
    effective_checks, unobservable_required, validate_case_checks, CheckContext, CheckDefinition,
};
use fastskill_evals::suite::EvalCase;

/// What `fastskill` stages when isolation is on, as the checks engine sees it.
///
/// Only the *presence* of a path decides observability: `skill_invoked` on a
/// backend with no typed `Skill` tool needs something to substring-match, and
/// the real per-trial scratch directory does not exist yet at pre-flight time.
/// Using a stand-in keeps the pre-flight answer identical to the run's without
/// inventing a path that will appear in a message and mislead someone.
const STAGED_SKILL_STANDIN: &str = "<the skill staged for this run>";

/// How many reason lines to print before eliding the rest.
const MAX_REASON_LINES: usize = 10;

/// How many case ids to name when one check is unobservable across many cases.
const MAX_NAMED_CASES: usize = 3;

/// The two decoder facts `CheckContext` needs about a backend.
///
/// An unrecognised agent key stays lenient in both, matching
/// [`CheckContext::default`]: `false` is a positive claim that the decoder
/// cannot produce the evidence, and "we do not know this backend" is not that
/// claim.
fn decoder_facts(agent_key: &str) -> (bool, bool) {
    let backend = Backend::from_key(agent_key);
    let structured_tools = backend
        .map(|b| b.capabilities().structured_tools)
        .unwrap_or(true);
    let typed_skill_tool = match backend {
        Some(b) => b == Backend::Claude,
        None => true,
    };
    (structured_tools, typed_skill_tool)
}

/// Build the context the checks engine will score this backend's traces with.
///
/// `staged_skill_path` is `None` when no skill will be staged — under
/// `--no-isolation` there is no scratch copy, so a path-matching check has
/// nothing to match on and must say so rather than fail every case.
pub(super) fn check_context<'a>(
    agent_key: &'a str,
    staged_skill_path: Option<&'a str>,
) -> CheckContext<'a> {
    let (structured_tools, typed_skill_tool) = decoder_facts(agent_key);
    CheckContext {
        backend: agent_key,
        structured_tools,
        typed_skill_tool,
        skill_path: staged_skill_path,
    }
}

/// The pre-flight context for a run that has not happened yet.
fn preflight_context(agent_key: &str, skill_will_be_staged: bool) -> CheckContext<'_> {
    check_context(
        agent_key,
        skill_will_be_staged.then_some(STAGED_SKILL_STANDIN),
    )
}

/// Reject a suite whose explicit checks contradict a case's `should_trigger`.
///
/// Backend-independent, so it is asked once and always hard-fails: the operator
/// wrote two inputs that disagree, and no choice of runtime resolves that.
pub(super) fn validate_suite_checks(
    cases: &[EvalCase],
    checks: &[CheckDefinition],
) -> CliResult<()> {
    for case in cases {
        validate_case_checks(checks, &case.id, case.should_trigger)
            .map_err(|e| CliError::Config(e.to_string()))?;
    }
    Ok(())
}

/// One check that cannot be observed, and the cases it applies to.
struct Unobservable {
    check_name: String,
    reason: String,
    case_ids: Vec<String>,
}

impl Unobservable {
    fn render(&self) -> String {
        let subject = match self.case_ids.len() {
            1 => format!("case '{}'", self.case_ids[0]),
            n => {
                let named: Vec<&str> = self
                    .case_ids
                    .iter()
                    .take(MAX_NAMED_CASES)
                    .map(String::as_str)
                    .collect();
                let rest = n.saturating_sub(named.len());
                if rest == 0 {
                    format!("{} cases ({})", n, named.join(", "))
                } else {
                    format!("{} cases ({}, +{} more)", n, named.join(", "), rest)
                }
            }
        };
        format!(
            "  check '{}' on {}: {}",
            self.check_name, subject, self.reason
        )
    }
}

/// Every required check this backend cannot produce evidence for, grouped by
/// check so a global check does not repeat once per case.
///
/// Asked per case rather than over the global check list, because
/// `cases = [...]` scopes a check to named cases and `should_trigger` adds one
/// that appears in no file at all. Asking the question globally would miss both.
fn unobservable_for(
    agent_key: &str,
    cases: &[EvalCase],
    checks: &[CheckDefinition],
    skill_will_be_staged: bool,
) -> Vec<Unobservable> {
    let ctx = preflight_context(agent_key, skill_will_be_staged);
    let mut grouped: Vec<Unobservable> = Vec::new();

    for case in cases {
        let case_checks = effective_checks(checks, &case.id, case.should_trigger);
        for (check_name, not_observable) in unobservable_required(&case_checks, &ctx) {
            match grouped
                .iter_mut()
                .find(|g| g.check_name == check_name && g.reason == not_observable.reason)
            {
                Some(existing) => existing.case_ids.push(case.id.clone()),
                None => grouped.push(Unobservable {
                    check_name,
                    reason: not_observable.reason,
                    case_ids: vec![case.id.clone()],
                }),
            }
        }
    }
    grouped
}

/// A runtime that cannot score the suite, with the reasons already rendered.
pub(super) struct ExcludedRuntime {
    pub agent_key: String,
    lines: Vec<String>,
    check_count: usize,
}

/// Which of the selected runtimes can score this suite, and why the rest cannot.
pub(super) struct Scoreability {
    pub scoreable: Vec<String>,
    pub excluded: Vec<ExcludedRuntime>,
}

fn remedy() -> &'static str {
    "Run the suite on a backend whose decoder emits the frames these checks read, or \
     mark them `required = false` to record them as unobserved instead of scoring them."
}

impl Scoreability {
    /// The hard-error message for a selection where nothing can be scored.
    fn nothing_scoreable(&self) -> CliError {
        let mut message = match self.excluded.as_slice() {
            [only] => format!(
                "EVAL_CHECKS_UNOBSERVABLE: agent '{}' cannot produce the evidence {} required \
                 check(s) need, so this suite has no score on this backend:",
                only.agent_key, only.check_count
            ),
            many => format!(
                "EVAL_CHECKS_UNOBSERVABLE: none of the {} selected runtime(s) can score this \
                 suite:",
                many.len()
            ),
        };
        message.push('\n');
        message.push_str(&self.rendered_lines());
        message.push('\n');
        message.push_str(remedy());
        CliError::Config(message)
    }

    /// The stderr notice for a selection where some runtimes were dropped.
    fn exclusion_notice(&self) -> String {
        format!(
            "warning: {} runtime(s) excluded — they cannot score this suite:\n{}\n\
             Scoring {} runtime(s): {}.",
            self.excluded.len(),
            self.rendered_lines(),
            self.scoreable.len(),
            self.scoreable.join(", ")
        )
    }

    /// The hard error, consuming the split — `validate` has no fallback path
    /// once nothing is scoreable, so it does not need the parts back.
    pub(super) fn into_error(self) -> CliError {
        self.nothing_scoreable()
    }

    /// The excluded runtimes as `(agent key, reason block)`, for a caller that
    /// reports per agent instead of printing one combined notice.
    pub(super) fn into_reasons(self) -> Vec<(String, String)> {
        self.excluded
            .into_iter()
            .map(|ex| (ex.agent_key, ex.lines.join("\n")))
            .collect()
    }

    fn rendered_lines(&self) -> String {
        let mut out: Vec<String> = Vec::new();
        let multi = self.excluded.len() > 1;
        for ex in &self.excluded {
            if multi {
                out.push(format!("  agent '{}':", ex.agent_key));
            }
            for line in &ex.lines {
                out.push(if multi {
                    format!("  {}", line)
                } else {
                    line.clone()
                });
            }
        }
        let elided = out.len().saturating_sub(MAX_REASON_LINES);
        out.truncate(MAX_REASON_LINES);
        if elided > 0 {
            out.push(format!("  ... and {} more", elided));
        }
        out.join("\n")
    }
}

/// Split the selected runtimes into the ones that can score this suite and the
/// ones that cannot, without running anything.
pub(super) fn partition_scoreable(
    runtimes: &[String],
    cases: &[EvalCase],
    checks: &[CheckDefinition],
    skill_will_be_staged: bool,
) -> CliResult<Scoreability> {
    validate_suite_checks(cases, checks)?;

    let mut scoreable = Vec::new();
    let mut excluded = Vec::new();
    for agent_key in runtimes {
        let unobservable = unobservable_for(agent_key, cases, checks, skill_will_be_staged);
        if unobservable.is_empty() {
            scoreable.push(agent_key.clone());
        } else {
            excluded.push(ExcludedRuntime {
                agent_key: agent_key.clone(),
                lines: unobservable.iter().map(Unobservable::render).collect(),
                check_count: unobservable.len(),
            });
        }
    }
    Ok(Scoreability {
        scoreable,
        excluded,
    })
}

/// Resolve a selection down to the runtimes worth spending trials on.
///
/// Errors when none of them can score the suite. Otherwise returns the
/// scoreable subset plus, when anything was dropped, a notice to print.
pub(super) fn scoreable_runtimes(
    runtimes: &[String],
    cases: &[EvalCase],
    checks: &[CheckDefinition],
    skill_will_be_staged: bool,
) -> CliResult<(Vec<String>, Option<String>)> {
    let split = partition_scoreable(runtimes, cases, checks, skill_will_be_staged)?;
    if split.scoreable.is_empty() {
        return Err(split.nothing_scoreable());
    }
    let notice = (!split.excluded.is_empty()).then(|| split.exclusion_notice());
    Ok((split.scoreable, notice))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn case(id: &str, should_trigger: bool) -> EvalCase {
        EvalCase {
            id: id.to_string(),
            prompt: "do the thing".to_string(),
            should_trigger,
            tags: vec![],
            workspace_subdir: None,
        }
    }

    fn max_tool_calls() -> CheckDefinition {
        CheckDefinition::MaxToolCalls {
            limit: 100,
            required: true,
            cases: None,
        }
    }

    /// An explicit `skill_invoked` replaces the one `should_trigger` implies,
    /// and a literal `path` is matched without help from the run context — so
    /// this is the knob that keeps a suite scoreable off isolation. Tests that
    /// are about some *other* check use it to take the implicit one out of play.
    fn skill_invoked_by_path(expected: bool) -> CheckDefinition {
        CheckDefinition::SkillInvoked {
            skill: None,
            path: Some("SKILL.md".to_string()),
            expected,
            required: true,
            cases: None,
        }
    }

    fn only(agent: &str) -> Vec<String> {
        vec![agent.to_string()]
    }

    /// `gemini` is wrapped as text-only: its decoder emits no tool frames, so a
    /// tool-call ceiling is always trivially satisfied. Paying for the trials
    /// to learn that is the waste R10 exists to prevent.
    #[test]
    fn test_run_is_refused_when_a_required_check_cannot_be_observed() {
        let err = scoreable_runtimes(
            &only("gemini"),
            &[case("c1", true)],
            &[max_tool_calls()],
            true,
        )
        .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("EVAL_CHECKS_UNOBSERVABLE"), "got: {msg}");
        assert!(msg.contains("gemini"), "must name the backend: {msg}");
        assert!(msg.contains("c1"), "must name the case: {msg}");
        assert!(msg.contains("max_tool_calls"), "must name the check: {msg}");
    }

    /// The same suite on a backend that decodes tool frames is scoreable, so
    /// nothing is refused. Without this half the refusal could be unconditional.
    #[test]
    fn test_a_backend_that_decodes_tool_frames_is_not_refused() {
        let (scoreable, notice) = scoreable_runtimes(
            &only("claude"),
            &[case("c1", true)],
            &[max_tool_calls()],
            true,
        )
        .unwrap();
        assert_eq!(scoreable, only("claude"));
        assert!(notice.is_none(), "nothing was excluded: {notice:?}");
    }

    /// The `should_trigger` column generates a required skill-invocation check
    /// that appears in no checks file. Off isolation there is no staged path,
    /// and on a backend with no typed `Skill` tool the check has nothing at all
    /// to match on — the pre-flight must see the implicit check, not just the
    /// file's.
    #[test]
    fn test_the_implicit_should_trigger_check_is_covered_by_the_refusal() {
        let err = scoreable_runtimes(&only("codex"), &[case("c1", true)], &[], false).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("skill_invoked"), "got: {msg}");
        assert!(msg.contains("c1"), "got: {msg}");
    }

    /// Staging a skill gives that same check a path to match, so isolation is
    /// what makes the suite scoreable on a pathless backend.
    #[test]
    fn test_staging_a_skill_makes_the_implicit_check_observable() {
        scoreable_runtimes(&only("codex"), &[case("c1", true)], &[], true).unwrap();
    }

    /// Off isolation, spelling the document path on the check restores the
    /// evidence the staged copy would have provided. This is the remedy the
    /// refusal message names, so it has to actually work.
    #[test]
    fn test_an_explicit_path_makes_the_check_observable_without_isolation() {
        scoreable_runtimes(
            &only("codex"),
            &[case("c1", true)],
            &[skill_invoked_by_path(true)],
            false,
        )
        .unwrap();
    }

    /// A check scoped to another case must not refuse this one.
    #[test]
    fn test_a_check_scoped_to_another_case_does_not_refuse_this_suite() {
        let scoped = CheckDefinition::MaxToolCalls {
            limit: 100,
            required: true,
            cases: Some(vec!["other".to_string()]),
        };
        // `gemini` cannot observe `skill_invoked` either, so the implicit check
        // has to be displaced for this test to be about the scoped one at all.
        let checks = vec![scoped, skill_invoked_by_path(true)];
        let err =
            scoreable_runtimes(&only("gemini"), &[case("c1", true)], &checks, true).unwrap_err();
        assert!(
            !err.to_string().contains("max_tool_calls"),
            "a check scoped to case 'other' must not be reported against 'c1': {err}"
        );
    }

    /// A contradiction between the column and an explicit check is a config
    /// error, caught before any agent runs rather than silently letting one win.
    #[test]
    fn test_a_check_contradicting_should_trigger_is_refused() {
        let contradiction = CheckDefinition::SkillInvoked {
            skill: Some("fastskill".to_string()),
            path: None,
            expected: false,
            required: true,
            cases: None,
        };
        let err = scoreable_runtimes(&only("claude"), &[case("c1", true)], &[contradiction], true)
            .unwrap_err();
        assert!(
            err.to_string().contains("EVAL_CHECKS_INVALID"),
            "got: {err}"
        );
    }

    /// An optional check that cannot be observed is recorded as unobserved, not
    /// refused: R10 gates on `required` only.
    #[test]
    fn test_an_optional_unobservable_check_does_not_refuse_the_suite() {
        let optional = CheckDefinition::MaxToolCalls {
            limit: 100,
            required: false,
            cases: None,
        };
        // Claude can observe the implicit `skill_invoked`, so the optional
        // ceiling is the only thing left that could have refused.
        scoreable_runtimes(&only("claude"), &[case("c1", true)], &[optional], true).unwrap();
    }

    /// An unrecognised agent key stays lenient, matching `CheckContext::default`.
    #[test]
    fn test_an_unknown_backend_is_not_refused() {
        scoreable_runtimes(
            &only("some-future-agent"),
            &[case("c1", true)],
            &[max_tool_calls()],
            true,
        )
        .unwrap();
    }

    /// `--all` must not be held hostage by one backend it cannot score: the
    /// scoreable runtimes still run, and the dropped one is named out loud.
    #[test]
    fn test_a_mixed_selection_keeps_the_runtimes_that_can_score() {
        let runtimes = vec![
            "claude".to_string(),
            "gemini".to_string(),
            "codex".to_string(),
        ];
        let (scoreable, notice) =
            scoreable_runtimes(&runtimes, &[case("c1", true)], &[max_tool_calls()], true).unwrap();
        assert_eq!(scoreable, vec!["claude".to_string(), "codex".to_string()]);
        let notice = notice.expect("dropping a runtime must produce a notice");
        assert!(notice.contains("gemini"), "got: {notice}");
        assert!(notice.contains("max_tool_calls"), "got: {notice}");
        assert!(
            !notice.contains("agent 'claude'"),
            "a scoreable runtime must not be listed as excluded: {notice}"
        );
    }

    /// When every selected runtime is unscoreable the notice is not enough:
    /// there is no score at all, so it is an error naming each one.
    #[test]
    fn test_a_selection_with_nothing_scoreable_is_an_error() {
        let runtimes = vec!["gemini".to_string(), "opencode".to_string()];
        let err = scoreable_runtimes(&runtimes, &[case("c1", true)], &[max_tool_calls()], true)
            .unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("none of the 2 selected runtime(s)"),
            "got: {msg}"
        );
        assert!(msg.contains("gemini"), "got: {msg}");
        assert!(msg.contains("opencode"), "got: {msg}");
    }

    /// One global check unobservable across many cases is one line, not one per
    /// case: a 14-case suite must not bury the remedy under 14 copies of it.
    #[test]
    fn test_one_check_across_many_cases_is_reported_once() {
        let cases: Vec<EvalCase> = (0..14).map(|i| case(&format!("c{i}"), true)).collect();
        let err =
            scoreable_runtimes(&only("gemini"), &cases, &[max_tool_calls()], true).unwrap_err();
        let msg = err.to_string();
        assert_eq!(
            msg.matches("max_tool_calls").count(),
            1,
            "expected one grouped line, got: {msg}"
        );
        assert!(msg.contains("14 cases"), "got: {msg}");
        assert!(msg.contains("+11 more"), "got: {msg}");
    }
}
