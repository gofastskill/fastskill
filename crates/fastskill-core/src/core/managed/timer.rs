//! The per-user timer that runs `managed apply` at login and then hourly, each run shifted by
//! up to 10 minutes at random (ADR-0016 decision 16).
//!
//! - Linux: a systemd user timer, `~/.config/systemd/user/fastskill-managed-apply.timer`.
//! - macOS: a launchd agent, `~/Library/LaunchAgents/dev.fastskill.managed-apply.plist`. launchd
//!   can't shift runs, so it runs `managed apply --timer`, which waits first.
//! - Windows: a scheduled task, `\FastSkill\managed-apply-<user>`.
//!
//! What to write and run is planned by pure functions, so every platform's plan is testable
//! anywhere; [`install`] and [`remove`] carry a plan out with an injectable command runner.
//! When a machine-wide timer is already installed, no per-user one is.

use std::path::{Path, PathBuf};

/// The systemd unit name, without its suffix.
pub const UNIT_NAME: &str = "fastskill-managed-apply";
/// The launchd label.
pub const LAUNCHD_LABEL: &str = "dev.fastskill.managed-apply";
/// The scheduled task folder.
pub const TASK_FOLDER: &str = r"\FastSkill\";
/// The most a run is shifted, in seconds.
pub const MAX_DELAY_SECS: u64 = 600;

/// The platforms with a timer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TimerOs {
    Linux,
    MacOs,
    Windows,
}

impl TimerOs {
    /// This platform, when it has a timer.
    pub fn current() -> Option<Self> {
        if cfg!(target_os = "linux") {
            Some(Self::Linux)
        } else if cfg!(target_os = "macos") {
            Some(Self::MacOs)
        } else if cfg!(windows) {
            Some(Self::Windows)
        } else {
            None
        }
    }
}

/// A file a plan writes.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct TimerFile {
    pub path: PathBuf,
    pub contents: String,
}

/// A command a plan runs; a command that isn't `required` may fail.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct TimerCommand {
    pub args: Vec<String>,
    pub required: bool,
}

/// What installing or removing a timer writes, removes and runs, in that order.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct TimerPlan {
    pub write: Vec<TimerFile>,
    pub remove: Vec<PathBuf>,
    pub commands: Vec<TimerCommand>,
}

/// Runs one command; `Err` says why it failed.
pub type CommandRunner = fn(&[String]) -> Result<(), String>;

/// How to install or remove this user's timer.
#[derive(Debug, Clone)]
pub struct TimerSetup {
    pub os: TimerOs,
    pub home: PathBuf,
    /// The FastSkill executable the timer runs.
    pub exe: PathBuf,
    /// The user name, which keeps Windows task names apart.
    pub user: String,
    pub run: CommandRunner,
    /// Whether a machine-wide timer is installed.
    pub system_present: fn(TimerOs) -> bool,
}

impl TimerSetup {
    /// This user's timer on this platform, run with real commands.
    pub fn current(home: &Path, exe: &Path) -> Option<Self> {
        Some(Self {
            os: TimerOs::current()?,
            home: home.to_path_buf(),
            exe: exe.to_path_buf(),
            user: std::env::var("USERNAME")
                .or_else(|_| std::env::var("USER"))
                .unwrap_or_else(|_| "user".to_string()),
            run: run_command,
            system_present: system_timer_present,
        })
    }
}

fn args(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| item.to_string()).collect()
}

fn required(items: &[&str]) -> TimerCommand {
    TimerCommand {
        args: args(items),
        required: true,
    }
}

fn optional(items: &[&str]) -> TimerCommand {
    TimerCommand {
        args: args(items),
        required: false,
    }
}

fn quoted(path: &Path) -> String {
    format!("\"{}\"", path.display().to_string().replace('"', "\\\""))
}

fn systemd_service(exe: &Path) -> String {
    format!(
        "[Unit]\nDescription=Apply the FastSkill managed state\n\n[Service]\nType=oneshot\n\
         ExecStart={} managed apply\n",
        quoted(exe)
    )
}

