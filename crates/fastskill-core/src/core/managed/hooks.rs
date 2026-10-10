//! Session-start hooks for the covered agents (ADR-0016 decisions 16 and 17).
//!
//! Each covered agent that can run a command when a session starts gets a user-level hook
//! running `managed apply --hook <id>`, where `<id>` is [`hook_id`] for that agent. Every apply
//! registers hooks for newly covered agents and removes those of agents no longer covered. An
//! agent whose administrator-level settings already hold FastSkill's hook gets no user-level
//! one. `unenroll` removes them all.

use aikit_sdk::{HookChange, SessionHook};
use std::path::Path;

/// The prefix of every hook id; the agent key follows it.
pub const HOOK_ID_PREFIX: &str = "fastskill-managed-";

/// How to register hooks for one apply.
#[derive(Debug, Clone)]
pub struct HookSetup {
    /// The program part of the hook command, already quoted for the agent's shell.
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

/// The hook running `program` for agent `key`.
pub fn hook_for(program: &str, key: &str) -> SessionHook {
    let id = hook_id(key);
    SessionHook {
        command: format!("{program} managed apply --hook {id}"),
        id,
    }
}

/// Quote `path` as the program of a hook command.
pub fn quote_program(path: &Path) -> String {
    format!("\"{}\"", path.display().to_string().replace('"', "\\\""))
}

/// What registering hooks did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HookSync {
    /// Covered agents with a hook, user- or administrator-level.
    pub hooked: Vec<String>,
    /// Notes for hooks just added to an agent that needs the user to approve them.
    pub approvals: Vec<String>,
    /// Hooks that couldn't be registered or removed, with why.
    pub warnings: Vec<String>,
}

/// Register the hook for every covered agent that supports one, and remove FastSkill's
/// user-level hook from every other supported agent.
pub fn sync(home: &Path, setup: &HookSetup, covered: &[&str]) -> HookSync {
    let mut sync = HookSync::default();
    for key in aikit_sdk::session_hook_agents() {
        let id = hook_id(key);
        let admin = covered.contains(&key) && (setup.admin_present)(key, &id);
        if covered.contains(&key) && !admin {
            match aikit_sdk::register_session_hook(home, key, &setup.hook(key)) {
                Ok(change) => {
                    sync.hooked.push(key.to_string());
                    if change == HookChange::Added {
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

/// One agent's administrator-level hook: where it goes and what to merge there.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct AdminHook {
    pub agent: String,
    pub file: Option<std::path::PathBuf>,
    pub entry: serde_json::Value,
    pub present: bool,
}

/// The administrator-level hook entry for every supported agent, writing nothing.
pub fn admin_hooks(setup: &HookSetup) -> Vec<AdminHook> {
    aikit_sdk::session_hook_agents()
        .into_iter()
        .filter_map(|key| {
            let hook = setup.hook(key);
            let entry = aikit_sdk::admin_hook_entry(key, &hook).ok()?;
            Some(AdminHook {
                agent: key.to_string(),
                file: aikit_sdk::admin_hook_file(key),
                present: (setup.admin_present)(key, &hook.id),
                entry,
            })
        })
        .collect()
}

#[cfg(test)]
#[path = "hooks_tests.rs"]
mod tests;
