//! Preservation-aware writes for the project Manifest.
//!
//! Lifecycle commands own a few well-known tables, while bundle declarations
//! and third-party extensions may share the same TOML document. This module
//! updates the owned values without serializing a partial typed model over the
//! complete Manifest.

use crate::core::manifest::{ManifestError, SkillProjectToml, MANIFEST_SCHEMA_VERSION};
use crate::utils::atomic_write;
use std::path::Path;

/// Persist the typed Manifest fields while retaining unrelated and unknown
/// tables already present in the document.
#[allow(clippy::expect_used)]
pub fn save_project_preserving(
    path: &Path,
    project: &SkillProjectToml,
) -> Result<(), ManifestError> {
    let mut stamped = project.clone();
    stamped.schema_version = Some(MANIFEST_SCHEMA_VERSION.to_string());
    let next = toml::Value::try_from(&stamped)
        .map_err(|error| ManifestError::Serialize(error.to_string()))?;
    let mut document = if path.exists() {
        let content = std::fs::read_to_string(path).map_err(ManifestError::Io)?;
        toml::from_str::<toml::Value>(&content)
            .map_err(|error| ManifestError::Parse(error.to_string()))?
    } else {
        toml::Value::Table(toml::Table::new())
    };

    // TOML documents and struct serialization always produce a root table.
    let document_table = document
        .as_table_mut()
        .expect("parsed TOML document root must be a table");
    let next_table = next
        .as_table()
        .expect("serialized Manifest root must be a table");

    if let Some(schema) = next_table.get("schema_version") {
        document_table.insert("schema_version".to_string(), schema.clone());
    }
    replace_owned_table(document_table, next_table, "dependencies");
    merge_owned_table(document_table, next_table, "metadata");
    merge_owned_table(document_table, next_table, "tool");

    let content = toml::to_string_pretty(&document)
        .map_err(|error| ManifestError::Serialize(error.to_string()))?;
    atomic_write(path, content.as_bytes()).map_err(ManifestError::Io)
}

fn replace_owned_table(current: &mut toml::Table, next: &toml::Table, key: &str) {
    match next.get(key) {
        Some(value) => {
            current.insert(key.to_string(), value.clone());
        }
        None => {
            current.remove(key);
        }
    }
}

fn merge_owned_table(current: &mut toml::Table, next: &toml::Table, key: &str) {
    let Some(next_value) = next.get(key) else {
        return;
    };
    match (current.get_mut(key), next_value) {
        (Some(toml::Value::Table(current_table)), toml::Value::Table(next_table)) => {
            merge_tables(current_table, next_table);
        }
        _ => {
            current.insert(key.to_string(), next_value.clone());
        }
    }
}

fn merge_tables(current: &mut toml::Table, next: &toml::Table) {
    for (key, next_value) in next {
        match (current.get_mut(key), next_value) {
            (Some(toml::Value::Table(current_table)), toml::Value::Table(next_table)) => {
                merge_tables(current_table, next_table);
            }
            _ => {
                current.insert(key.clone(), next_value.clone());
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::core::manifest::{DependenciesSection, DependencySpec};
    use std::collections::HashMap;
    use tempfile::TempDir;

    #[test]
    fn preserves_bundle_and_unknown_extension_tables_while_replacing_dependencies() {
        let temporary = TempDir::new().unwrap();
        let path = temporary.path().join("skill-project.toml");
        std::fs::write(
            &path,
            "[dependencies]\nold = \"1.0.0\"\n[bundles.team]\nversion = \"1.0.0\"\nartifact = \"team.zip\"\n[extension]\nvalue = \"keep\"\n[tool.fastskill]\nskills_directory = \"skills\"\n[tool.fastskill.extension]\nflag = true\n",
        )
        .unwrap();
        let mut project = SkillProjectToml::load_from_file(&path).unwrap();
        project.dependencies = Some(DependenciesSection {
            dependencies: HashMap::from([(
                "new".to_string(),
                DependencySpec::Version("2.0.0".to_string()),
            )]),
        });

        save_project_preserving(&path, &project).unwrap();

        let saved = std::fs::read_to_string(path).unwrap();
        assert!(saved.contains("[extension]"), "{saved}");
        assert!(saved.contains("value = \"keep\""), "{saved}");
        assert!(saved.contains("[bundles.team]"), "{saved}");
        assert!(saved.contains("[tool.fastskill.extension]"), "{saved}");
        assert!(saved.contains("flag = true"), "{saved}");
        let value: toml::Value = toml::from_str(&saved).unwrap();
        assert!(value["dependencies"].get("old").is_none());
        assert_eq!(value["dependencies"]["new"].as_str(), Some("2.0.0"));
    }

    #[test]
    fn creates_a_new_manifest_and_rejects_non_table_documents() {
        let temporary = TempDir::new().unwrap();
        let path = temporary.path().join("new.toml");
        let project = SkillProjectToml {
            schema_version: None,
            metadata: None,
            dependencies: Some(DependenciesSection {
                dependencies: HashMap::new(),
            }),
            tool: None,
        };
        save_project_preserving(&path, &project).unwrap();
        assert!(std::fs::read_to_string(&path)
            .unwrap()
            .contains("schema_version = \"1\""));

        std::fs::write(&path, "[invalid\n").unwrap();
        assert!(matches!(
            save_project_preserving(&path, &project),
            Err(ManifestError::Parse(_))
        ));
    }

    #[test]
    fn removes_owned_dependencies_and_repairs_a_non_table_owned_section() {
        let temporary = TempDir::new().unwrap();
        let source = temporary.path().join("source.toml");
        std::fs::write(
            &source,
            "[metadata]\nid = \"demo\"\nversion = \"1.0.0\"\ndescription = \"demo\"\n",
        )
        .unwrap();
        let project = SkillProjectToml::load_from_file(&source).unwrap();
        let target = temporary.path().join("target.toml");
        std::fs::write(
            &target,
            "metadata = \"invalid-owned-shape\"\n[dependencies]\nold = \"1.0.0\"\n[extension]\nkeep = true\n",
        )
        .unwrap();

        save_project_preserving(&target, &project).unwrap();

        let saved: toml::Value = toml::from_str(&std::fs::read_to_string(target).unwrap()).unwrap();
        assert!(saved.get("dependencies").is_none());
        assert_eq!(saved["metadata"]["id"].as_str(), Some("demo"));
        assert_eq!(saved["extension"]["keep"].as_bool(), Some(true));
    }
}
