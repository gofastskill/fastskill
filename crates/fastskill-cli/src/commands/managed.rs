//! `fastskill managed`: follow a signed managed state for this user's agents
//! ([ADR-0016](../../../../docs/adr/0016-machines-follow-a-signed-managed-state.md)).
//!
//! `enroll`, `apply` and `unenroll` are writes, `status` is a read (ADR-0003). The work is in
//! `fastskill_core::core::managed`; this module reads the settings, runs it and renders it.

use crate::error::{CliError, CliResult};
use cli_framework::command::{FromArgValueMap, IntoCommandSpec};
use cli_framework::spec::arg_spec::{ArgKind, ArgSpec, ArgValueType, Cardinality};
use cli_framework::spec::command_tree::{CommandSpec, GroupMetadata};
use cli_framework::spec::value::ArgValue;
use fastskill_core::core::managed::config::user_file_path;
use fastskill_core::core::managed::{
    self, ApplyContext, ApplyOutcome, ApplyResult, ManagedLayout, ManagedSettings, ManagedStatus,
    UnenrollOutcome,
};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::IsTerminal;
use std::path::PathBuf;

/// The `managed` group.
pub fn group_metadata() -> GroupMetadata {
    GroupMetadata {
        summary: "Follow a signed managed state for this user's agents",
        hidden: false,
        category: Some("Operations"),
        help_order: Some(50),
    }
}

/// The options every `managed` command takes.
#[derive(Debug, Default)]
pub struct ManagedArgs {
    pub json: bool,
    /// `status` only: include what the last apply did and the report body; with `--json`, print
    /// only the report body.
    pub report: bool,
}

fn json_arg() -> ArgSpec {
    ArgSpec {
        name: "json",
        kind: ArgKind::Flag,
        long: Some("json"),
        value_type: ArgValueType::Bool,
        cardinality: Cardinality::Optional,
        help: "Output in JSON format",
        ..Default::default()
    }
}

fn spec(summary: &'static str, syntax: &'static str, examples: Vec<&'static str>) -> CommandSpec {
    CommandSpec {
        summary,
        syntax: Some(syntax),
        category: Some("managed"),
        examples,
        args: vec![json_arg()],
        ..Default::default()
    }
}

impl FromArgValueMap for ManagedArgs {
    fn from_arg_value_map(map: &HashMap<String, ArgValue>) -> Self {
        Self {
            json: matches!(map.get("json"), Some(ArgValue::Bool(true))),
            report: matches!(map.get("report"), Some(ArgValue::Bool(true))),
        }
    }
}

macro_rules! managed_args {
    ($name:ident, $spec:expr) => {
        #[derive(Debug, Default)]
        pub struct $name(pub ManagedArgs);

        impl IntoCommandSpec for $name {
            fn command_spec() -> CommandSpec {
                $spec
            }
        }

        impl FromArgValueMap for $name {
            fn from_arg_value_map(map: &HashMap<String, ArgValue>) -> Self {
                Self(ManagedArgs::from_arg_value_map(map))
            }
        }
    };
}

managed_args!(
    EnrollArgs,
    spec(
        "Enroll this user with the configured managed source and apply its state",
        "managed enroll [--json]",
        vec!["fastskill managed enroll"],
    )
);
managed_args!(
    ApplyArgs,
    spec(
        "Apply the managed state to this user's agents",
        "managed apply [--json]",
        vec!["fastskill managed apply", "fastskill managed apply --json"],
    )
);
managed_args!(StatusArgs, {
    let mut spec = spec(
        "Show the managed source, state, targets, collisions and quarantine",
        "managed status [--report] [--json]",
        vec![
            "fastskill managed status",
            "fastskill managed status --report",
            "fastskill managed status --json --report",
        ],
    );
    spec.args.push(ArgSpec {
        name: "report",
        kind: ArgKind::Flag,
        long: Some("report"),
        value_type: ArgValueType::Bool,
        cardinality: Cardinality::Optional,
        help: "Include everything the last apply did and the report body; with --json, print \
               only the report body",
        ..Default::default()
    });
    spec
});
managed_args!(
    UnenrollArgs,
    spec(
        "Remove managed skills, records and settings for this user, keeping the quarantine",
        "managed unenroll [--json]",
        vec!["fastskill managed unenroll"],
    )
);

