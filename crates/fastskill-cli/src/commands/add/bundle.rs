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
    if !should_probe_remote_bundle(url) {
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

fn should_probe_remote_bundle(url: &str) -> bool {
    url.starts_with("https://") || (cfg!(test) && url.starts_with("http://127.0.0.1"))
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
    let bundle_service = BundleService::new(root, service.config().skill_storage_path.clone());
    let preview = bundle_service
        .plan_install(artifact)
        .map_err(CliError::Service)?;
    if args.dry_run {
        if args.json {
            emit_bundle_add_json(&preview, true, None)?;
        } else if preview.changes.is_empty() {
            crate::outln!(
                "Bundle {}@{} is already installed; no changes were applied",
                preview.id,
                preview.target_revision
            );
        } else {
            crate::outln!(
                "Would install bundle {}@{}:",
                preview.id,
                preview.target_revision
            );
            for change in &preview.changes {
                crate::outln!("  {change}");
            }
            crate::outln!("No changes were applied");
        }
        return Ok(true);
    }
    let outcome = bundle_service
        .install(artifact)
        .map_err(CliError::Service)?;
    if args.json {
        let indexing = crate::utils::reindex_utils::lifecycle_reindex_result(
            service,
            "add bundle",
            reindex,
            no_reindex,
            crate::config_file::load_auto_reindex_config(),
        )
        .await;
        emit_bundle_add_json(&preview, false, Some(&indexing))?;
        return Ok(true);
    }
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

fn emit_bundle_add_json(
    preview: &fastskill_core::core::bundle::BundleInstallPreview,
    dry_run: bool,
    indexing: Option<&crate::utils::reindex_utils::LifecycleIndexResult>,
) -> CliResult<()> {
    let changed = !preview.changes.is_empty();
    crate::outln!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "scope": "project",
            "outcome": if changed { "changed" } else { "unchanged" },
            "dry_run": dry_run,
            "targets": [{
                "id": preview.id,
                "outcome": if changed { "changed" } else { "unchanged" },
                "current_revision": preview.current_revision,
                "target_revision": preview.target_revision,
                "changes": preview.changes,
                "retained": preview.retained,
            }],
            "diagnostics": Vec::<String>::new(),
            "indexing": indexing,
        }))
        .map_err(|error| CliError::Config(format!(
            "Failed to serialize bundle add result: {error}"
        )))?
    );
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use fastskill_core::ServiceConfig;
    use std::io::{Cursor, Write};
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};
    use zip::write::SimpleFileOptions;

    fn args() -> AddArgs {
        AddArgs {
            source: "fixture".to_string(),
            source_type: None,
            repository: None,
            branch: None,
            tag: None,
            force: false,
            editable: false,
            group: None,
            recursive: false,
            reindex: false,
            no_reindex: true,
            offline: false,
            dry_run: true,
            json: false,
        }
    }

    async fn service(root: &Path) -> FastSkillService {
        let mut service = FastSkillService::new(ServiceConfig {
            skill_storage_path: root.join("skills"),
            ..Default::default()
        })
        .await
        .unwrap();
        service.initialize().await.unwrap();
        service
    }

    fn ordinary_skill_zip() -> Vec<u8> {
        let cursor = Cursor::new(Vec::new());
        let mut writer = zip::ZipWriter::new(cursor);
        writer
            .start_file("SKILL.md", SimpleFileOptions::default())
            .unwrap();
        writer
            .write_all(b"---\nname: ordinary\nversion: 1.0.0\n---\n# ordinary\n")
            .unwrap();
        writer.finish().unwrap().into_inner()
    }

    #[tokio::test]
    async fn bundle_detection_skips_other_sources_and_non_https_remote_urls() {
        let temp = TempDir::new().unwrap();
        let service = service(temp.path()).await;
        assert!(!install_if_bundle(
            &service,
            &SkillSource::Folder(temp.path().to_path_buf()),
            &args(),
            false,
            true,
        )
        .await
        .unwrap());
        assert!(!install_if_bundle(
            &service,
            &SkillSource::RemoteZipUrl("http://example.invalid/skill.zip".to_string()),
            &args(),
            false,
            true,
        )
        .await
        .unwrap());
        assert!(should_probe_remote_bundle(
            "https://example.invalid/bundle.zip"
        ));
    }

    #[tokio::test]
    async fn remote_bundle_probe_downloads_once_and_preserves_single_skill_zip_handling() {
        let temp = TempDir::new().unwrap();
        let service = service(temp.path()).await;
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/ordinary.zip"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(ordinary_skill_zip()))
            .mount(&server)
            .await;
        assert!(!install_remote_if_bundle(
            &service,
            &format!("{}/ordinary.zip", server.uri()),
            &args(),
            false,
            true,
        )
        .await
        .unwrap());

        Mock::given(method("GET"))
            .and(path("/missing.zip"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;
        assert!(install_remote_if_bundle(
            &service,
            &format!("{}/missing.zip", server.uri()),
            &args(),
            false,
            true,
        )
        .await
        .is_err());
    }
}
