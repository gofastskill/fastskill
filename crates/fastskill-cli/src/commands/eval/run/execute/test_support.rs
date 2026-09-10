use super::*;
use fastskill_evals::artifacts::{CaseResult, CaseTrialsResult, IsolationReport, ScopeFidelity};
use fastskill_evals::checks::CheckDefinition;
use fastskill_evals::runner::{CaseRunOptions, CaseRunOutput};
use fastskill_evals::suite::EvalCase;
use std::path::{Path, PathBuf};

pub(super) struct CwdGuard(PathBuf);

impl CwdGuard {
    pub(super) fn enter(path: &Path) -> Self {
        let original = env::current_dir().unwrap();
        env::set_current_dir(path).unwrap();
        Self(original)
    }
}

impl Drop for CwdGuard {
    fn drop(&mut self) {
        env::set_current_dir(&self.0).unwrap();
    }
}

#[derive(Clone)]
pub(super) struct ScriptedRunner {
    pub(super) status: CaseStatus,
    pub(super) report_isolation: bool,
    pub(super) block_artifact_directory: bool,
}

#[async_trait::async_trait]
impl EvalRunner for ScriptedRunner {
    async fn run_case(
        &self,
        case: &EvalCase,
        opts: &CaseRunOptions,
        _checks: &[CheckDefinition],
    ) -> (CaseRunOutput, CaseResult, String) {
        let passed = self.status == CaseStatus::Passed;
        if self.block_artifact_directory {
            let run_dir = opts
                .retain_workspace_in
                .as_ref()
                .and_then(|path| path.parent())
                .unwrap();
            std::fs::remove_dir_all(run_dir).unwrap();
            std::fs::write(run_dir, "block directory recreation").unwrap();
        }
        (
            CaseRunOutput {
                stdout: b"agent output".to_vec(),
                stderr: Vec::new(),
                exit_code: Some(if passed { 0 } else { 1 }),
                timed_out: false,
                workspace: None,
                workspace_diff: Some("diff --git a/a b/a".to_string()),
                isolation: self.report_isolation.then(|| IsolationReport {
                    requested: false,
                    project_scope: ScopeFidelity::Inherited,
                    user_scope: ScopeFidelity::Inherited,
                    mechanism: Some("test-runner".to_string()),
                    agent_version: Some("1.0.0".to_string()),
                    ambient_skills: vec!["ambient-demo".to_string()],
                    workspace_root: None,
                    degrade_reason: None,
                }),
            },
            CaseResult {
                id: case.id.clone(),
                status: self.status.clone(),
                command_count: Some(2),
                input_tokens: Some(3),
                output_tokens: Some(5),
                check_results: Vec::new(),
                error_message: (!passed).then(|| "scripted failure".to_string()),
                exit_code: Some(if passed { 0 } else { 1 }),
                terminal: None,
                cost_usd: Some(0.01),
                tokens: Default::default(),
                skill_path: Some(PathBuf::from("SKILL.md")),
            },
            "{\"type\":\"test\"}\n".to_string(),
        )
    }

    async fn run_case_trials(
        &self,
        _case: &EvalCase,
        _opts: &CaseRunOptions,
        _checks: &[CheckDefinition],
        _trial_count: u32,
        _max_parallelism: Option<u32>,
    ) -> CaseTrialsResult {
        unreachable!("execute_run_with_runner calls run_case")
    }
}

pub(super) fn run_args(output_dir: PathBuf) -> RunArgs {
    RunArgs {
        agent: vec!["aikit".to_string()],
        all: false,
        output_dir,
        model: Some("test-model".to_string()),
        case: None,
        tag: None,
        format: None,
        json: false,
        no_fail: false,
        trials: Some(1),
        ci: false,
        threshold: Some(0.5),
        judge: false,
        judge_model: None,
        no_isolation: true,
    }
}

pub(super) fn scaffold_project(dir: &Path) {
    std::fs::create_dir_all(dir.join("evals")).unwrap();
    std::fs::write(
        dir.join("evals/prompts.csv"),
        "id,prompt,should_trigger,tags\ncase-1,say hello,true,smoke\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("evals/checks.toml"),
        "[[check]]\nname = \"skill_invoked\"\npath = \"SKILL.md\"\nexpected = true\nrequired = true\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("skill-project.toml"),
        "[metadata]\nid = \"demo-skill\"\nversion = \"1.0.0\"\n\n\
         [tool.fastskill.eval]\nprompts = \"evals/prompts.csv\"\n\
         checks = \"evals/checks.toml\"\nparallel = 1\n\
         fail_on_missing_agent = false\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("SKILL.md"),
        "---\nname: demo-skill\ndescription: Demo\n---\n# Demo\n",
    )
    .unwrap();
}