fn systemd_timer(install_target: &str) -> String {
    format!(
        "[Unit]\nDescription=Apply the FastSkill managed state at login and hourly\n\n\
         [Timer]\nOnStartupSec=2min\nOnActiveSec=2min\nOnUnitActiveSec=1h\n\
         RandomizedDelaySec={}\n\n[Install]\nWantedBy={install_target}\n",
        MAX_DELAY_SECS
    )
}

fn xml_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn launchd_plist(exe: &Path) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \
         \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <plist version=\"1.0\">\n<dict>\n\
         \t<key>Label</key>\n\t<string>{LAUNCHD_LABEL}</string>\n\
         \t<key>ProgramArguments</key>\n\t<array>\n\
         \t\t<string>{}</string>\n\t\t<string>managed</string>\n\t\t<string>apply</string>\n\
         \t\t<string>--timer</string>\n\t</array>\n\
         \t<key>RunAtLoad</key>\n\t<true/>\n\
         \t<key>StartInterval</key>\n\t<integer>3600</integer>\n\
         \t<key>ProcessType</key>\n\t<string>Background</string>\n\
         </dict>\n</plist>\n",
        xml_escape(&exe.display().to_string())
    )
}

/// The task definition: at logon of `user` (or any user when `None`) and hourly, each run
/// delayed by up to [`MAX_DELAY_SECS`].
fn task_xml(exe: &Path, user: Option<&str>) -> String {
    let delay = format!("PT{}M", MAX_DELAY_SECS / 60);
    let (logon_user, principal) = match user {
        Some(user) => (
            format!("<UserId>{}</UserId>", xml_escape(user)),
            format!(
                "<Principal id=\"Author\"><UserId>{}</UserId><LogonType>InteractiveToken</LogonType>\
                 <RunLevel>LeastPrivilege</RunLevel></Principal>",
                xml_escape(user)
            ),
        ),
        None => (
            String::new(),
            "<Principal id=\"Author\"><GroupId>S-1-5-32-545</GroupId>\
             <RunLevel>LeastPrivilege</RunLevel></Principal>"
                .to_string(),
        ),
    };
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-16\"?>\n\
         <Task version=\"1.2\" xmlns=\"http://schemas.microsoft.com/windows/2004/02/mit/task\">\n\
         <RegistrationInfo><Description>Apply the FastSkill managed state at logon and hourly\
         </Description></RegistrationInfo>\n\
         <Triggers>\n\
         <LogonTrigger><Enabled>true</Enabled>{logon_user}<Delay>{delay}</Delay></LogonTrigger>\n\
         <TimeTrigger><Enabled>true</Enabled><StartBoundary>2000-01-01T00:00:00</StartBoundary>\
         <Repetition><Interval>PT1H</Interval></Repetition><RandomDelay>{delay}</RandomDelay>\
         </TimeTrigger>\n\
         </Triggers>\n\
         <Principals>{principal}</Principals>\n\
         <Settings><MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>\
         <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>\
         <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>\
         <StartWhenAvailable>true</StartWhenAvailable><Hidden>true</Hidden></Settings>\n\
         <Actions Context=\"Author\"><Exec><Command>{}</Command><Arguments>managed apply\
         </Arguments></Exec></Actions>\n\
         </Task>\n",
        xml_escape(&exe.display().to_string())
    )
}

fn task_name(user: &str) -> String {
    let user: String = user
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    format!("{TASK_FOLDER}managed-apply-{user}")
}

fn systemd_user_dir(home: &Path) -> PathBuf {
    home.join(".config/systemd/user")
}

fn plist_path(home: &Path) -> PathBuf {
    home.join("Library/LaunchAgents")
        .join(format!("{LAUNCHD_LABEL}.plist"))
}

fn task_file(home: &Path) -> PathBuf {
    home.join("AppData/Local/fastskill/managed-apply-task.xml")
}