/// What a command runs against, so tests can point it elsewhere.
pub struct Environment {
    pub layout: ManagedLayout,
    pub settings: ManagedSettings,
    pub user_settings_file: Option<PathBuf>,
    pub home: PathBuf,
    pub project_skills: Option<PathBuf>,
    /// Whether a person is watching; a hook runs without a terminal.
    pub interactive: bool,
    /// FastSkill's configuration folder, where the credential command runs.
    pub config_dir: PathBuf,
}

impl Environment {
    /// This user's environment.
    pub fn current() -> CliResult<Self> {
        let settings = ManagedSettings::load().map_err(fastskill_core::ServiceError::from)?;
        Ok(Self {
            layout: ManagedLayout::for_current_user()?,
            settings,
            config_dir: user_file_path()
                .and_then(|file| file.parent().map(PathBuf::from))
                .unwrap_or_else(std::env::temp_dir),
            user_settings_file: user_file_path(),
            home: dirs::home_dir()
                .ok_or_else(|| CliError::Config("can't determine the home folder".to_string()))?,
            project_skills: crate::config::resolve_skills_storage_directory(false).ok(),
            interactive: std::io::stdout().is_terminal(),
        })
    }

    fn context(&self, may_enroll: bool) -> CliResult<ApplyContext> {
        if !self.settings.is_configured() {
            return Err(not_configured());
        }
        Ok(ApplyContext {
            layout: self.layout.clone(),
            settings: self.settings.clone(),
            home: self.home.clone(),
            project_skills: self.project_skills.clone(),
            may_enroll,
            now: chrono::Utc::now(),
            interactive: self.interactive,
            config_dir: self.config_dir.clone(),
        })
    }
}

fn not_configured() -> CliError {
    CliError::Config(
        "no managed source is configured; set `source` and `[[keys]]` in the system or user \
         managed.toml"
            .to_string(),
    )
}

pub async fn execute_enroll(args: EnrollArgs) -> CliResult<()> {
    emit(off_runtime(move || run_enroll(&Environment::current()?, &args.0)).await?)
}

pub async fn execute_apply(args: ApplyArgs) -> CliResult<()> {
    emit(off_runtime(move || run_apply(&Environment::current()?, &args.0)).await?)
}

/// Run an apply on a blocking thread: it makes blocking https requests and runs the
/// credential command.
async fn off_runtime(
    work: impl FnOnce() -> CliResult<Rendered> + Send + 'static,
) -> CliResult<Rendered> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|error| CliError::Config(format!("the apply stopped unexpectedly: {error}")))?
}

pub async fn execute_status(args: StatusArgs) -> CliResult<()> {
    let env = Environment::current()?;
    emit(Ok(Some(run_status(&env, &args.0)?)))
}

pub async fn execute_unenroll(args: UnenrollArgs) -> CliResult<()> {
    let env = Environment::current()?;
    emit(Ok(Some(run_unenroll(&env, &args.0)?)))
}

/// Print what a command produced; an incomplete apply is an error after its report.
fn emit(result: Result<Option<String>, (String, CliError)>) -> CliResult<()> {
    match result {
        Ok(Some(text)) => {
            crate::outln!("{text}");
            Ok(())
        }
        Ok(None) => Ok(()),
        Err((text, error)) => {
            crate::outln!("{text}");
            Err(error)
        }
    }
}

type Rendered = Result<Option<String>, (String, CliError)>;

pub fn run_enroll(env: &Environment, args: &ManagedArgs) -> CliResult<Rendered> {
    let result = managed::enroll(&env.context(true)?)?;
    Ok(rendered_apply(result, env, args))
}

pub fn run_apply(env: &Environment, args: &ManagedArgs) -> CliResult<Rendered> {
    let result = managed::apply(&env.context(env.settings.source_from_system)?)?;
    Ok(rendered_apply(result, env, args))
}

