//! Utilities for auto-reindex after skill mutations

use crate::error::CliResult;
use fastskill_core::FastSkillService;

/// Run reindex if conditions are met; failures are non-fatal warnings.
pub async fn maybe_auto_reindex(
    service: &FastSkillService,
    command_name: &str,
    explicit_reindex: bool,
    explicit_no_reindex: bool,
    config_auto_reindex: bool,
    verbose: bool,
) -> CliResult<()> {
    if explicit_no_reindex {
        return Ok(());
    }

    if service.config().embedding.is_none() {
        if verbose {
            println!(
                "Note: skipping auto-reindex after '{}' (no embedding provider configured).",
                command_name
            );
        }
        return Ok(());
    }

    let should_reindex = explicit_reindex || config_auto_reindex;
    if !should_reindex {
        return Ok(());
    }

    let args = crate::commands::reindex::ReindexArgs {
        skills_dir: None,
        force: false,
        max_concurrent: 5,
        progress: false,
        no_progress: true,
    };

    if let Err(e) = crate::commands::reindex::execute_reindex(service, args).await {
        eprintln!(
            "Warning: auto-reindex after '{}' failed: {}",
            command_name, e
        );
    }

    Ok(())
}
