#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::*;

fn setup(admin_present: fn(&str, &str) -> bool) -> HookSetup {
    HookSetup {
        program: "/opt/fast skill/fastskill".to_string(),
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

const ALL: &[&str] = &["claude", "codex", "cursor", "pi", "gemini", "copilot"];

#[test]
fn a_hook_names_its_agent_and_runs_apply_in_hook_mode() {
    let hook = setup(no_admin).hook("claude");
    assert_eq!(hook.id, "fastskill-managed-claude");
    assert_eq!(
        hook.argv.as_deref(),
        Some(
            &[
                "/opt/fast skill/fastskill",
                "managed",
                "apply",
                "--hook",
                "fastskill-managed-claude"
            ]
            .map(String::from)[..]
        )
    );
    if cfg!(unix) {
        assert_eq!(
            hook.command,
            "'/opt/fast skill/fastskill' managed apply --hook fastskill-managed-claude"
        );
    }
    assert_eq!(agent_of("fastskill-managed-gemini"), Some("gemini"));
    assert_eq!(agent_of("fastskill-managed-"), None);
    assert_eq!(agent_of("other"), None);
    assert_eq!(HookSetup::new("x").program, "x");
    assert_eq!(HookSetup::for_exe(Path::new("/bin/fs")).program, "/bin/fs");
    assert_eq!(hooked_agents(), HOOKED_AGENTS);
}

#[test]
fn sync_hooks_the_four_agents_and_no_other() {
    let home = tempfile::tempdir().unwrap();
    let setup = setup(no_admin);

    let first = sync(home.path(), &setup, ALL);
    assert_eq!(first.hooked, vec!["claude", "codex", "cursor", "pi"]);
    assert!(first.warnings.is_empty(), "{:?}", first.warnings);
    let noted: Vec<&str> = first
        .approvals
        .iter()
        .map(|note| note.split(':').next().unwrap())
        .collect();
    assert_eq!(noted, vec!["claude", "codex"]);
    for key in HOOKED_AGENTS {
        assert!(has(home.path(), key), "{key}");
    }
    assert!(!has(home.path(), "gemini"));

    let again = sync(home.path(), &setup, ALL);
    assert_eq!(again.hooked, first.hooked);
    assert!(again.approvals.is_empty());

    let fewer = sync(home.path(), &setup, &["claude", "pi"]);
    assert_eq!(fewer.hooked, vec!["claude", "pi"]);
    assert!(has(home.path(), "claude") && has(home.path(), "pi"));
    assert!(!has(home.path(), "codex") && !has(home.path(), "cursor"));
}

#[test]
fn sync_removes_a_gemini_hook_left_from_before() {
    let home = tempfile::tempdir().unwrap();
    let setup = setup(no_admin);
    aikit_sdk::register_session_hook(home.path(), "gemini", &setup.hook("gemini")).unwrap();
    assert!(has(home.path(), "gemini"));

    let synced = sync(home.path(), &setup, &["claude", "gemini"]);
    assert_eq!(synced.hooked, vec!["claude"]);
    assert!(synced.warnings.is_empty(), "{:?}", synced.warnings);
    assert!(!has(home.path(), "gemini"));
}

#[test]
fn a_changed_codex_hook_asks_for_approval_again() {
    let home = tempfile::tempdir().unwrap();
    sync(home.path(), &setup(no_admin), &["codex"]);
    let moved = HookSetup {
        program: "/usr/local/bin/fastskill".to_string(),
        admin_present: no_admin,
    };
    let synced = sync(home.path(), &moved, &["codex"]);
    assert_eq!(synced.approvals.len(), 1, "{:?}", synced.approvals);
    assert!(synced.approvals[0].starts_with("codex: "));
    assert!(sync(home.path(), &moved, &["codex"]).approvals.is_empty());
}

#[test]
fn an_administrator_hook_replaces_the_user_one() {
    let home = tempfile::tempdir().unwrap();
    sync(home.path(), &setup(no_admin), &["claude"]);
    assert!(has(home.path(), "claude"));

    let synced = sync(home.path(), &setup(claude_admin), &["claude"]);
    assert_eq!(synced.hooked, vec!["claude"]);
    assert!(synced.approvals.is_empty());
    assert!(!has(home.path(), "claude"));
}

#[test]
fn a_hook_file_that_cannot_be_changed_is_a_warning() {
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
fn a_codex_config_with_hooks_and_a_foreign_pi_file_are_warnings() {
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(home.path().join(".codex")).unwrap();
    let config = "[[hooks.SessionStart]]\nhooks = []\n";
    std::fs::write(home.path().join(".codex/config.toml"), config).unwrap();
    let extensions = home.path().join(".pi/agent/extensions");
    std::fs::create_dir_all(&extensions).unwrap();
    let mine = extensions.join("fastskill-managed-pi.ts");
    std::fs::write(&mine, "export default function () {}\n").unwrap();

    let synced = sync(home.path(), &setup(no_admin), &["codex", "pi"]);
    assert!(synced.hooked.is_empty(), "{:?}", synced.hooked);
    assert!(synced.approvals.is_empty());
    assert_eq!(synced.warnings.len(), 2, "{:?}", synced.warnings);
    assert!(synced.warnings[0].contains("can't register the codex"));
    assert!(synced.warnings[1].contains("can't register the pi"));
    assert_eq!(
        std::fs::read_to_string(home.path().join(".codex/config.toml")).unwrap(),
        config
    );
    assert!(!home.path().join(".codex/hooks.json").exists());
    assert_eq!(
        std::fs::read_to_string(&mine).unwrap(),
        "export default function () {}\n"
    );
    // Unenrolling leaves the file FastSkill didn't write in place too.
    remove_all(home.path());
    assert!(mine.exists());
}

#[test]
fn remove_all_removes_every_user_hook() {
    let home = tempfile::tempdir().unwrap();
    let setup = setup(no_admin);
    sync(home.path(), &setup, ALL);
    aikit_sdk::register_session_hook(home.path(), "gemini", &setup.hook("gemini")).unwrap();
    let (removed, warnings) = remove_all(home.path());
    assert_eq!(removed, vec!["claude", "gemini", "codex", "cursor", "pi"]);
    assert!(warnings.is_empty(), "{warnings:?}");
    for key in ["claude", "codex", "cursor", "gemini", "pi"] {
        assert!(!has(home.path(), key), "{key}");
    }
    assert_eq!(remove_all(home.path()).0, Vec::<String>::new());
}

#[test]
fn admin_hooks_give_each_hooked_agent_its_entry_without_writing() {
    let hooks = admin_hooks(&setup(claude_admin));
    let agents: Vec<&str> = hooks.iter().map(|hook| hook.agent.as_str()).collect();
    assert_eq!(agents, HOOKED_AGENTS);
    assert!(hooks[0].present && !hooks[1].present);
    let formats: Vec<Option<&str>> = hooks.iter().map(|hook| hook.format).collect();
    assert_eq!(
        formats,
        vec![Some("json"), Some("toml"), Some("json"), None]
    );
    let codex = hooks[1].text.as_deref().unwrap();
    assert!(codex.contains("[[hooks.SessionStart]]"), "{codex}");
    assert!(codex.contains("fastskill-managed-codex"), "{codex}");
    let cursor = hooks[2].text.as_deref().unwrap();
    assert!(cursor.contains("sessionStart"), "{cursor}");
    let pi = &hooks[3];
    assert!(pi.file.is_none() && pi.text.is_none() && !pi.present);
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
