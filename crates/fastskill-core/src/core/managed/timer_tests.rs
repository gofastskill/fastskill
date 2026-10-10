#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::*;
use std::sync::Mutex;

static RAN: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn record(args: &[String]) -> Result<(), String> {
    RAN.lock().unwrap().push(args.join(" "));
    Ok(())
}

fn fail(args: &[String]) -> Result<(), String> {
    Err(format!("no {}", args[0]))
}

fn absent(_: TimerOs) -> bool {
    false
}

fn present(_: TimerOs) -> bool {
    true
}

fn setup(os: TimerOs, home: &Path, run: CommandRunner) -> TimerSetup {
    TimerSetup {
        os,
        home: home.to_path_buf(),
        exe: PathBuf::from("/opt/fastskill"),
        user: "a b".to_string(),
        run,
        system_present: absent,
    }
}

#[test]
fn linux_installs_and_removes_a_systemd_user_timer() {
    let home = tempfile::tempdir().unwrap();
    let setup = setup(TimerOs::Linux, home.path(), record);
    let installed = install(&setup).unwrap();
    assert_eq!(
        installed.as_deref(),
        Some("systemd user timer fastskill-managed-apply.timer")
    );
    let dir = home.path().join(".config/systemd/user");
    let service = std::fs::read_to_string(dir.join("fastskill-managed-apply.service")).unwrap();
    assert!(service.contains("ExecStart=\"/opt/fastskill\" managed apply\n"));
    let timer = std::fs::read_to_string(dir.join("fastskill-managed-apply.timer")).unwrap();
    for line in [
        "OnStartupSec=2min",
        "OnActiveSec=2min",
        "OnUnitActiveSec=1h",
        "RandomizedDelaySec=600",
        "WantedBy=timers.target",
    ] {
        assert!(timer.contains(line), "{line}: {timer}");
    }

    assert!(remove(&setup).unwrap());
    assert!(!dir.join("fastskill-managed-apply.timer").exists());
    assert!(!remove(&setup).unwrap());
    let ran = RAN.lock().unwrap().clone();
    assert!(ran.contains(&"systemctl --user enable --now fastskill-managed-apply.timer".into()));
    assert!(ran.contains(&"systemctl --user disable --now fastskill-managed-apply.timer".into()));
}

#[test]
fn macos_runs_apply_with_its_own_delay_from_a_launch_agent() {
    let home = tempfile::tempdir().unwrap();
    let setup = setup(TimerOs::MacOs, home.path(), record);
    let plan = install_plan(&setup);
    let plist = &plan.write[0];
    assert_eq!(
        plist.path,
        home.path()
            .join("Library/LaunchAgents/dev.fastskill.managed-apply.plist")
    );
    for part in [
        "<string>/opt/fastskill</string>",
        "<string>--timer</string>",
        "<key>RunAtLoad</key>\n\t<true/>",
        "<integer>3600</integer>",
    ] {
        assert!(plist.contents.contains(part), "{part}");
    }
    assert!(!plan.commands[0].required && plan.commands[1].required);
    assert_eq!(
        install(&setup).unwrap().as_deref(),
        Some("launchd agent dev.fastskill.managed-apply")
    );
    assert!(remove(&setup).unwrap());
    assert_eq!(remove_plan(&setup).commands[0].args[1], "unload");
}

#[test]
fn windows_registers_a_task_from_a_utf16_definition() {
    let home = tempfile::tempdir().unwrap();
    let setup = setup(TimerOs::Windows, home.path(), record);
    let plan = install_plan(&setup);
    let task = &plan.write[0].contents;
    for part in [
        "<LogonTrigger><Enabled>true</Enabled><UserId>a b</UserId><Delay>PT10M</Delay>",
        "<Interval>PT1H</Interval>",
        "<RandomDelay>PT10M</RandomDelay>",
        "<Command>/opt/fastskill</Command>",
    ] {
        assert!(task.contains(part), "{part}");
    }
    assert_eq!(
        plan.commands[0].args[..4],
        args(&[
            "schtasks",
            "/Create",
            "/TN",
            r"\FastSkill\managed-apply-a-b"
        ])
    );
    assert_eq!(
        install(&setup).unwrap().as_deref(),
        Some(r"scheduled task \FastSkill\managed-apply-a-b")
    );
    let bytes = std::fs::read(&plan.write[0].path).unwrap();
    assert_eq!(&bytes[..4], &[0xFF, 0xFE, b'<', 0]);
    assert!(remove(&setup).unwrap());
    assert_eq!(remove_plan(&setup).commands[0].args[1], "/Delete");
}

