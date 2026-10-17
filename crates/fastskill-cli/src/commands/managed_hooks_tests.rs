#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::*;
use fastskill_core::core::managed::timer::TimerSetup;
use fastskill_core::core::managed::{Enrollment, ManagedLayout, ManagedSettings, ManagedSource};
use std::sync::atomic::{AtomicUsize, Ordering};
use tempfile::TempDir;

static STARTED: AtomicUsize = AtomicUsize::new(0);

fn count(_: &Path) -> std::io::Result<()> {
    STARTED.fetch_add(1, Ordering::SeqCst);
    Ok(())
}

fn no_admin(_: &str, _: &str) -> bool {
    false
}

fn claude_admin(key: &str, _: &str) -> bool {
    key == "claude"
}

fn ok(_: &[String]) -> Result<(), String> {
    Ok(())
}

fn absent(_: TimerOs) -> bool {
    false
}

fn present(_: TimerOs) -> bool {
    true
}

fn environment(dir: &TempDir, configured: bool, from_system: bool) -> Environment {
    Environment {
        layout: ManagedLayout::new(dir.path().join("data")),
        settings: ManagedSettings {
            source: configured.then(|| ManagedSource::File(dir.path().join("state.dsse"))),
            source_from_system: from_system,
            ..ManagedSettings::default()
        },
        user_settings_file: None,
        home: dir.path().join("home"),
        project_skills: None,
        interactive: false,
        config_dir: dir.path().to_path_buf(),
        exe: dir.path().join("fastskill"),
        hooks: None,
        timer: None,
        background: count,
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

fn enrolled() -> ManagedStatus {
    ManagedStatus {
        enrollment: Some(Enrollment::new("file:/x", chrono::Utc::now())),
        subject: Some("team-a".to_string()),
        ..ManagedStatus::default()
    }
}

fn args(json: bool, system: bool) -> ManagedArgs {
    ManagedArgs {
        json,
        system,
        ..ManagedArgs::default()
    }
}

fn hook_setup(admin_present: fn(&str, &str) -> bool) -> HookSetup {
    HookSetup {
        program: "fastskill".to_string(),
        admin_present,
    }
}

#[test]
fn a_hook_starts_an_apply_and_prints_what_the_user_must_act_on() {
    let dir = TempDir::new().unwrap();
    let env = environment(&dir, true, false);
    enroll_record(&env);
    let before = STARTED.load(Ordering::SeqCst);
    let printed = run_hook(&env, "fastskill-managed-claude").unwrap();
    assert!(STARTED.load(Ordering::SeqCst) > before);
    let value: Value = serde_json::from_str(&printed).unwrap();
    assert!(value["systemMessage"]
        .as_str()
        .unwrap()
        .contains("no managed skills have been applied"));
}

#[test]
fn each_agent_gets_the_notice_in_its_own_form() {
    let dir = TempDir::new().unwrap();
    let env = environment(&dir, true, false);
    enroll_record(&env);
    let notice =
        notice(&managed::status(&env.layout, &env.settings, chrono::Utc::now()).unwrap()).unwrap();
    let printed = |agent: &str| run_hook(&env, &hook_id(agent)).unwrap();
    let field = |agent: &str, key: &str| {
        let value: Value = serde_json::from_str(&printed(agent)).unwrap();
        value[key].as_str().unwrap().to_string()
    };
    assert_eq!(field("claude", "systemMessage"), notice);
    assert_eq!(field("codex", "systemMessage"), notice);
    // Cursor passes the text to the model; Pi shows plain text in its interface.
    assert_eq!(field("cursor", "additional_context"), notice);
    assert_eq!(printed("pi"), notice);
}

#[test]
fn a_hook_does_nothing_when_there_is_nothing_to_do() {
    let dir = TempDir::new().unwrap();
    // Not one of FastSkill's ids, not configured, or not enrolled with a user source.
    assert_eq!(run_hook(&environment(&dir, true, true), "other"), None);
    assert_eq!(
        run_hook(&environment(&dir, false, true), "fastskill-managed-claude"),
        None
    );
    assert_eq!(
        run_hook(&environment(&dir, true, false), "fastskill-managed-claude"),
        None
    );

    // An administrator source enrolls by first apply, so the hook starts one and, with nothing
    // enrolled yet, prints nothing.
    let before = STARTED.load(Ordering::SeqCst);
    let env = environment(&dir, true, true);
    assert_eq!(run_hook(&env, "fastskill-managed-gemini"), None);
    assert!(STARTED.load(Ordering::SeqCst) > before);
}

#[test]
fn the_notice_names_what_to_do() {
    assert_eq!(notice(&ManagedStatus::default()), None);
    assert_eq!(notice(&enrolled()), None);
    let sign_in = ManagedStatus {
        sign_in_needed: true,
        expired: true,
        ..enrolled()
    };
    assert!(notice(&sign_in).unwrap().contains("sign-in needed"));
    let expired = ManagedStatus {
        expired: true,
        ..enrolled()
    };
    assert!(notice(&expired).unwrap().contains("has expired"));
    let none = ManagedStatus {
        subject: None,
        ..enrolled()
    };
    assert!(notice(&none).unwrap().contains("no managed skills"));
}

#[test]
fn a_background_apply_that_cannot_start_is_an_error() {
    let dir = TempDir::new().unwrap();
    assert!(start_background_apply(&dir.path().join("missing")).is_err());
}

#[cfg(unix)]
#[test]
fn a_background_apply_starts_the_program() {
    assert!(start_background_apply(Path::new("/bin/true")).is_ok());
}

#[test]
fn hooks_show_each_agent_and_the_timer() {
    let dir = TempDir::new().unwrap();
    let mut env = environment(&dir, true, false);
    let setup = hook_setup(no_admin);
    for agent in ["claude", "pi"] {
        aikit_sdk::register_session_hook(&env.home, agent, &setup.hook(agent)).unwrap();
    }
    std::fs::create_dir_all(env.home.join(".cursor")).unwrap();
    std::fs::write(env.home.join(".cursor/hooks.json"), "not json").unwrap();
    env.hooks = Some(setup);

    let text = run_hooks(&env, &args(false, false)).unwrap();
    assert!(text.contains("claude   user-level"), "{text}");
    assert!(text.contains("codex    none"), "{text}");
    assert!(text.contains("cursor   unknown: "), "{text}");
    assert!(text.contains("pi       user-level"), "{text}");
    // Gemini CLI is reached by the timer alone.
    assert!(!text.contains("gemini"), "{text}");
    assert!(text.contains("Timer: none on this platform"), "{text}");

    let mut timer = TimerSetup {
        os: TimerOs::Linux,
        home: env.home.clone(),
        exe: env.exe.clone(),
        user: "u".to_string(),
        run: ok,
        system_present: absent,
    };
    env.timer = Some(timer.clone());
    let text = run_hooks(&env, &args(false, false)).unwrap();
    assert!(
        text.contains("`fastskill managed enroll` installs it"),
        "{text}"
    );

    timer::install(&timer).unwrap();
    let value: Value = serde_json::from_str(&run_hooks(&env, &args(true, false)).unwrap()).unwrap();
    let agents: Vec<&str> = value["agents"]
        .as_array()
        .unwrap()
        .iter()
        .map(|agent| agent["agent"].as_str().unwrap())
        .collect();
    assert_eq!(agents, vec!["claude", "codex", "cursor", "pi"]);
    assert_eq!(value["agents"][0]["user"], true);
    assert!(value["agents"][2]["problem"].is_string());
    assert_eq!(value["timer"]["installed"], true);
    let text = run_hooks(&env, &args(false, false)).unwrap();
    assert!(
        text.contains("Timer: systemd user timer fastskill-managed-apply.timer"),
        "{text}"
    );

    timer.system_present = present;
    env.timer = Some(timer);
    env.hooks = Some(hook_setup(claude_admin));
    let text = run_hooks(&env, &args(false, false)).unwrap();
    assert!(text.contains("claude   administrator-level"), "{text}");
    assert!(text.contains("Timer: machine-wide"), "{text}");
}

#[test]
fn an_agent_without_a_settings_file_has_no_hook() {
    let dir = TempDir::new().unwrap();
    let mut env = environment(&dir, true, false);
    env.hooks = Some(hook_setup(no_admin));
    let text = run_hooks(&env, &args(false, false)).unwrap();
    assert!(text.contains("claude   none"), "{text}");
}

#[test]
fn system_hooks_print_what_an_administrator_installs() {
    let dir = TempDir::new().unwrap();
    let env = environment(&dir, true, false);
    let value: Value = serde_json::from_str(&run_hooks(&env, &args(true, true)).unwrap()).unwrap();
    let hooks = value["hooks"].as_array().unwrap();
    let agents: Vec<&str> = hooks
        .iter()
        .map(|hook| hook["agent"].as_str().unwrap())
        .collect();
    assert_eq!(agents, vec!["claude", "codex", "cursor", "pi"]);
    assert_eq!(value["timer"].is_null(), TimerOs::current().is_none());
    // Without a setup, the hooks run this executable.
    assert_eq!(hooks[0]["format"], "json");
    assert!(hooks[0]["text"]
        .as_str()
        .unwrap()
        .contains("managed apply --hook fastskill-managed-claude"));
    assert_eq!(hooks[1]["format"], "toml");
    assert!(hooks[1]["text"]
        .as_str()
        .unwrap()
        .contains("[[hooks.SessionStart]]"));
    assert!(hooks[3]["file"].is_null() && hooks[3]["text"].is_null());

    let setup = hook_setup(claude_admin);
    for os in [TimerOs::Linux, TimerOs::MacOs, TimerOs::Windows] {
        let text = render_system(&system_json(&setup, Some(os), &env.exe));
        assert!(text.contains("claude: merge into "), "{text}");
        assert!(text.contains("(already present)"), "{text}");
        assert!(text.contains("codex: add to "), "{text}");
        assert!(text.contains("[[hooks.SessionStart]]"), "{text}");
        assert!(text.contains("cursor: merge into "), "{text}");
        assert!(
            text.contains("pi: no administrator-level location; the machine-wide timer covers it"),
            "{text}"
        );
        assert!(!text.contains("gemini"), "{text}");
        assert!(text.contains("Machine-wide timer:\n"), "{text}");
        // A machine-wide launch agent loads at each user's next login.
        assert_eq!(text.contains("then run: "), os != TimerOs::MacOs, "{text}");
    }
    let text = render_system(&system_json(&setup, None, &env.exe));
    assert!(
        text.ends_with("Machine-wide timer: none on this platform"),
        "{text}"
    );
    assert!(!run_hooks(&env, &args(false, true)).unwrap().is_empty());
}