fn rendered_apply(result: ApplyResult, env: &Environment, args: &ManagedArgs) -> Rendered {
    let outcome = match result {
        // A hook that finds an apply running has nothing to add.
        ApplyResult::Busy if !env.interactive => return Ok(None),
        ApplyResult::Busy => {
            return Ok(Some(
                "An apply is already running for this user; it will finish on its own.".to_string(),
            ))
        }
        ApplyResult::Done(outcome) => outcome,
    };
    let text = if args.json {
        serde_json::to_string_pretty(&*outcome).unwrap_or_default()
    } else {
        render_outcome(&outcome)
    };
    if outcome.completed {
        Ok(Some(text))
    } else {
        Err((
            text,
            CliError::Validation(format!(
                "the apply didn't complete: {} problem(s); see `fastskill managed status --report`",
                outcome.failures.len()
            )),
        ))
    }
}

/// The human report of an apply.
pub fn render_outcome(outcome: &ApplyOutcome) -> String {
    let mut lines = Vec::new();
    if outcome.enrolled_now {
        lines.push("Enrolled this user.".to_string());
    }
    lines.push(format!(
        "Applied the managed state for {:?}{}.",
        outcome.subject,
        if outcome.expired {
            " (expired: no skills were added, changed or removed)"
        } else {
            ""
        }
    ));
    if let Some(problem) = &outcome.state_problem {
        lines.push(format!("Used the last accepted state: {problem}"));
    }
    if outcome.sign_in_needed {
        lines.push(SIGN_IN_NEEDED.to_string());
    }
    if outcome.targets.is_empty() {
        lines.push("No agent targets were found for this user.".to_string());
    }
    for target in &outcome.targets {
        lines.push(format!("  target   {}", target.display()));
    }
    for (label, changes) in [
        ("deployed", &outcome.deployed),
        ("adopted", &outcome.adopted),
        ("removed", &outcome.removed),
    ] {
        for change in changes {
            lines.push(format!("  {label:<8} {}", change.path.display()));
        }
    }
    for record in &outcome.quarantined {
        lines.push(format!(
            "  quarantined {} ({:?}) -> {}",
            record.former_path.display(),
            record.reason,
            record.path.display()
        ));
    }
    for path in &outcome.collisions {
        lines.push(format!(
            "  collision {}: not FastSkill's and not the listed content; left alone",
            path.display()
        ));
    }
    for path in &outcome.project_clashes {
        lines.push(format!(
            "  clash    {}: a project skill has the id of a managed skill",
            path.display()
        ));
    }
    for (what, why) in &outcome.failures {
        lines.push(format!("  failed   {what}: {why}"));
    }
    for warning in &outcome.warnings {
        lines.push(format!("  warning  {warning}"));
    }
    if let Some(report) = &outcome.report {
        lines.push(match (&report.problem, report.accepted) {
            (_, true) => format!("  report   #{} accepted", report.sequence),
            (Some(problem), _) => format!("  report   not accepted: {problem}"),
            (None, false) => "  report   not accepted".to_string(),
        });
    }
    lines.join("\n")
}

const SIGN_IN_NEEDED: &str =
    "Sign-in needed: the credential command didn't give a token the source accepts. Run \
     `fastskill managed apply` in a terminal to sign in.";

pub fn run_status(env: &Environment, args: &ManagedArgs) -> CliResult<String> {
    let status = managed::status(&env.layout, &env.settings, chrono::Utc::now())?;
    if args.json && args.report {
        // The exact body the next report carries, and nothing else (decision 15).
        return Ok(serde_json::to_string_pretty(&status.report).unwrap_or_default());
    }
    if args.json {
        return Ok(
            serde_json::to_string_pretty(&status_json(&status, args.report)).unwrap_or_default(),
        );
    }
    Ok(render_status(&status, args.report))
}

fn status_json(status: &ManagedStatus, report: bool) -> Value {
    let last = status.last_apply.as_ref();
    json!({
        "source": status.source,
        "enrolled": status.enrollment.is_some(),
        "machine_id": status.enrollment.as_ref().map(|e| e.machine_id.clone()),
        "subject": status.subject,
        "issued_at": status.issued_at,
        "expires_at": status.expires_at,
        "expired": status.expired,
        "state_problem": status.state_problem,
        "sign_in_needed": status.sign_in_needed,
        "ignored": status.ignored,
        "report_to": status.report_note,
        "warnings": last.map(|l| l.warnings.clone()).unwrap_or_default(),
        "targets": last.map(|l| l.targets.clone()).unwrap_or_default(),
        "collisions": last.map(|l| l.collisions.clone()).unwrap_or_default(),
        "owned": status.owned,
        "quarantine": status.quarantine.iter().map(|record| json!({
            "path": record.path,
            "former_path": record.former_path,
            "digest": record.digest,
            "reason": record.reason,
            "quarantined_at": record.quarantined_at,
        })).collect::<Vec<_>>(),
        "last_apply": if report { serde_json::to_value(last).unwrap_or_default() } else {
            json!(last.map(|l| json!({ "applied_at": l.applied_at, "completed": l.completed })))
        },
    })
}