#[test]
fn a_machine_wide_timer_means_no_user_timer() {
    let home = tempfile::tempdir().unwrap();
    let mut setup = setup(TimerOs::Linux, home.path(), fail);
    setup.system_present = present;
    assert_eq!(install(&setup).unwrap(), None);
    assert!(!home.path().join(".config").exists());
}

#[test]
fn a_required_command_that_fails_is_an_error_and_optional_ones_are_not() {
    let home = tempfile::tempdir().unwrap();
    let setup = setup(TimerOs::Linux, home.path(), fail);
    let error = install(&setup).unwrap_err();
    assert!(
        error.contains("`systemctl --user daemon-reload` failed: no systemctl"),
        "{error}"
    );
    assert!(remove(&setup).unwrap());
}

#[test]
fn files_that_cannot_be_written_or_removed_are_errors() {
    let home = tempfile::tempdir().unwrap();
    std::fs::write(home.path().join(".config"), "a file").unwrap();
    let setup = setup(TimerOs::Linux, home.path(), record);
    assert!(install(&setup).unwrap_err().contains("can't create"));

    let home = tempfile::tempdir().unwrap();
    let dir = home
        .path()
        .join("Library/LaunchAgents/dev.fastskill.managed-apply.plist");
    std::fs::create_dir_all(dir.join("inside")).unwrap();
    let setup = self::setup(TimerOs::MacOs, home.path(), record);
    assert!(install(&setup).unwrap_err().contains("can't write"));
    assert!(remove(&setup).unwrap_err().contains("can't remove"));
}

#[test]
fn the_machine_wide_plan_is_for_every_user() {
    let exe = Path::new("/usr/bin/fastskill");
    let linux = system_plan(TimerOs::Linux, exe);
    assert_eq!(
        linux.write[1].path,
        PathBuf::from("/etc/systemd/user/fastskill-managed-apply.timer")
    );
    assert_eq!(
        linux.commands[0].args.join(" "),
        "systemctl --global enable fastskill-managed-apply.timer"
    );
    let macos = system_plan(TimerOs::MacOs, exe);
    assert!(macos.write[0].path.starts_with("/Library/LaunchAgents"));
    let windows = system_plan(TimerOs::Windows, exe);
    assert!(windows.write[0]
        .contents
        .contains("<GroupId>S-1-5-32-545</GroupId>"));
    assert!(!windows.write[0].contents.contains("<UserId>"));
    assert!(!system_timer_present(TimerOs::Windows));
    assert!(!system_timer_present(TimerOs::MacOs));
}

#[test]
fn the_real_runner_reports_how_a_command_ended() {
    assert!(run_command(&[]).is_err());
    assert!(run_command(&args(&["fastskill-no-such-program"]))
        .unwrap_err()
        .contains("couldn't start"));
    #[cfg(unix)]
    {
        assert!(run_command(&args(&["true"])).is_ok());
        assert!(run_command(&args(&["false"]))
            .unwrap_err()
            .contains("exited with"));
    }
}

#[test]
fn the_current_setup_and_delay_stay_in_bounds() {
    let current = TimerSetup::current(Path::new("/home/u"), Path::new("/bin/fastskill"));
    assert_eq!(current.map(|setup| setup.os), TimerOs::current());
    for _ in 0..20 {
        assert!(random_delay().as_secs() <= MAX_DELAY_SECS);
    }
}