/// What installs this user's timer.
pub fn install_plan(setup: &TimerSetup) -> TimerPlan {
    let home = &setup.home;
    match setup.os {
        TimerOs::Linux => {
            let dir = systemd_user_dir(home);
            let timer = format!("{UNIT_NAME}.timer");
            TimerPlan {
                write: vec![
                    TimerFile {
                        path: dir.join(format!("{UNIT_NAME}.service")),
                        contents: systemd_service(&setup.exe),
                    },
                    TimerFile {
                        path: dir.join(&timer),
                        contents: systemd_timer("timers.target"),
                    },
                ],
                remove: Vec::new(),
                commands: vec![
                    required(&["systemctl", "--user", "daemon-reload"]),
                    required(&["systemctl", "--user", "enable", "--now", &timer]),
                ],
            }
        }
        TimerOs::MacOs => {
            let plist = plist_path(home).display().to_string();
            TimerPlan {
                write: vec![TimerFile {
                    path: plist_path(home),
                    contents: launchd_plist(&setup.exe),
                }],
                remove: Vec::new(),
                commands: vec![
                    optional(&["launchctl", "unload", &plist]),
                    required(&["launchctl", "load", "-w", &plist]),
                ],
            }
        }
        TimerOs::Windows => {
            let file = task_file(home);
            let path = file.display().to_string();
            TimerPlan {
                write: vec![TimerFile {
                    path: file,
                    contents: task_xml(&setup.exe, Some(&setup.user)),
                }],
                remove: Vec::new(),
                commands: vec![required(&[
                    "schtasks",
                    "/Create",
                    "/TN",
                    &task_name(&setup.user),
                    "/XML",
                    &path,
                    "/F",
                ])],
            }
        }
    }
}

/// What removes this user's timer.
pub fn remove_plan(setup: &TimerSetup) -> TimerPlan {
    let home = &setup.home;
    match setup.os {
        TimerOs::Linux => {
            let dir = systemd_user_dir(home);
            let timer = format!("{UNIT_NAME}.timer");
            TimerPlan {
                write: Vec::new(),
                remove: vec![dir.join(&timer), dir.join(format!("{UNIT_NAME}.service"))],
                commands: vec![
                    optional(&["systemctl", "--user", "disable", "--now", &timer]),
                    optional(&["systemctl", "--user", "daemon-reload"]),
                ],
            }
        }
        TimerOs::MacOs => TimerPlan {
            write: Vec::new(),
            remove: vec![plist_path(home)],
            commands: vec![optional(&[
                "launchctl",
                "unload",
                "-w",
                &plist_path(home).display().to_string(),
            ])],
        },
        TimerOs::Windows => TimerPlan {
            write: Vec::new(),
            remove: vec![task_file(home)],
            commands: vec![optional(&[
                "schtasks",
                "/Delete",
                "/TN",
                &task_name(&setup.user),
                "/F",
            ])],
        },
    }
}

/// The machine-wide timer an administrator installs for every user: the files to place and
/// the commands to run. Nothing is written.
pub fn system_plan(os: TimerOs, exe: &Path) -> TimerPlan {
    match os {
        TimerOs::Linux => {
            let dir = PathBuf::from("/etc/systemd/user");
            let timer = format!("{UNIT_NAME}.timer");
            TimerPlan {
                write: vec![
                    TimerFile {
                        path: dir.join(format!("{UNIT_NAME}.service")),
                        contents: systemd_service(exe),
                    },
                    TimerFile {
                        path: dir.join(&timer),
                        contents: systemd_timer("timers.target"),
                    },
                ],
                remove: Vec::new(),
                commands: vec![required(&["systemctl", "--global", "enable", &timer])],
            }
        }
        TimerOs::MacOs => TimerPlan {
            write: vec![TimerFile {
                path: PathBuf::from(format!("/Library/LaunchAgents/{LAUNCHD_LABEL}.plist")),
                contents: launchd_plist(exe),
            }],
            remove: Vec::new(),
            commands: Vec::new(),
        },
        TimerOs::Windows => {
            let file = PathBuf::from(r"C:\ProgramData\fastskill\managed-apply-task.xml");
            let path = file.display().to_string();
            TimerPlan {
                write: vec![TimerFile {
                    path: file,
                    contents: task_xml(exe, None),
                }],
                remove: Vec::new(),
                commands: vec![required(&[
                    "schtasks",
                    "/Create",
                    "/TN",
                    &format!("{TASK_FOLDER}managed-apply"),
                    "/XML",
                    &path,
                    "/F",
                ])],
            }
        }
    }
}

