use crate::core::bundle::{BundleDescriptor, BundleMemberPolicy, BUNDLE_FORMAT};
use crate::core::bundle_persistence::parse_bundle_project;
use crate::core::manifest::{DependencySpec, SkillProjectToml};
use crate::core::service::ServiceError;
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::Path;

pub(super) type PreparedBundleManifest =
    (BundleDescriptor, BTreeMap<String, DependencySpec>, Vec<u8>);

/// Derive a bundle from project identity and the complete effective dependency set.
pub(super) fn prepare_bundle_build_manifest(
    bytes: &[u8],
    project_root: &Path,
) -> Result<PreparedBundleManifest, ServiceError> {
    let content = std::str::from_utf8(bytes).map_err(|error| {
        ServiceError::Validation(format!("skill-project.toml is not UTF-8: {error}"))
    })?;
    let project = SkillProjectToml::from_toml_str(content).map_err(|error| {
        ServiceError::Validation(format!("Invalid skill-project.toml: {error}"))
    })?;
    let Some(metadata) = project.metadata.as_ref() else {
        if has_legacy_bundle_table(content) {
            let (descriptor, dependencies) = parse_bundle_project(bytes)?;
            return Ok((descriptor, dependencies, bytes.to_vec()));
        }
        return Err(ServiceError::Validation(
            "Bundle build requires [metadata] with id and version in skill-project.toml"
                .to_string(),
        ));
    };
    let id = required_metadata(&metadata.id, "id")?;
    let version = required_metadata(&metadata.version, "version")?;
    let mut dependencies: BTreeMap<_, _> = project
        .dependencies
        .as_ref()
        .map(|section| {
            section
                .dependencies
                .iter()
                .map(|(id, dependency)| (id.clone(), dependency.clone()))
                .collect()
        })
        .unwrap_or_default();
    for entry in project
        .to_skill_entries(project_root)
        .map_err(ServiceError::Validation)?
    {
        dependencies
            .entry(entry.id)
            .or_insert(DependencySpec::Inline {
                origin: entry.origin,
                groups: (!entry.groups.is_empty()).then_some(entry.groups),
            });
    }
    if dependencies.is_empty() {
        return Err(ServiceError::Validation(
            "Bundle build requires at least one [dependencies] entry".to_string(),
        ));
    }
    let descriptor = BundleDescriptor {
        format_marker: BUNDLE_FORMAT.to_string(),
        id,
        version,
        members: dependencies
            .keys()
            .map(|id| (id.clone(), BundleMemberPolicy::default()))
            .collect(),
    };
    descriptor.validate(&dependencies)?;
    let archive = archive_manifest(content, &descriptor, &dependencies)?;
    Ok((descriptor, dependencies, archive))
}

fn required_metadata(value: &Option<String>, field: &str) -> Result<String, ServiceError> {
    value
        .clone()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            ServiceError::Validation(format!("Bundle build requires [metadata].{field}"))
        })
}

fn has_legacy_bundle_table(content: &str) -> bool {
    toml::from_str::<toml::Value>(content)
        .ok()
        .and_then(|value| value.get("bundle").cloned())
        .is_some()
}

fn archive_manifest(
    content: &str,
    descriptor: &BundleDescriptor,
    dependencies: &BTreeMap<String, DependencySpec>,
) -> Result<Vec<u8>, ServiceError> {
    let mut document = content.parse::<toml_edit::DocumentMut>().map_err(|error| {
        ServiceError::Validation(format!("Invalid skill-project.toml: {error}"))
    })?;
    let dependency_document = serialized_document(&BundleArchiveDependencies {
        dependencies: dependencies.clone(),
    })?;
    document["dependencies"] = dependency_document["dependencies"].clone();
    document.remove("bundle");
    let descriptor_document = serialized_document(&BundleArchiveDescriptor {
        bundle: descriptor.clone(),
    })?;
    document["bundle"] = descriptor_document["bundle"].clone();
    Ok(document.to_string().into_bytes())
}

pub(super) fn serialized_document(
    value: &impl Serialize,
) -> Result<toml_edit::DocumentMut, ServiceError> {
    let serialized = toml::to_string(value).map_err(|error| {
        ServiceError::Config(format!("Failed to serialize bundle manifest: {error}"))
    })?;
    serialized.parse().map_err(|error| {
        ServiceError::Config(format!("Failed to construct bundle manifest: {error}"))
    })
}

#[derive(Serialize)]
struct BundleArchiveDescriptor {
    bundle: BundleDescriptor,
}

#[derive(Serialize)]
struct BundleArchiveDependencies {
    dependencies: BTreeMap<String, DependencySpec>,
}
