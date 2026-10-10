use super::*;
use fastskill_core::core::managed::{
    Enrollment, EntryChange, ManagedSource, PinnedKey, QuarantineReason, QuarantineRecord,
    ReportDelivery,
};
use tempfile::TempDir;

fn environment(dir: &TempDir, configured: bool, interactive: bool) -> Environment {
    let source = dir.path().join("source").join("state.dsse");
    Environment {
        layout: ManagedLayout::new(dir.path().join("data")),
        settings: ManagedSettings {
            source: configured.then_some(ManagedSource::File(source)),
            keys: vec![PinnedKey {
                id: "k1".to_string(),
                public_key: [7; 32],
            }],
            source_from_system: true,
            ..ManagedSettings::default()
        },
        user_settings_file: Some(dir.path().join("managed.toml")),
        home: dir.path().join("home"),
        project_skills: None,
        interactive,
        config_dir: dir.path().to_path_buf(),
    }
}

fn human() -> ManagedArgs {
    ManagedArgs::default()
}

fn json_args() -> ManagedArgs {
    ManagedArgs {
        json: true,
        report: false,
    }
}

fn enroll_record(env: &Environment) {
    std::fs::create_dir_all(env.layout.managed()).unwrap();
    let enrollment = Enrollment::new(
        env.settings.source.as_ref().unwrap().as_str(),
        chrono::Utc::now(),
    );
    std::fs::write(
        env.layout.enrollment_file(),
        serde_json::to_vec(&enrollment).unwrap(),
    )
    .unwrap();
}

fn outcome(completed: bool) -> ApplyOutcome {
    let change = |name: &str| EntryChange {
        path: PathBuf::from(format!("/home/u/.claude/skills/{name}")),
        id: name.to_string(),
        digest: "sha256:00".to_string(),
    };
    ApplyOutcome {
        completed,
        applied_at: Some(chrono::Utc::now()),
        subject: "team-a".to_string(),
        expired: true,
        state_problem: Some("the source can't be reached".to_string()),
        enrolled_now: true,
        targets: vec![PathBuf::from("/home/u/.claude/skills")],
        deployed: vec![change("one")],
        adopted: vec![change("two")],
        removed: vec![change("three")],
        quarantined: vec![QuarantineRecord {
            former_path: PathBuf::from("/home/u/.claude/skills/bad"),
            reason: QuarantineReason::Blocked,
            path: PathBuf::from("/data/quarantine/x/content"),
            ..QuarantineRecord::default()
        }],
        collisions: vec![PathBuf::from("/home/u/.claude/skills/theirs")],
        project_clashes: vec![PathBuf::from("/p/.claude/skills/one")],
        failures: if completed {
            Vec::new()
        } else {
            vec![("one".to_string(), "digest mismatch".to_string())]
        },
        ..ApplyOutcome::default()
    }
}

#[test]
fn specs_name_the_commands_and_flags() {
    assert!(EnrollArgs::command_spec().summary.contains("Enroll"));
    assert!(ApplyArgs::command_spec().summary.contains("Apply"));
    assert!(UnenrollArgs::command_spec().summary.contains("Remove"));
    let status = StatusArgs::command_spec();
    let names: Vec<_> = status.args.iter().map(|arg| arg.name).collect();
    assert_eq!(names, ["json", "report"]);
    assert_eq!(group_metadata().category, Some("Operations"));
}

#[test]
fn args_read_the_flags() {
    let mut map = HashMap::new();
    map.insert("json".to_string(), ArgValue::Bool(true));
    map.insert("report".to_string(), ArgValue::Bool(true));
    let args = StatusArgs::from_arg_value_map(&map).0;
    assert!(args.json && args.report);
    let args = ApplyArgs::from_arg_value_map(&HashMap::new()).0;
    assert!(!args.json && !args.report);
    assert!(!EnrollArgs::from_arg_value_map(&HashMap::new()).0.json);
    assert!(!UnenrollArgs::from_arg_value_map(&HashMap::new()).0.json);
}

#[test]
fn without_a_source_apply_and_enroll_say_so() {
    let dir = TempDir::new().unwrap();
    let env = environment(&dir, false, true);
    for result in [run_apply(&env, &human()), run_enroll(&env, &human())] {
        let error = result.err().unwrap().to_string();
        assert!(error.contains("no managed source is configured"), "{error}");
    }
}

#[test]
fn apply_before_enrolling_is_an_error_when_the_source_is_the_users() {
    let dir = TempDir::new().unwrap();
    let mut env = environment(&dir, true, true);
    env.settings.source_from_system = false;
    let error = run_apply(&env, &human()).err().unwrap().to_string();
    assert!(error.contains("isn't enrolled"), "{error}");
}

