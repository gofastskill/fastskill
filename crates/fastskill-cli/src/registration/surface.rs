//! Top-level help, global arguments, and environment metadata.

use cli_framework::prelude::AppBuilder;
use cli_framework::spec::arg_spec::{ArgKind, ArgSpec, ArgValueType, Cardinality};
use cli_framework::spec::command_tree::CommandPath;
use cli_framework::spec::EnvVarEntry;

const HELP_SECTIONS: &[&str] = &[
    "Skills and projects",
    "Sources and distribution",
    "Quality",
    "Operations",
];

/// Configure the command surface that is shared by every registered leaf.
pub fn configure(builder: AppBuilder) -> anyhow::Result<AppBuilder> {
    let builder = builder
        .with_help_section_order(HELP_SECTIONS)
        .with_builtin_command_namespace(&CommandPath::root_for("cli"))
        .global_flag(ArgSpec {
            name: "skills-dir",
            kind: ArgKind::Option,
            long: Some("skills-dir"),
            value_type: ArgValueType::String,
            cardinality: Cardinality::Optional,
            help: "Override the skills directory path",
            ..Default::default()
        })
        .global_flag(ArgSpec {
            name: "global",
            kind: ArgKind::Flag,
            long: Some("global"),
            value_type: ArgValueType::Bool,
            cardinality: Cardinality::Optional,
            help: "Use global skills directory (~/.config/fastskill/skills)",
            ..Default::default()
        })
        .global_flag(ArgSpec {
            name: "verbose",
            kind: ArgKind::Flag,
            long: Some("verbose"),
            short: Some('v'),
            value_type: ArgValueType::Bool,
            cardinality: Cardinality::Optional,
            help: "Enable verbose output",
            ..Default::default()
        });

    let builder = builder
        .register_env_var(EnvVarEntry {
            name: "FASTSKILL_CACHE_DIR",
            description: "Override the shared skill cache directory",
        })?
        .register_env_var(EnvVarEntry {
            name: "FASTSKILL_EMBEDDING_MODEL",
            description: "Override the configured embedding model",
        })?
        .register_env_var(EnvVarEntry {
            name: "FASTSKILL_NO_PROGRESS",
            description: "Disable progress indicators when set",
        })?
        .register_env_var(EnvVarEntry {
            name: "FORCE_COLOR",
            description: "Force ANSI color output when supported",
        })?
        .register_env_var(EnvVarEntry {
            name: "NO_COLOR",
            description: "Disable ANSI color output",
        })?
        .register_env_var(EnvVarEntry {
            name: "OPENAI_API_KEY",
            description: "API key for semantic search and embeddings",
        })?
        .register_env_var(EnvVarEntry {
            name: "OPENAI_BASE_URL",
            description: "Override the embedding provider base URL",
        })?
        .register_env_var(EnvVarEntry {
            name: "PAT_TOKEN",
            description: "Default token for authenticated HTTP registries",
        })?
        .register_env_var(EnvVarEntry {
            name: "REGISTRY_INDEX_PATH",
            description: "Override the local registry index path",
        })?
        .register_env_var(EnvVarEntry {
            name: "RUST_LOG",
            description: "Override the FastSkill logging filter",
        })?
        .register_env_var(EnvVarEntry {
            name: "XDG_CONFIG_HOME",
            description: "Override the global configuration directory on Unix",
        })?;

    Ok(builder)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn surface_configuration_builds() {
        configure(AppBuilder::new()).unwrap();
    }
}
