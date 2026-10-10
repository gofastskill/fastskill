#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::*;

fn setup(admin_present: fn(&str, &str) -> bool) -> HookSetup {
    HookSetup {
        program: quote_program(Path::new("/opt/fast skill/fastskill")),
        admin_present,
    }
}

fn no_admin(_: &str, _: &str) -> bool {
    false
}

fn claude_admin(key: &str, id: &str) -> bool {
    key == "claude" && id == "fastskill-managed-claude"
}

fn has(home: &Path, key: &str) -> bool {
    aikit_sdk::has_session_hook(home, key, &hook_id(key)).unwrap()
}

#[test]
fn a_hook_names_its_agent_and_runs_apply_in_hook_mode() {
    let hook = setup(no_admin).hook("claude");
    assert_eq!(hook.id, "fastskill-managed-claude");
    assert_eq!(
        hook.command,
        "\"/opt/fast skill/fastskill\" managed apply --hook fastskill-managed-claude"
    );
    assert_eq!(agent_of("fastskill-managed-gemini"), Some("gemini"));
    assert_eq!(agent_of("fastskill-managed-"), None);
    assert_eq!(agent_of("other"), None);
    assert_eq!(quote_program(Path::new("a\"b")), "\"a\\\"b\"");
    assert_eq!(HookSetup::new("x").program, "x");
}

#[test]
fn sync_registers_covered_agents_and_removes_the_others() {
    let home = tempfile::tempdir().unwrap();
    let setup = setup(no_admin);

    let first = sync(home.path(), &setup, &["claude", "gemini", "codex"]);
    assert_eq!(first.hooked, vec!["claude", "gemini"]);
    assert!(first.warnings.is_empty(), "{:?}", first.warnings);
    assert_eq!(first.approvals.len(), 1);
    assert!(first.approvals[0].starts_with("claude: "));
    assert!(has(home.path(), "claude") && has(home.path(), "gemini"));

    let again = sync(home.path(), &setup, &["claude", "gemini"]);
    assert_eq!(again.hooked, vec!["claude", "gemini"]);
    assert!(again.approvals.is_empty());

    let fewer = sync(home.path(), &setup, &["claude"]);
    assert_eq!(fewer.hooked, vec!["claude"]);
    assert!(has(home.path(), "claude"));
    assert!(!has(home.path(), "gemini"));
}

#[test]
fn an_administrator_hook_replaces_the_user_one() {
    let home = tempfile::tempdir().unwrap();
    sync(home.path(), &setup(no_admin), &["claude"]);
    assert!(has(home.path(), "claude"));

    let synced = sync(home.path(), &setup(claude_admin), &["claude"]);
    assert_eq!(synced.hooked, vec!["claude"]);
    assert!(!has(home.path(), "claude"));
}

#[test]
fn a_settings_file_that_cannot_be_changed_is_a_warning() {
    let home = tempfile::tempdir().unwrap();
    for folder in [".claude", ".gemini"] {
        std::fs::create_dir_all(home.path().join(folder)).unwrap();
        std::fs::write(home.path().join(folder).join("settings.json"), "not json").unwrap();
    }
    let synced = sync(home.path(), &setup(no_admin), &["claude"]);
    assert!(synced.hooked.is_empty());
    assert_eq!(synced.warnings.len(), 2, "{:?}", synced.warnings);
    assert!(synced.warnings[0].contains("can't register the claude"));
    assert!(synced.warnings[1].contains("can't remove the gemini"));

    let (removed, warnings) = remove_all(home.path());
    assert!(removed.is_empty());
    assert_eq!(warnings.len(), 2);
}

#[test]
fn remove_all_removes_every_user_hook() {
    let home = tempfile::tempdir().unwrap();
    sync(home.path(), &setup(no_admin), &["claude", "gemini"]);
    let (removed, warnings) = remove_all(home.path());
    assert_eq!(removed, vec!["claude", "gemini"]);
    assert!(warnings.is_empty());
    assert!(!has(home.path(), "claude") && !has(home.path(), "gemini"));
    assert_eq!(remove_all(home.path()).0, Vec::<String>::new());
}

