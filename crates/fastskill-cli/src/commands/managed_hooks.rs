//! `fastskill managed apply --hook <id>` and `fastskill managed hooks`: the session-start hook
//! and what is registered (ADR-0016 decision 16).
//!
//! A hook starts an apply in the background and returns at once. It prints nothing unless the
//! last apply left something the user must act on, and then one notice in the agent's form.

use super::managed::{Environment, ManagedArgs};
use crate::error::CliResult;
use fastskill_core::core::managed::hooks::{self, agent_of, hook_id, quote_program, HookSetup};
use fastskill_core::core::managed::timer::{self, TimerOs};
use fastskill_core::core::managed::{self, ManagedStatus};
use serde_json::{json, Value};
use std::path::Path;
use std::process::{Command, Stdio};

/// Run as the hook with `id`: start an apply in the background when this user is enrolled or
/// would be, and return the notice to print, if any. Never fails.
pub fn run_hook(env: &Environment, id: &str) -> Option<String> {
    let agent = agent_of(id)?;
    if !env.settings.is_configured() {
        return None;
    }
    let status = managed::status(&env.layout, &env.settings, chrono::Utc::now()).ok()?;
    if status.enrollment.is_none() && !env.settings.source_from_system {
        return None;
    }
    // The per-user lock keeps this from overlapping a running apply.
    let _ = (env.background)(&env.exe);
    aikit_sdk::format_notice(agent, &notice(&status)?)
}

/// What the user must act on after the last apply, if anything.
pub fn notice(status: &ManagedStatus) -> Option<String> {
    status.enrollment.as_ref()?;
    if status.sign_in_needed {
        Some(
            "FastSkill: sign-in needed to update managed skills. Run `fastskill managed apply` \
             in a terminal."
                .to_string(),
        )
    } else if status.expired {
        Some(
            "FastSkill: the managed state has expired, so managed skills aren't updated. Run \
             `fastskill managed status`."
                .to_string(),
        )
    } else if status.subject.is_none() {
        Some(
            "FastSkill: no managed skills have been applied for this user yet. Run `fastskill \
             managed status`."
                .to_string(),
        )
    } else {
        None
    }
}

/// Start `managed apply` detached from this process, with no terminal.
pub fn start_background_apply(exe: &Path) -> std::io::Result<()> {
    let mut command = Command::new(exe);
    command
        .args(["managed", "apply"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        command.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }
    command.spawn().map(drop)
}

/// `managed hooks`: this user's hooks and timer, or with `--system` the administrator-level
/// hooks and the machine-wide timer. Writes nothing.
pub fn run_hooks(env: &Environment, args: &ManagedArgs) -> CliResult<String> {
    let setup = env
        .hooks
        .clone()
        .unwrap_or_else(|| HookSetup::new(quote_program(&env.exe)));
    let value = if args.system {
        system_json(&setup, TimerOs::current(), &env.exe)
    } else {
        user_json(env, &setup)
    };
    if args.json {
        return Ok(serde_json::to_string_pretty(&value).unwrap_or_default());
    }
    Ok(if args.system {
        render_system(&value)
    } else {
        render_user(&value)
    })
}

fn user_json(env: &Environment, setup: &HookSetup) -> Value {
    let agents: Vec<Value> = aikit_sdk::session_hook_agents()
        .into_iter()
        .map(|key| {
            let id = hook_id(key);
            let user = aikit_sdk::has_session_hook(&env.home, key, &id);
            json!({
                "agent": key,
                "user": user.as_ref().ok().copied().unwrap_or(false),
                "problem": user.err().map(|error| error.to_string()),
                "administrator": (setup.admin_present)(key, &id),
            })
        })
        .collect();
    let timer = env.timer.as_ref().map(|setup| {
        json!({
            "name": timer::describe(setup),
            "installed": timer::remove_plan(setup).remove.iter().any(|path| path.exists()),
            "machine_wide": (setup.system_present)(setup.os),
        })
    });
    json!({ "agents": agents, "timer": timer })
}

fn render_user(value: &Value) -> String {
    let mut lines = vec!["Session-start hooks:".to_string()];
    for agent in value["agents"].as_array().into_iter().flatten() {
        let name = agent["agent"].as_str().unwrap_or_default();
        let state = match (
            agent["administrator"].as_bool(),
            agent["user"].as_bool(),
            agent["problem"].as_str(),
        ) {
            (Some(true), _, _) => "administrator-level".to_string(),
            (_, Some(true), _) => "user-level".to_string(),
            (_, _, Some(problem)) => format!("unknown: {problem}"),
            _ => "none".to_string(),
        };
        lines.push(format!("  {name:<8} {state}"));
    }
    let timer = &value["timer"];
    lines.push(match timer["name"].as_str() {
        None => "Timer: none on this platform".to_string(),
        Some(_) if timer["machine_wide"] == true => "Timer: machine-wide".to_string(),
        Some(name) if timer["installed"] == true => format!("Timer: {name}"),
        Some(_) => "Timer: none; `fastskill managed enroll` installs it".to_string(),
    });
    lines.join("\n")
}

fn system_json(setup: &HookSetup, os: Option<TimerOs>, exe: &Path) -> Value {
    json!({
        "hooks": hooks::admin_hooks(setup),
        "timer": os.map(|os| timer::system_plan(os, exe)),
    })
}

fn render_system(value: &Value) -> String {
    let mut lines = Vec::new();
    for hook in value["hooks"].as_array().into_iter().flatten() {
        let file = hook["file"].as_str().unwrap_or("(no administrator file)");
        lines.push(format!(
            "{}: merge into {file}{}",
            hook["agent"].as_str().unwrap_or_default(),
            if hook["present"] == true {
                " (already present)"
            } else {
                ""
            }
        ));
        lines.push(serde_json::to_string_pretty(&hook["entry"]).unwrap_or_default());
        lines.push(String::new());
    }
    let plan = &value["timer"];
    if plan.is_null() {
        lines.push("Machine-wide timer: none on this platform".to_string());
        return lines.join("\n");
    }
    lines.push("Machine-wide timer:".to_string());
    for file in plan["write"].as_array().into_iter().flatten() {
        lines.push(format!("{}:", file["path"].as_str().unwrap_or_default()));
        lines.push(file["contents"].as_str().unwrap_or_default().to_string());
    }
    for command in plan["commands"].as_array().into_iter().flatten() {
        let args: Vec<&str> = command["args"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect();
        lines.push(format!("then run: {}", args.join(" ")));
    }
    lines.join("\n")
}

#[cfg(test)]
#[path = "managed_hooks_tests.rs"]
mod tests;
