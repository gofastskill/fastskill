use crate::error::{CliError, CliResult};
use std::io::{self, BufRead, Write};

/// Prompt the user for confirmation unless forced by the caller.
pub(crate) fn confirm_removal(skill_ids: &[String], force: bool) -> CliResult<()> {
    if force {
        return Ok(());
    }
    confirm_removal_with_io(skill_ids, io::stdin().lock(), io::stdout().lock())
}

fn confirm_removal_with_io(
    skill_ids: &[String],
    mut input: impl BufRead,
    mut output: impl Write,
) -> CliResult<()> {
    writeln!(
        output,
        "[WARNING] This will permanently remove the following skills:"
    )
    .map_err(CliError::Io)?;
    for skill_id in skill_ids {
        writeln!(output, "  - {skill_id}").map_err(CliError::Io)?;
    }
    write!(output, "Are you sure you want to continue? (y/n): ").map_err(CliError::Io)?;
    output.flush().map_err(CliError::Io)?;

    let mut response = String::new();
    input.read_line(&mut response).map_err(CliError::Io)?;
    let response = response.trim().to_lowercase();
    if response == "yes" || response == "y" {
        Ok(())
    } else {
        Err(CliError::Validation(
            "Removal cancelled; no changes were applied".to_string(),
        ))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn force_skips_the_prompt_and_interactive_answers_are_parsed() {
        confirm_removal(&["demo".to_string()], true).unwrap();

        for (answer, expected) in [("y\n", true), ("YES\n", true), ("n\n", false), ("", false)] {
            let mut output = Vec::new();
            let accepted = confirm_removal_with_io(
                &["demo".to_string(), "other".to_string()],
                std::io::Cursor::new(answer),
                &mut output,
            );
            assert_eq!(accepted.is_ok(), expected);
            if let Err(error) = accepted {
                assert_eq!(error.exit_code(), 1);
                assert!(error.to_string().contains("Removal cancelled"));
            }
            let prompt = String::from_utf8(output).unwrap();
            assert!(prompt.contains("demo"));
            assert!(prompt.contains("other"));
            assert!(prompt.contains("(y/n)"));
        }
    }
}
