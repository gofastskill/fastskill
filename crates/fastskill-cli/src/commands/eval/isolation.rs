//! Environment isolation for `eval run` (spec 016): mode resolution from the
//! skill project, and human rendering of the achieved-isolation report.

use crate::error::{CliError, CliResult};
use fastskill_core::core::manifest::SkillProjectToml;
use fastskill_evals::artifacts::{IsolationReport, ScopeFidelity};
use fastskill_evals::runner::{IsolationMode, SkillSource};

/// One-line human rendering of the run's isolation contract (spec 016 D6).
///
/// `None` means the runner reported nothing — rendered as "unknown", never as
/// a claim in either direction.
pub(super) fn render_isolation_line(iso: Option<&IsolationReport>) -> String {
    let Some(iso) = iso else {
        return "isolation: unknown (no report from runner)".to_string();
    };
    let scope = |s: &ScopeFidelity| match s {
        ScopeFidelity::Isolated => "isolated",
        ScopeFidelity::Inherited => "inherited",
        ScopeFidelity::Unsupported => "unsupported",
    };
    let mut line = if iso.requested {
        format!(
            "isolation: project scope {}, user scope {}",
            scope(&iso.project_scope),
            scope(&iso.user_scope)
        )
    } else {
        "isolation: off (--no-isolation) — scores reflect this machine's ambient skills".to_string()
    };
    if let Some(mechanism) = &iso.mechanism {
        line.push_str(&format!(" (via {})", mechanism));
    }
    if let Some(reason) = &iso.degrade_reason {
        line.push_str(&format!(" — degraded: {}", reason));
    }
    line
}

