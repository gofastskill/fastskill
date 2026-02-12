//! Extension-based directory validation (scripts/references allowed extensions).

use crate::validation::result::ValidationResult;
use std::path::Path;

/// Parameters for extension-based directory validation.
pub(crate) struct ExtensionCheckConfig<'a> {
    pub allowed_extensions: &'a [&'a str],
    pub field_label: &'a str,
    pub no_extension_message: Option<&'a str>,
}

/// Preset for scripts vs references directory validation.
pub(crate) enum ExtensionPreset {
    Scripts,
    References,
}

pub(crate) fn extension_config(preset: ExtensionPreset) -> ExtensionCheckConfig<'static> {
    match preset {
        ExtensionPreset::Scripts => ExtensionCheckConfig {
            allowed_extensions: &["py", "js", "ts", "sh", "bash", "rb", "go", "rs"],
            field_label: "scripts",
            no_extension_message: Some("Script file without extension:"),
        },
        ExtensionPreset::References => ExtensionCheckConfig {
            allowed_extensions: &["md", "txt", "json", "yaml", "yml", "csv", "tsv"],
            field_label: "references",
            no_extension_message: None,
        },
    }
}

pub(crate) fn process_file_extension(
    path: &Path,
    result: &mut ValidationResult,
    config: &ExtensionCheckConfig<'_>,
) {
    if !path.is_file() {
        return;
    }
    let Some(ext) = path.extension() else {
        if let Some(msg) = config.no_extension_message {
            *result = result
                .clone()
                .with_warning(config.field_label, &format!("{} {}", msg, path.display()));
        }
        return;
    };
    let ext_str = ext.to_string_lossy().to_lowercase();
    if !config.allowed_extensions.contains(&ext_str.as_str()) {
        *result = result.clone().with_warning(
            config.field_label,
            &format!("Unusual {} file extension: {}", config.field_label, ext_str),
        );
    }
}