#[test]
fn admin_hooks_give_each_agent_its_entry_without_writing() {
    let hooks = admin_hooks(&setup(claude_admin));
    let agents: Vec<&str> = hooks.iter().map(|hook| hook.agent.as_str()).collect();
    assert_eq!(agents, vec!["claude", "gemini"]);
    assert!(hooks[0].present && !hooks[1].present);
    let text = hooks[1].entry.to_string();
    assert!(text.contains("SessionStart"), "{text}");
    assert!(text.contains("--hook fastskill-managed-gemini"), "{text}");
}

mod with_apply {
    use super::super::super::apply::ApplyResult;
    use super::super::super::apply_tests::{now, Fixture};
    use super::super::super::enrollment::{enroll, unenroll};
    use super::super::super::timer::{TimerOs, TimerSetup};
    use super::*;
    use serde_json::json;

    fn ok(_: &[String]) -> Result<(), String> {
        Ok(())
    }

    fn broken(_: &[String]) -> Result<(), String> {
        Err("no systemd".to_string())
    }

    fn absent(_: TimerOs) -> bool {
        false
    }

    fn timer(fixture: &Fixture, run: fn(&[String]) -> Result<(), String>) -> TimerSetup {
        TimerSetup {
            os: TimerOs::Linux,
            home: fixture.home(),
            exe: "/opt/fastskill".into(),
            user: "u".to_string(),
            run,
            system_present: absent,
        }
    }

    #[test]
    fn enrolling_registers_hooks_and_the_timer_and_unenroll_removes_them() {
        let mut fixture = Fixture::new();
        let pdf = fixture.skill("pdf", "one");
        fixture.publish(&[pdf], json!({}));
        let mut context = fixture.context();
        context.hooks = Some(setup(no_admin));
        context.timer = Some(timer(&fixture, ok));

        let first = fixture.apply_with(&context);
        assert!(first.enrolled_now && first.completed, "{first:?}");
        assert_eq!(first.hooks, vec!["claude"]);
        assert_eq!(first.hook_approvals.len(), 1);
        assert_eq!(
            first.timer.as_deref(),
            Some("systemd user timer fastskill-managed-apply.timer")
        );
        assert!(first.snapshot.agents.iter().all(|agent| agent.hook));
        assert!(has(&fixture.home(), "claude"));

        let again = fixture.apply_with(&context);
        assert_eq!(again.hooks, vec!["claude"]);
        assert!(again.timer.is_none() && again.hook_approvals.is_empty());

        let ApplyResult::Done(enrolled) = enroll(&context).unwrap() else {
            panic!("busy")
        };
        assert!(enrolled.timer.is_some());

        let removed = unenroll(
            &fixture.layout,
            &fixture.settings(),
            None,
            &fixture.home(),
            context.timer.as_ref(),
            now(),
        )
        .unwrap();
        assert_eq!(removed.removed_hooks, vec!["claude"]);
        assert!(removed.removed_timer && removed.warnings.is_empty());
        assert!(!has(&fixture.home(), "claude"));
    }

    #[test]
    fn a_timer_that_cannot_be_installed_or_removed_is_a_warning() {
        let mut fixture = Fixture::new();
        let pdf = fixture.skill("pdf", "one");
        fixture.publish(&[pdf], json!({}));
        let mut context = fixture.context();
        context.timer = Some(timer(&fixture, broken));
        let outcome = fixture.apply_with(&context);
        assert!(outcome.timer.is_none() && outcome.hooks.is_empty());
        assert!(
            outcome
                .warnings
                .iter()
                .any(|warning| warning.contains("can't install the timer")),
            "{:?}",
            outcome.warnings
        );
        assert!(!outcome.snapshot.agents.iter().any(|agent| agent.hook));

        let units = fixture.home().join(".config/systemd/user");
        std::fs::remove_dir_all(&units).unwrap();
        std::fs::create_dir_all(units.join("fastskill-managed-apply.timer/x")).unwrap();
        let removed = unenroll(
            &fixture.layout,
            &fixture.settings(),
            None,
            &fixture.home(),
            context.timer.as_ref(),
            now(),
        )
        .unwrap();
        assert!(!removed.removed_timer);
        assert!(removed.warnings[0].contains("can't remove the timer"));
    }
}
