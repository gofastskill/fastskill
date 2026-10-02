//! Run a credential command and read the token it prints (ADR-0018).
//!
//! The program runs directly, with no shell, from FastSkill's config
//! directory. Its first line of standard output is the token. Nothing it
//! prints is logged, and no error message ever contains the token.

use super::credential::SecretToken;
use crate::core::service::ServiceError;
use std::io::IsTerminal;
use std::path::PathBuf;
use std::process::{ExitStatus, Stdio};
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::process::Command;

/// How long a credential command may run before it is stopped.
pub const COMMAND_TIMEOUT: Duration = Duration::from_secs(30);

/// The most standard output a credential command may print.
pub const MAX_COMMAND_OUTPUT: usize = 16 * 1024;

/// Set to `0` in the command's environment when FastSkill cannot prompt:
/// standard input or standard error is not a terminal.
pub const INTERACTIVE_ENV: &str = "FASTSKILL_INTERACTIVE";

/// How a credential command is run. [`CommandSettings::detect`] gives the
/// settings FastSkill uses; tests build their own.
#[derive(Debug, Clone)]
pub struct CommandSettings {
    pub timeout: Duration,
    pub working_dir: PathBuf,
    pub stdin_is_terminal: bool,
    pub stderr_is_terminal: bool,
}

impl CommandSettings {
    /// The real settings: a 30 second timeout, FastSkill's config directory,
    /// and this process's own terminals.
    pub fn detect() -> Result<Self, ServiceError> {
        let working_dir =
            crate::core::repository::user_config::user_config_dir().ok_or_else(|| {
                ServiceError::Config(
                    "Cannot run a credential command: FastSkill's config directory is \
                     unknown on this platform"
                        .to_string(),
                )
            })?;
        Ok(Self {
            timeout: COMMAND_TIMEOUT,
            working_dir,
            stdin_is_terminal: std::io::stdin().is_terminal(),
            stderr_is_terminal: std::io::stderr().is_terminal(),
        })
    }

    fn interactive(&self) -> bool {
        self.stdin_is_terminal && self.stderr_is_terminal
    }
}

enum Outcome {
    Finished(Vec<u8>, ExitStatus),
    TooLarge,
}

/// Run `argv` and return the first line it prints as the token.
pub async fn run_credential_command(
    argv: &[String],
    settings: &CommandSettings,
) -> Result<SecretToken, ServiceError> {
    let Some((program, args)) = argv.split_first() else {
        return Err(ServiceError::Config(
            "Credential command is empty; set command = [\"program\", \"arg\", ...]".to_string(),
        ));
    };
    let failed =
        |what: String| ServiceError::Custom(format!("Credential command '{program}' {what}"));

    std::fs::create_dir_all(&settings.working_dir).map_err(|error| {
        failed(format!(
            "could not run: cannot create {}: {error}",
            settings.working_dir.display()
        ))
    })?;

    let mut command = Command::new(program);
    command
        .args(args)
        .current_dir(&settings.working_dir)
        .stdout(Stdio::piped())
        .kill_on_drop(true);
    if settings.interactive() {
        command.stdin(Stdio::inherit());
    } else {
        command.stdin(Stdio::null()).env(INTERACTIVE_ENV, "0");
    }
    if settings.stderr_is_terminal {
        command.stderr(Stdio::inherit());
    } else {
        command.stderr(Stdio::null());
    }

    let mut child = command
        .spawn()
        .map_err(|error| failed(format!("could not be started: {error}")))?;
    let Some(mut stdout) = child.stdout.take() else {
        return Err(failed("has no standard output".to_string()));
    };

    let run = async {
        let mut output = Vec::new();
        (&mut stdout)
            .take(MAX_COMMAND_OUTPUT as u64 + 1)
            .read_to_end(&mut output)
            .await?;
        if output.len() > MAX_COMMAND_OUTPUT {
            return Ok::<_, std::io::Error>(Outcome::TooLarge);
        }
        let status = child.wait().await?;
        Ok(Outcome::Finished(output, status))
    };

    let outcome = tokio::time::timeout(settings.timeout, run).await;
    let (output, status) = match outcome {
        Err(_) => {
            stop(&mut child).await;
            return Err(failed(format!(
                "did not finish within {}s and was stopped",
                settings.timeout.as_secs_f32()
            )));
        }
        Ok(Err(error)) => {
            stop(&mut child).await;
            return Err(failed(format!("could not be read: {error}")));
        }
        Ok(Ok(Outcome::TooLarge)) => {
            stop(&mut child).await;
            return Err(failed(format!(
                "printed more than {} KiB; it must print only the token",
                MAX_COMMAND_OUTPUT / 1024
            )));
        }
        Ok(Ok(Outcome::Finished(output, status))) => (output, status),
    };

    if !status.success() {
        return Err(failed(format!("failed ({status})")));
    }
    first_line_token(output)
        .ok_or_else(|| failed("printed no token".to_string()))?
        .map_err(|()| failed("printed a token that is not valid UTF-8".to_string()))
}

async fn stop(child: &mut tokio::process::Child) {
    let _ = child.start_kill();
    let _ = child.wait().await;
}

/// The first line of `output` with trailing CR/LF removed, or `None` when
/// it is empty. `Err` when it is not UTF-8.
fn first_line_token(output: Vec<u8>) -> Option<Result<SecretToken, ()>> {
    let line_end = output
        .iter()
        .position(|byte| *byte == b'\n')
        .unwrap_or(output.len());
    let mut line = output;
    line.truncate(line_end);
    while line
        .last()
        .is_some_and(|byte| *byte == b'\r' || *byte == b'\n')
    {
        line.pop();
    }
    if line.iter().all(u8::is_ascii_whitespace) {
        return None;
    }
    Some(
        String::from_utf8(line)
            .map(SecretToken::new)
            .map_err(|_| ()),
    )
}

#[cfg(test)]
#[path = "credential_command_tests.rs"]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests;