/// Resolve the isolation mode for this run (spec 016).
///
/// Default is `Isolated`: the skill next to `skill-project.toml` is deployed
/// alone into a per-case scratch workspace, so scores measure the skill under
/// test instead of whatever else is installed on this machine. The skill
/// identity comes from the project itself — `SKILL.md` beside the project
/// file, named by `[metadata].id`. Both are required for isolation and their
/// absence is a loud config error, not a silent fallback to the ambient
/// environment.
pub(super) fn resolve_isolation_mode(
    no_isolation: bool,
    project_file: &std::path::Path,
    project_root: &std::path::Path,
) -> CliResult<IsolationMode> {
    if no_isolation {
        return Ok(IsolationMode::Inherit);
    }

    let skill_md = project_root.join("SKILL.md");
    if !skill_md.is_file() {
        return Err(CliError::Config(format!(
            "EVAL_ISOLATION_NO_SKILL: eval runs isolated by default, which needs a SKILL.md \
             next to '{}' to deploy as the skill under test. Add one, or pass --no-isolation \
             to run against the ambient agent environment.",
            project_file.display()
        )));
    }

    let content = std::fs::read_to_string(project_file).map_err(|e| {
        CliError::Config(format!(
            "EVAL_ISOLATION_NO_SKILL: cannot read '{}': {}",
            project_file.display(),
            e
        ))
    })?;
    // Schema-aware (legacy manifests upgrade in memory), like every other manifest reader.
    let toml = SkillProjectToml::from_toml_str(&content)
        .map_err(|e| CliError::Config(format!("EVAL_CONFIG_INVALID: {}", e)))?;
    let skill_name = toml
        .metadata
        .as_ref()
        .and_then(|m| m.id.clone())
        .ok_or_else(|| {
            CliError::Config(format!(
                "EVAL_ISOLATION_NO_SKILL: isolation needs [metadata].id in '{}' to name the \
                 skill under test. Add it, or pass --no-isolation.",
                project_file.display()
            ))
        })?;

    Ok(IsolationMode::Isolated {
        skill_name,
        source: SkillSource::Dir(project_root.to_path_buf()),
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod isolation_mode_tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn write_project(dir: &std::path::Path, with_id: bool, with_skill_md: bool) -> PathBuf {
        let project_file = dir.join("skill-project.toml");
        let metadata = if with_id {
            "[metadata]\nid = \"greeting-helper\"\nversion = \"1.0.0\"\n\n"
        } else {
            ""
        };
        std::fs::write(
            &project_file,
            format!("{metadata}[tool.fastskill.eval]\nprompts = \"evals/prompts.csv\"\ntimeout_seconds = 60\nfail_on_missing_agent = false\n"),
        )
        .unwrap();
        if with_skill_md {
            std::fs::write(
                dir.join("SKILL.md"),
                "---\nname: greeting-helper\n---\nbody\n",
            )
            .unwrap();
        }
        project_file
    }

    #[test]
    fn test_default_resolves_isolated_with_skill_identity() {
        let dir = TempDir::new().unwrap();
        let project_file = write_project(dir.path(), true, true);

        let mode = resolve_isolation_mode(false, &project_file, dir.path()).unwrap();
        match mode {
            IsolationMode::Isolated { skill_name, source } => {
                assert_eq!(skill_name, "greeting-helper");
                assert_eq!(source, SkillSource::Dir(dir.path().to_path_buf()));
            }
            IsolationMode::Inherit => panic!("default must be Isolated, got Inherit"),
        }
    }

    /// A pre-`Origin` manifest (no `schema_version`, `source = "git"` plus flat fields) must
    /// resolve through the schema-aware loader like every other command. A raw
    /// `toml::from_str` rejects it with an opaque untagged-enum error, so `fastskill eval`
    /// would refuse a project that `list` and `install` accept.
    #[test]
    fn test_legacy_manifest_resolves_isolated() {
        let dir = TempDir::new().unwrap();
        let project_file = dir.path().join("skill-project.toml");
        std::fs::write(
            &project_file,
            "[metadata]\nid = \"greeting-helper\"\nversion = \"1.0.0\"\n\n\
             [dependencies.helper]\nsource = \"git\"\nurl = \"https://github.com/org/helper\"\n\
             branch = \"main\"\n\n[tool.fastskill.eval]\nprompts = \"evals/prompts.csv\"\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("SKILL.md"),
            "---\nname: greeting-helper\n---\nbody\n",
        )
        .unwrap();

        let mode = resolve_isolation_mode(false, &project_file, dir.path()).unwrap();
        assert_eq!(
            mode,
            IsolationMode::Isolated {
                skill_name: "greeting-helper".to_string(),
                source: SkillSource::Dir(dir.path().to_path_buf()),
            }
        );
    }

    #[test]
    fn test_no_isolation_resolves_inherit_without_needing_skill_md() {
        let dir = TempDir::new().unwrap();
        let project_file = write_project(dir.path(), false, false);

        let mode = resolve_isolation_mode(true, &project_file, dir.path()).unwrap();
        assert_eq!(mode, IsolationMode::Inherit);
    }

    #[test]
    fn test_missing_skill_md_is_loud_error_not_silent_inherit() {
        let dir = TempDir::new().unwrap();
        let project_file = write_project(dir.path(), true, false);

        let err = resolve_isolation_mode(false, &project_file, dir.path()).unwrap_err();
        assert!(
            err.to_string().contains("EVAL_ISOLATION_NO_SKILL"),
            "unexpected error: {err}"
        );
        assert!(err.to_string().contains("--no-isolation"));
    }

    #[test]
    fn test_missing_metadata_id_is_loud_error() {
        let dir = TempDir::new().unwrap();
        let project_file = write_project(dir.path(), false, true);

        let err = resolve_isolation_mode(false, &project_file, dir.path()).unwrap_err();
        assert!(
            err.to_string().contains("EVAL_ISOLATION_NO_SKILL"),
            "unexpected error: {err}"
        );
        assert!(err.to_string().contains("[metadata].id"));
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod isolation_render_tests {
    use super::*;

    fn report(requested: bool) -> IsolationReport {
        IsolationReport {
            requested,
            project_scope: ScopeFidelity::Isolated,
            user_scope: ScopeFidelity::Isolated,
            mechanism: Some("--setting-sources project".to_string()),
            agent_version: None,
            ambient_skills: vec![],
            workspace_root: None,
            degrade_reason: None,
        }
    }

    #[test]
    fn test_render_no_report_is_unknown_never_a_claim() {
        let line = render_isolation_line(None);
        assert!(line.contains("unknown"), "got: {line}");
        assert!(
            !line.contains("isolated,"),
            "must not claim fidelity: {line}"
        );
    }

    #[test]
    fn test_render_isolated_reports_scopes_and_mechanism() {
        let line = render_isolation_line(Some(&report(true)));
        assert!(line.contains("project scope isolated"), "got: {line}");
        assert!(line.contains("user scope isolated"), "got: {line}");
        assert!(line.contains("--setting-sources project"), "got: {line}");
    }

    #[test]
    fn test_render_not_requested_says_off_and_warns() {
        let line = render_isolation_line(Some(&report(false)));
        assert!(line.contains("off"), "got: {line}");
        assert!(line.contains("ambient"), "got: {line}");
    }

    #[test]
    fn test_render_degraded_reason_is_surfaced() {
        let mut r = report(true);
        r.user_scope = ScopeFidelity::Inherited;
        r.degrade_reason = Some("opencode has no skills path".to_string());
        let line = render_isolation_line(Some(&r));
        assert!(line.contains("degraded"), "got: {line}");
        assert!(line.contains("opencode has no skills path"), "got: {line}");
        assert!(line.contains("user scope inherited"), "got: {line}");
    }
}

/// End-to-end locks on the isolation wiring: what `execute_run_with_runner`
/// actually hands the runner, and what lands in summary.json.
///
/// These tests `set_current_dir` into a temp project (the project file is
/// resolved from the cwd; there is no injection seam). That is process-global
/// state — safe under nextest's process-per-test model, which is what CI runs.
#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod isolation_e2e_tests {
    use super::*;
    use crate::commands::eval::run::{execute_run_with_runner, RunArgs};
    use fastskill_evals::artifacts::CaseResult;
    use fastskill_evals::artifacts::{CaseStatus, CaseTrialsResult, SummaryResult};
    use fastskill_evals::runner::CaseRunOutput;
    use fastskill_evals::runner::{CaseRunOptions, EvalRunner};
    use fastskill_evals::suite::EvalCase;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::sync::Mutex;
    use tempfile::TempDir;

    /// Captures every `CaseRunOptions` it is called with and reports a
    /// passing case plus a canned isolation report.
    struct CapturingRunner {
        seen: Mutex<Vec<CaseRunOptions>>,
    }

    #[async_trait::async_trait]
    impl EvalRunner for CapturingRunner {
        async fn run_case(
            &self,
            case: &EvalCase,
            opts: &CaseRunOptions,
            _checks: &[fastskill_evals::checks::CheckDefinition],
        ) -> (CaseRunOutput, CaseResult, String) {
            self.seen.lock().unwrap().push(opts.clone());
            let requested = matches!(opts.isolation, IsolationMode::Isolated { .. });
            (
                CaseRunOutput {
                    stdout: b"ok".to_vec(),
                    stderr: vec![],
                    exit_code: Some(0),
                    timed_out: false,
                    workspace: None,
                    isolation: Some(IsolationReport {
                        requested,
                        project_scope: ScopeFidelity::Isolated,
                        user_scope: ScopeFidelity::Isolated,
                        mechanism: Some("fake-mechanism".to_string()),
                        agent_version: None,
                        ambient_skills: vec![],
                        workspace_root: None,
                        degrade_reason: None,
                    }),
                },
                CaseResult {
                    id: case.id.clone(),
                    status: CaseStatus::Passed,
                    command_count: None,
                    input_tokens: None,
                    output_tokens: None,
                    check_results: vec![],
                    error_message: None,
                },
                String::new(),
            )
        }

        async fn run_case_trials(
            &self,
            _case: &EvalCase,
            _opts: &CaseRunOptions,
            _checks: &[fastskill_evals::checks::CheckDefinition],
            _trial_count: u32,
            _max_parallelism: Option<u32>,
        ) -> CaseTrialsResult {
            unreachable!("execute_run_with_runner drives run_case directly")
        }
    }

    fn scaffold_project(dir: &std::path::Path) {
        std::fs::create_dir_all(dir.join("evals")).unwrap();
        std::fs::write(
            dir.join("evals/prompts.csv"),
            "id,prompt,should_trigger\ncase-1,say hello,true\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("skill-project.toml"),
            "[metadata]\nid = \"greeting-helper\"\nversion = \"1.0.0\"\n\n\
             [tool.fastskill.eval]\nprompts = \"evals/prompts.csv\"\n\
             timeout_seconds = 60\nfail_on_missing_agent = false\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("SKILL.md"),
            "---\nname: greeting-helper\n---\nbody\n",
        )
        .unwrap();
    }

    fn run_args(output_dir: PathBuf, no_isolation: bool) -> RunArgs {
        RunArgs {
            agent: vec!["aikit".to_string()],
            all: false,
            output_dir,
            model: None,
            case: None,
            tag: None,
            format: None,
            json: false,
            no_fail: false,
            trials: None,
            ci: false,
            threshold: None,
            no_isolation,
        }
    }

    async fn run_and_capture(no_isolation: bool) -> (Vec<CaseRunOptions>, SummaryResult) {
        let project = TempDir::new().unwrap();
        scaffold_project(project.path());
        std::env::set_current_dir(project.path()).unwrap();

        let runner = Arc::new(CapturingRunner {
            seen: Mutex::new(vec![]),
        });
        let out_dir = project.path().join("eval-out");
        execute_run_with_runner(run_args(out_dir.clone(), no_isolation), Arc::clone(&runner))
            .await
            .unwrap();

        let seen = runner.seen.lock().unwrap().clone();

        // Exactly one run dir with one agent subdir was allocated.
        let run_dir = std::fs::read_dir(&out_dir)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path()
            .join("aikit");
        let summary: SummaryResult =
            serde_json::from_str(&std::fs::read_to_string(run_dir.join("summary.json")).unwrap())
                .unwrap();
        (seen, summary)
    }

    #[tokio::test]
    async fn test_default_run_hands_runner_isolated_mode_with_retention() {
        let (seen, summary) = run_and_capture(false).await;

        assert!(!seen.is_empty());
        for opts in &seen {
            match &opts.isolation {
                IsolationMode::Isolated { skill_name, source } => {
                    assert_eq!(skill_name, "greeting-helper");
                    assert!(matches!(source, SkillSource::Dir(_)));
                }
                IsolationMode::Inherit => {
                    panic!("default eval run must isolate, runner got Inherit")
                }
            }
            let retain = opts
                .retain_workspace_in
                .as_ref()
                .expect("failed-workspace retention dir must be set");
            assert!(retain.ends_with("workspaces"), "got: {}", retain.display());
        }

        let iso = summary
            .isolation
            .expect("summary.json must carry the isolation report");
        assert!(iso.requested);
        assert_eq!(iso.mechanism.as_deref(), Some("fake-mechanism"));
    }

    #[tokio::test]
    async fn test_no_isolation_hands_runner_inherit() {
        let (seen, summary) = run_and_capture(true).await;

        assert!(!seen.is_empty());
        for opts in &seen {
            assert_eq!(
                opts.isolation,
                IsolationMode::Inherit,
                "--no-isolation must reach the runner as Inherit"
            );
        }
        let iso = summary.isolation.expect("report still recorded");
        assert!(!iso.requested);
    }
}