/// Whether the machine-wide timer is installed. Windows can't tell without querying the task
/// scheduler, so it answers `false`.
pub fn system_timer_present(os: TimerOs) -> bool {
    match os {
        TimerOs::Windows => false,
        _ => system_plan(os, Path::new("fastskill"))
            .write
            .iter()
            .any(|file| file.path.is_file()),
    }
}

/// Install this user's timer: `Ok(Some(what))` when it was, `Ok(None)` when a machine-wide one
/// is installed instead.
pub fn install(setup: &TimerSetup) -> Result<Option<String>, String> {
    if (setup.system_present)(setup.os) {
        return Ok(None);
    }
    carry_out(setup, &install_plan(setup))?;
    Ok(Some(describe(setup)))
}

/// Remove this user's timer; `true` when anything was there.
pub fn remove(setup: &TimerSetup) -> Result<bool, String> {
    let plan = remove_plan(setup);
    let present = plan.remove.iter().any(|path| path.exists());
    carry_out(setup, &plan)?;
    Ok(present)
}

/// What the timer is called on this platform.
pub fn describe(setup: &TimerSetup) -> String {
    match setup.os {
        TimerOs::Linux => format!("systemd user timer {UNIT_NAME}.timer"),
        TimerOs::MacOs => format!("launchd agent {LAUNCHD_LABEL}"),
        TimerOs::Windows => format!("scheduled task {}", task_name(&setup.user)),
    }
}

fn carry_out(setup: &TimerSetup, plan: &TimerPlan) -> Result<(), String> {
    for file in &plan.write {
        if let Some(parent) = file.path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("can't create {}: {error}", parent.display()))?;
        }
        let bytes = if setup.os == TimerOs::Windows {
            utf16_with_bom(&file.contents)
        } else {
            file.contents.clone().into_bytes()
        };
        std::fs::write(&file.path, bytes)
            .map_err(|error| format!("can't write {}: {error}", file.path.display()))?;
    }
    for path in &plan.remove {
        match std::fs::remove_file(path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("can't remove {}: {error}", path.display())),
        }
    }
    for command in &plan.commands {
        if let Err(error) = (setup.run)(&command.args) {
            if command.required {
                return Err(format!("`{}` failed: {error}", command.args.join(" ")));
            }
        }
    }
    Ok(())
}

/// The task scheduler reads task definitions as UTF-16.
fn utf16_with_bom(text: &str) -> Vec<u8> {
    let mut bytes = vec![0xFF, 0xFE];
    for unit in text.encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    bytes
}

/// Run a command without a terminal and wait for it.
pub fn run_command(args: &[String]) -> Result<(), String> {
    let Some((program, rest)) = args.split_first() else {
        return Err("empty command".to_string());
    };
    let status = std::process::Command::new(program)
        .args(rest)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map_err(|error| format!("couldn't start: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("exited with {status}"))
    }
}

/// A random delay of up to [`MAX_DELAY_SECS`], for a timer that can't shift runs itself.
pub fn random_delay() -> std::time::Duration {
    let random = uuid::Uuid::new_v4().as_u128() as u64;
    std::time::Duration::from_secs(random % (MAX_DELAY_SECS + 1))
}

#[cfg(test)]
#[path = "timer_tests.rs"]
mod tests;
