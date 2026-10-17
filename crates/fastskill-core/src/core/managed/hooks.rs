//! Session-start hooks for the covered agents (ADR-0016 decisions 16 and 17).
//!
//! FastSkill hooks only the agents in [`HOOKED_AGENTS`]: Claude Code, Codex, Cursor and Pi. Each
//! covered one gets a user-level hook running `managed apply --hook <id>`, where `<id>` is
//! [`hook_id`] for that agent. Every apply registers hooks for newly covered agents and removes
//! FastSkill's hook from every other agent `aikit-sdk` supports, Gemini CLI included, which the
//! timer alone reaches. An agent whose administrator-level settings already hold FastSkill's
//! hook gets no user-level one. `unenroll` removes them all.

use aikit_sdk::{AdminHookFormat, HookChange, SessionHook};
use std::path::{Path, PathBuf};

/// The prefix of every hook id; the agent key follows it.
pub const HOOK_ID_PREFIX: &str = "fastskill-managed-";

/// The `aikit-sdk` keys of the agents FastSkill hooks at session start (third amendment).
pub const HOOKED_AGENTS: &[&str] = &["claude", "codex", "cursor", "pi"];

/// How to register hooks for one apply.
#[derive(Debug, Clone)]
pub struct HookSetup {
    /// The absolute path of the FastSkill program the hooks run.
    pub program: String,
    /// Whether the administrator-level settings of agent `key` hold the hook with id `id`.
    pub admin_present: fn(&str, &str) -> bool,
}

impl HookSetup {
    /// Hooks that run `program`, checking the real administrator-level settings.
    pub fn new(program: impl Into<String>) -> Self {
        Self {
            program: program.into(),
            admin_present: aikit_sdk::admin_hook_present,
        }
    }

    /// Hooks that run the executable at `path`.
    pub fn for_exe(path: &Path) -> Self {
        Self::new(path.to_string_lossy())
    }

    /// The hook for agent `key`.
    pub fn hook(&self, key: &str) -> SessionHook {
        hook_for(&self.program, key)
    }
}

/// The hook id for agent `key`.
pub fn hook_id(key: &str) -> String {
    format!("{HOOK_ID_PREFIX}{key}")
}

/// The agent key a hook id names, when it is one of FastSkill's.
pub fn agent_of(id: &str) -> Option<&str> {
    id.strip_prefix(HOOK_ID_PREFIX)
        .filter(|key| !key.is_empty())
}

/// The hook running `program managed apply --hook <id>` for agent `key`. `aikit-sdk` derives the
/// shell command agents store from these arguments.
pub fn hook_for(program: &str, key: &str) -> SessionHook {
    let id = hook_id(key);
    SessionHook::from_argv(id.clone(), program, ["managed", "apply", "--hook", &id])
}

/// The hooked agents `aikit-sdk` supports, in [`HOOKED_AGENTS`] order.
pub fn hooked_agents() -> Vec<&'static str> {
    let supported = aikit_sdk::session_hook_agents();
    HOOKED_AGENTS
        .iter()
        .copied()
        .filter(|key| supported.contains(key))
        .collect()
}

/// What registering hooks did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HookSync {
    /// Covered agents with a hook, user- or administrator-level.
    pub hooked: Vec<String>,
    /// Notes for hooks just added or changed in an agent that needs the user to approve them.
    pub approvals: Vec<String>,
    /// Hooks that couldn't be registered or removed, with why.
    pub warnings: Vec<String>,
}

/// Register the hook for every covered agent FastSkill hooks, and remove FastSkill's user-level
/// hook from every other agent `aikit-sdk` supports.
pub fn sync(home: &Path, setup: &HookSetup, covered: &[&str]) -> HookSync {
    let mut sync = HookSync::default();
    for key in aikit_sdk::session_hook_agents() {
        let id = hook_id(key);
        let wanted = HOOKED_AGENTS.contains(&key) && covered.contains(&key);
        let admin = wanted && (setup.admin_present)(key, &id);
        if wanted && !admin {
            match aikit_sdk::register_session_hook(home, key, &setup.hook(key)) {
                Ok(change) => {
                    sync.hooked.push(key.to_string());
                    // An agent that approves hooks asks again when one changes.
                    if matches!(change, HookChange::Added | HookChange::Updated) {
                        if let Some(note) = aikit_sdk::session_hook_support(key)
                            .and_then(|support| support.approval_note)
                        {
                            sync.approvals.push(format!("{key}: {note}"));
                        }
                    }
                }
                Err(error) => sync
                    .warnings
                    .push(format!("can't register the {key} session hook: {error}")),
            }
            continue;
        }
        if admin {
            sync.hooked.push(key.to_string());
        }
        if let Err(error) = aikit_sdk::unregister_session_hook(home, key, &id) {
            sync.warnings
                .push(format!("can't remove the {key} session hook: {error}"));
        }
    }
    sync
}

/// Remove FastSkill's user-level hook from every supported agent; the agents it was removed
/// from, and why any couldn't be.
pub fn remove_all(home: &Path) -> (Vec<String>, Vec<String>) {
    let mut removed = Vec::new();
    let mut warnings = Vec::new();
    for key in aikit_sdk::session_hook_agents() {
        match aikit_sdk::unregister_session_hook(home, key, &hook_id(key)) {
            Ok(HookChange::Removed) => removed.push(key.to_string()),
            Ok(_) => {}
            Err(error) => warnings.push(format!("can't remove the {key} session hook: {error}")),
        }
    }
    (removed, warnings)
}

/// One hooked agent's administrator-level hook: where it goes and what to add there.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct AdminHook {
    pub agent: String,
    /// The administrator file, or `None` when the agent has no administrator-level location.
    pub file: Option<PathBuf>,
    /// `json` or `toml`, the administrator file's format; `None` without a location.
    pub format: Option<&'static str>,
    /// The fragment to merge into `file`; `None` without a location.
    pub text: Option<String>,
    pub present: bool,
}

/// The administrator-level hook entry for every hooked agent, writing nothing. An agent without
/// an administrator-level location (Pi) has no file, format or text; the machine-wide timer
/// covers it.
pub fn admin_hooks(setup: &HookSetup) -> Vec<AdminHook> {
    hooked_agents()
        .into_iter()
        .map(|key| {
            let hook = setup.hook(key);
            let entry = aikit_sdk::admin_hook_entry(key, &hook).ok();
            AdminHook {
                agent: key.to_string(),
                file: entry.as_ref().and_then(|_| aikit_sdk::admin_hook_file(key)),
                format: entry.as_ref().map(|entry| match entry.format {
                    AdminHookFormat::Json => "json",
                    AdminHookFormat::Toml => "toml",
                }),
                present: (setup.admin_present)(key, &hook.id),
                text: entry.map(|entry| entry.text),
            }
        })
        .collect()
}

#[cfg(test)]
#[path = "hooks_tests.rs"]
mod tests;
