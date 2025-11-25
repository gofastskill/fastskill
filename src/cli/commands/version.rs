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
