use crate::error::{CliError, CliResult};
use std::io::{self, BufRead, Write};

/// Prompt the user for confirmation unless forced by the caller.
pub(super) fn confirm_removal(skill_ids: &[String], force: bool) -> CliResult<bool> {
    if force {
        return Ok(true);
    }
    confirm_removal_with_io(skill_ids, io::stdin().lock(), io::stdout().lock())
}

fn confirm_removal_with_io(
    skill_ids: &[String],
    mut input: impl BufRead,
    mut output: impl Write,
) -> CliResult<bool> {
    writeln!(
        output,
        "Warning: This will permanently remove the following skills:"
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
    Ok(response == "yes" || response == "y")
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn force_skips_the_prompt_and_interactive_answers_are_parsed() {
        assert!(confirm_removal(&["demo".to_string()], true).unwrap());

        for (answer, expected) in [("y\n", true), ("YES\n", true), ("n\n", false)] {
            let mut output = Vec::new();
            let accepted = confirm_removal_with_io(
                &["demo".to_string(), "other".to_string()],
                std::io::Cursor::new(answer),
                &mut output,
            )
            .unwrap();
            assert_eq!(accepted, expected);
            let prompt = String::from_utf8(output).unwrap();
            assert!(prompt.contains("demo"));
            assert!(prompt.contains("other"));
            assert!(prompt.contains("(y/n)"));
        }
    }
}
