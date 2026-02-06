//! Version command implementation

use crate::cli::error::CliResult;
use clap::Args;

/// Display FastSkill version
#[derive(Debug, Args)]
pub struct VersionArgs {}

pub async fn execute_version(_args: VersionArgs) -> CliResult<()> {
    println!("fastskill {}", fastskill::VERSION);
    Ok(())
}

#[allow(clippy::unwrap_used, clippy::expect_used)]
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_execute_version() {
        let args = VersionArgs {};
        let result = execute_version(args).await;
        assert!(result.is_ok());
    }
}