/// The human status.
pub fn render_status(status: &ManagedStatus, report: bool) -> String {
    let Some(source) = &status.source else {
        return "No managed source is configured.".to_string();
    };
    let mut lines = vec![format!("Source:   {source}")];
    let Some(enrollment) = &status.enrollment else {
        lines.push("Not enrolled; run `fastskill managed enroll`.".to_string());
        return lines.join("\n");
    };
    lines.push(format!("Machine:  {}", enrollment.machine_id));
    if let Some(subject) = &status.subject {
        lines.push(format!("Subject:  {subject}"));
    }
    if let Some(expires_at) = status.expires_at {
        lines.push(format!(
            "Expires:  {expires_at}{}",
            if status.expired { " (expired)" } else { "" }
        ));
    }
    if let Some(problem) = &status.state_problem {
        lines.push(format!("State:    {problem}"));
    }
    if status.sign_in_needed {
        lines.push(SIGN_IN_NEEDED.to_string());
    }
    for ignored in &status.ignored {
        lines.push(format!("Ignored:  {ignored}"));
    }
    if let Some(note) = &status.report_note {
        lines.push(format!("Report:   {note}"));
    }
    if let Some(last) = &status.last_apply {
        lines.push(format!(
            "Last apply: {} ({})",
            last.applied_at.map(|at| at.to_string()).unwrap_or_default(),
            if last.completed {
                "completed"
            } else {
                "did not complete"
            }
        ));
        for target in &last.targets {
            lines.push(format!("  target    {}", target.display()));
        }
        for path in &last.collisions {
            lines.push(format!("  collision {}", path.display()));
        }
        for warning in &last.warnings {
            lines.push(format!("  warning   {warning}"));
        }
        if report {
            lines.push(render_outcome(last));
        }
    }
    if let (true, Some(body)) = (report, &status.report) {
        lines.push("Report body:".to_string());
        lines.push(serde_json::to_string_pretty(body).unwrap_or_default());
    }
    lines.push(format!("Managed entries: {}", status.owned.len()));
    if status.quarantine.is_empty() {
        lines.push("Quarantine: empty".to_string());
    } else {
        lines.push("Quarantine:".to_string());
        for record in &status.quarantine {
            lines.push(format!(
                "  {} ({:?}, was {})",
                record.path.display(),
                record.reason,
                record.former_path.display()
            ));
        }
    }
    lines.join("\n")
}

pub fn run_unenroll(env: &Environment, args: &ManagedArgs) -> CliResult<String> {
    let outcome = managed::unenroll(
        &env.layout,
        &env.settings,
        env.user_settings_file.as_deref(),
        chrono::Utc::now(),
    )?;
    if args.json {
        return Ok(serde_json::to_string_pretty(&unenroll_json(&outcome)).unwrap_or_default());
    }
    let mut lines = vec!["Unenrolled this user.".to_string()];
    for path in &outcome.removed {
        lines.push(format!("  removed     {}", path.display()));
    }
    for record in &outcome.quarantined {
        lines.push(format!(
            "  quarantined {} -> {}",
            record.former_path.display(),
            record.path.display()
        ));
    }
    if let Some(file) = &outcome.removed_settings {
        lines.push(format!("  removed     {}", file.display()));
    }
    Ok(lines.join("\n"))
}

fn unenroll_json(outcome: &UnenrollOutcome) -> Value {
    json!({
        "removed": outcome.removed,
        "quarantined": outcome.quarantined.iter().map(|record| json!({
            "path": record.path,
            "former_path": record.former_path,
        })).collect::<Vec<_>>(),
        "removed_settings": outcome.removed_settings,
    })
}

#[cfg(test)]
#[path = "managed_tests.rs"]
mod tests;
