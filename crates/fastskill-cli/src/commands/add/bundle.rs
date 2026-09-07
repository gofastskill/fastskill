//! Bundle detection and project-level installation for `fastskill add`.

use super::AddArgs;
use crate::error::{CliError, CliResult};
use crate::utils::SkillSource;
use fastskill_core::core::bundle::BundleService;
use fastskill_core::core::project::resolve_project_file;
use fastskill_core::FastSkillService;
use std::env;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

pub(super) async fn install_if_bundle(
    service: &FastSkillService,
    source: &SkillSource,
    args: &AddArgs,
    reindex: bool,
    no_reindex: bool,
) -> CliResult<bool> {
    match source {
        SkillSource::ZipFile(path) => {
            install_local_if_bundle(service, path, args, reindex, no_reindex).await
        }
        SkillSource::RemoteZipUrl(url) => {
            install_remote_if_bundle(service, url, args, reindex, no_reindex).await
        }
        _ => Ok(false),
    }
}

async fn install_remote_if_bundle(
    service: &FastSkillService,
    url: &str,
    args: &AddArgs,
    reindex: bool,
    no_reindex: bool,
) -> CliResult<bool> {
    if !url.starts_with("https://") {
        // Preserve the existing HTTP single-skill ZIP path. Bundle publication
        // intentionally probes only HTTPS URLs; private artifacts are first
        // obtained locally by an authenticated external tool.
        return Ok(false);
    }
    let response = reqwest::get(url)
        .await
        .map_err(|error| CliError::InvalidSource(format!("Failed to download '{url}': {error}")))?
        .error_for_status()
        .map_err(|error| CliError::InvalidSource(format!("Failed to download '{url}': {error}")))?;
    let bytes = response
        .bytes()
        .await
        .map_err(|error| CliError::InvalidSource(format!("Failed to read '{url}': {error}")))?;
    let temporary = TempDir::new().map_err(CliError::Io)?;
    let artifact = temporary.path().join("bundle.zip");
    fs::write(&artifact, bytes).map_err(CliError::Io)?;
    install_local_if_bundle(service, &artifact, args, reindex, no_reindex).await
}

async fn install_local_if_bundle(
    service: &FastSkillService,
    artifact: &Path,
    args: &AddArgs,
    reindex: bool,
    no_reindex: bool,
) -> CliResult<bool> {
    if !BundleService::is_bundle_artifact(artifact).map_err(CliError::Service)? {
        return Ok(false);
    }
    if args.force {
        return Err(CliError::Validation(
            "Use 'fastskill update --bundle <id> --from <artifact>' to replace a bundle release"
                .to_string(),
        ));
    }
    if args.group.is_some() {
        return Err(CliError::Validation(
            "--group applies to individual skills and cannot be used when adding a bundle"
                .to_string(),
        ));
    }
    let current = env::current_dir().map_err(|error| {
        CliError::Config(format!("Failed to determine current directory: {error}"))
    })?;
    let project = resolve_project_file(&current);
    let root = project.path.parent().ok_or_else(|| {
        CliError::Config("skill-project.toml has no project directory".to_string())
    })?;
    let outcome = BundleService::new(root, service.config().skill_storage_path.clone())
        .install(artifact)
        .map_err(CliError::Service)?;
    if outcome.unchanged {
        crate::outln!(
            "Bundle {}@{} is already installed",
            outcome.id,
            outcome.version
        );
    } else {
        crate::outln!("Installed bundle {}@{}", outcome.id, outcome.version);
        crate::outln!(
            "{}",
            crate::utils::messages::ok("Updated skill-project.toml and skills.lock")
        );
    }
    let auto_reindex = crate::config_file::load_auto_reindex_config();
    crate::utils::reindex_utils::maybe_auto_reindex(
        service,
        "add",
        reindex,
        no_reindex,
        auto_reindex,
        false,
    )
    .await?;
    Ok(true)
}