#[test]
fn enroll_with_an_unreadable_state_fails() {
    let dir = TempDir::new().unwrap();
    let env = environment(&dir, true, true);
    let error = run_enroll(&env, &human()).err().unwrap().to_string();
    assert!(error.contains("can't read the managed state"), "{error}");
}

#[test]
fn a_busy_apply_is_silent_from_a_hook_and_explained_in_a_terminal() {
    let dir = TempDir::new().unwrap();
    let hook = environment(&dir, true, false);
    let _lock = hook.layout.try_lock().unwrap().unwrap();
    assert!(matches!(run_apply(&hook, &human()).unwrap(), Ok(None)));
    let terminal = environment(&dir, true, true);
    let text = run_apply(&terminal, &human()).unwrap().unwrap().unwrap();
    assert!(text.contains("already running"), "{text}");
}

#[test]
fn a_completed_apply_is_reported() {
    let dir = TempDir::new().unwrap();
    let env = environment(&dir, true, true);
    let done = ApplyResult::Done(Box::new(outcome(true)));
    let text = rendered_apply(done, &env, &human()).unwrap().unwrap();
    for expected in [
        "Enrolled this user.",
        "(expired",
        "Used the last accepted state",
        "deployed",
        "adopted",
        "removed",
        "quarantined /home/u/.claude/skills/bad",
        "collision",
        "clash",
    ] {
        assert!(text.contains(expected), "{expected} in {text}");
    }
    let done = ApplyResult::Done(Box::new(outcome(true)));
    let text = rendered_apply(done, &env, &json_args()).unwrap().unwrap();
    let value: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(value["subject"], "team-a");
}

#[test]
fn an_incomplete_apply_is_an_error_after_its_report() {
    let dir = TempDir::new().unwrap();
    let env = environment(&dir, true, true);
    let done = ApplyResult::Done(Box::new(outcome(false)));
    let (text, error) = rendered_apply(done, &env, &human()).unwrap_err();
    assert!(text.contains("failed   one: digest mismatch"), "{text}");
    assert!(error.to_string().contains("1 problem(s)"));
}

#[test]
fn an_apply_without_targets_says_so() {
    let text = render_outcome(&ApplyOutcome {
        completed: true,
        ..ApplyOutcome::default()
    });
    assert!(text.contains("No agent targets were found"), "{text}");
    assert!(!text.contains("expired"));
}

#[test]
fn emit_prints_and_passes_errors_on() {
    assert!(emit(Ok(Some("text".to_string()))).is_ok());
    assert!(emit(Ok(None)).is_ok());
    let error = CliError::Validation("no".to_string());
    assert!(emit(Err(("text".to_string(), error))).is_err());
}

#[test]
fn status_without_a_source_or_enrollment() {
    let dir = TempDir::new().unwrap();
    let text = run_status(&environment(&dir, false, true), &human()).unwrap();
    assert_eq!(text, "No managed source is configured.");
    let text = run_status(&environment(&dir, true, true), &human()).unwrap();
    assert!(text.contains("Not enrolled"), "{text}");
}

#[test]
fn status_of_an_enrolled_user() {
    let dir = TempDir::new().unwrap();
    let env = environment(&dir, true, true);
    enroll_record(&env);
    std::fs::write(
        env.layout.last_apply_file(),
        serde_json::to_vec(&outcome(false)).unwrap(),
    )
    .unwrap();
    let source = dir.path().join("skill");
    std::fs::create_dir_all(&source).unwrap();
    managed::quarantine::quarantine(
        &env.layout,
        &source,
        None,
        QuarantineReason::NotAllowed,
        chrono::Utc::now(),
    )
    .unwrap();

    let text = run_status(&env, &human()).unwrap();
    for expected in [
        "Machine:",
        "State:    no state has been accepted yet",
        "did not complete",
        "target",
        "collision",
        "Managed entries: 0",
        "NotAllowed",
    ] {
        assert!(text.contains(expected), "{expected} in {text}");
    }
    assert!(!text.contains("deployed"));
    let report = ManagedArgs {
        json: false,
        report: true,
    };
    assert!(run_status(&env, &report).unwrap().contains("deployed"));

    let value: Value = serde_json::from_str(&run_status(&env, &json_args()).unwrap()).unwrap();
    assert_eq!(value["enrolled"], true);
    assert_eq!(value["last_apply"]["completed"], false);
    assert_eq!(value["quarantine"][0]["reason"], "not-allowed");
    let report = ManagedArgs {
        json: true,
        report: true,
    };
    let value: Value = serde_json::from_str(&run_status(&env, &report).unwrap()).unwrap();
    assert_eq!(value["format_version"], 1);
    assert_eq!(value["sequence"], 1);
    assert_eq!(value["machine_id"], value_of_machine(&env));
    assert!(value.get("last_apply").is_none(), "{value}");
}

fn value_of_machine(env: &Environment) -> String {
    let enrollment: Enrollment =
        serde_json::from_slice(&std::fs::read(env.layout.enrollment_file()).unwrap()).unwrap();
    enrollment.machine_id
}

#[test]
fn sign_in_warnings_and_the_report_are_shown() {
    let mut last = outcome(true);
    last.sign_in_needed = true;
    last.warnings = vec!["report_url is elsewhere".to_string()];
    for (delivery, expected) in [
        (
            ReportDelivery {
                sequence: 4,
                accepted: true,
                problem: None,
            },
            "report   #4 accepted",
        ),
        (
            ReportDelivery {
                sequence: 4,
                accepted: false,
                problem: Some("HTTP 500".to_string()),
            },
            "report   not accepted: HTTP 500",
        ),
        (ReportDelivery::default(), "report   not accepted"),
    ] {
        last.report = Some(delivery);
        let text = render_outcome(&last);
        for expected in [
            expected,
            "Sign-in needed",
            "warning  report_url is elsewhere",
        ] {
            assert!(text.contains(expected), "{expected} in {text}");
        }
    }

    let status = ManagedStatus {
        source: Some("/s/state.dsse".to_string()),
        enrollment: Some(Enrollment::new("/s/state.dsse", chrono::Utc::now())),
        sign_in_needed: true,
        ignored: vec!["report_url: a file source never gets a report".to_string()],
        report_note: Some("none: a file source never gets a report".to_string()),
        report: Some(managed::Report::build(managed::report::ReportInput {
            source: &ManagedSource::File(PathBuf::from("/s/state.dsse")),
            enrollment: &Enrollment::new("/s/state.dsse", chrono::Utc::now()),
            issued_at: None,
            completed: true,
            snapshot: &last.snapshot,
            quarantine: &[],
            refusals: &managed::report::Refusals::default(),
        })),
        last_apply: Some(last),
        ..ManagedStatus::default()
    };
    let text = render_status(&status, true);
    for expected in [
        "Sign-in needed",
        "Ignored:  report_url",
        "Report:   none: a file source",
        "  warning   report_url is elsewhere",
        "Report body:",
        "\"format_version\": 1",
    ] {
        assert!(text.contains(expected), "{expected} in {text}");
    }
    assert!(!render_status(&status, false).contains("Report body:"));
}

#[test]
fn status_shows_the_state_and_an_empty_quarantine() {
    let status = ManagedStatus {
        source: Some("/s/state.dsse".to_string()),
        enrollment: Some(Enrollment::new("/s/state.dsse", chrono::Utc::now())),
        subject: Some("team-a".to_string()),
        expires_at: Some(chrono::Utc::now()),
        expired: true,
        last_apply: Some(outcome(true)),
        ..ManagedStatus::default()
    };
    let text = render_status(&status, false);
    for expected in [
        "Subject:  team-a",
        "(expired)",
        "(completed)",
        "Quarantine: empty",
    ] {
        assert!(text.contains(expected), "{expected} in {text}");
    }
}

#[test]
fn unenroll_removes_the_records_and_settings() {
    let dir = TempDir::new().unwrap();
    let env = environment(&dir, true, true);
    enroll_record(&env);
    std::fs::write(env.user_settings_file.as_ref().unwrap(), "").unwrap();
    let text = run_unenroll(&env, &human()).unwrap();
    assert!(text.starts_with("Unenrolled this user."), "{text}");
    assert!(text.contains("managed.toml"), "{text}");
    assert!(!env.layout.enrollment_file().exists());

    enroll_record(&env);
    let value: Value = serde_json::from_str(&run_unenroll(&env, &json_args()).unwrap()).unwrap();
    assert_eq!(value["removed_settings"], Value::Null);
}

#[test]
fn unenroll_lists_removed_and_quarantined_entries() {
    let outcome = UnenrollOutcome {
        removed: vec![PathBuf::from("/h/.claude/skills/one")],
        quarantined: vec![QuarantineRecord {
            former_path: PathBuf::from("/h/.claude/skills/two"),
            path: PathBuf::from("/q/content"),
            ..QuarantineRecord::default()
        }],
        removed_settings: None,
    };
    let value = unenroll_json(&outcome);
    assert_eq!(
        value["quarantined"][0]["former_path"],
        "/h/.claude/skills/two"
    );
    assert_eq!(value["removed"][0], "/h/.claude/skills/one");
}

#[test]
fn unenroll_is_refused_when_the_system_requires_it() {
    let dir = TempDir::new().unwrap();
    let mut env = environment(&dir, true, true);
    env.settings.required_by_system = true;
    let error = run_unenroll(&env, &human()).unwrap_err().to_string();
    assert!(error.contains("can't unenroll"), "{error}");
}
