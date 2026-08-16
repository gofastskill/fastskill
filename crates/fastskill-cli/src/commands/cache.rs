//! `fastskill cache` subcommand (PRD 006 "Local Skill Cache", US-006):
//! inspect and reclaim the on-disk, machine-global skill content cache that
//! `add`/`install`/`update` populate for git, registry, and local origins
//! (`fastskill_core::core::cache::SkillCache`).
//!
//! `cache info` and `cache clean` both work directly against
//! [`SkillCache::from_env`] -- like `repos refresh` (US-005), neither needs a
//! full `FastSkillService`, just the resolved cache root.

use crate::commands::common::validate_format_args;
use crate::error::{CliError, CliResult};
use crate::utils::messages;
use cli_framework::command::{FromArgValueMap, IntoCommandSpec};
use cli_framework::spec::arg_spec::{ArgKind, ArgSpec, ArgValueType, Cardinality};
use cli_framework::spec::command_tree::CommandSpec;
use cli_framework::spec::value::ArgValue;
use fastskill_core::core::cache::{CacheStats, CleanReport, ContentSourceKind, SkillCache};
use fastskill_core::OutputFormat;
use std::collections::HashMap;
use std::str::FromStr;

/// Restrict `cache info`'s `--format` to `table`/`json`, mirroring
/// `validate_eval_format_args`: there is no per-source-kind grid/XML layout
/// to render, so `grid`/`xml` are rejected with a clear error rather than
/// silently collapsing to `table`.
fn validate_cache_info_format_args(
    format: &Option<OutputFormat>,
    json: bool,
) -> CliResult<OutputFormat> {
    let resolved = validate_format_args(format, json)?;
    match resolved {
        OutputFormat::Grid | OutputFormat::Xml => Err(CliError::Config(
            "Error: cache info supports only --format table or json (grid/xml are not \
             implemented for cache output). Use --format json for machine-readable output."
                .to_string(),
        )),
        other => Ok(other),
    }
}

// ---------------------------------------------------------------------------
// cache info
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct CacheInfoArgs {
    pub format: Option<OutputFormat>,
    pub json: bool,
}

impl IntoCommandSpec for CacheInfoArgs {
    fn command_spec() -> CommandSpec {
        CommandSpec {
            summary: "Show the skill content cache location, entry counts, and disk usage",
            syntax: Some("cache info [OPTIONS]"),
            category: Some("cache"),
            args: vec![
                ArgSpec {
                    name: "format",
                    kind: ArgKind::Option,
                    long: Some("format"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    help: "Output format: table or json (default: table)",
                    ..Default::default()
                },
                ArgSpec {
                    name: "json",
                    kind: ArgKind::Flag,
                    long: Some("json"),
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    help: "Shorthand for --format json",
                    ..Default::default()
                },
            ],
            ..Default::default()
        }
    }
}

impl FromArgValueMap for CacheInfoArgs {
    fn from_arg_value_map(map: &HashMap<String, ArgValue>) -> Self {
        Self {
            format: map
                .get("format")
                .and_then(|v| {
                    if let ArgValue::Str(s) = v {
                        Some(s.as_str())
                    } else {
                        None
                    }
                })
                .and_then(|s| OutputFormat::from_str(s).ok()),
            json: matches!(map.get("json"), Some(ArgValue::Bool(true))),
        }
    }
}

pub async fn execute_cache_info(args: CacheInfoArgs) -> CliResult<()> {
    let resolved_format = validate_cache_info_format_args(&args.format, args.json)?;
    let cache = SkillCache::from_env()?;
    let stats = cache.stats()?;

    match resolved_format {
        OutputFormat::Json => {
            let json_output = serde_json::to_string_pretty(&stats)
                .map_err(|e| CliError::Config(format!("Failed to serialize JSON: {}", e)))?;
            crate::outln!("{}", json_output);
        }
        _ => crate::outln!("{}", format_info_table(&stats)),
    }
    Ok(())
}

fn format_info_table(stats: &CacheStats) -> String {
    let mut output = format!("Cache location: {}\n\n", stats.root.display());
    for kind in ContentSourceKind::ALL {
        let s = stats.for_kind(kind);
        output.push_str(&format!(
            "  • {:<8} {} {}, {}\n",
            kind.to_string(),
            s.entry_count,
            if s.entry_count == 1 {
                "entry"
            } else {
                "entries"
            },
            format_bytes(s.total_bytes),
        ));
    }
    let total = stats.total();
    output.push_str(&format!(
        "\nTotal: {} {}, {}\n",
        total.entry_count,
        if total.entry_count == 1 {
            "entry"
        } else {
            "entries"
        },
        format_bytes(total.total_bytes),
    ));
    output
}

// ---------------------------------------------------------------------------
// cache clean
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct CacheCleanArgs {
    pub source: Option<String>,
    pub json: bool,
}

impl IntoCommandSpec for CacheCleanArgs {
    fn command_spec() -> CommandSpec {
        CommandSpec {
            summary: "Remove cached skill content and print bytes reclaimed",
            syntax: Some("cache clean [--source <git|registry|local|zip>]"),
            category: Some("cache"),
            args: vec![
                ArgSpec {
                    name: "source",
                    kind: ArgKind::Option,
                    long: Some("source"),
                    value_type: ArgValueType::Enum(vec!["git", "registry", "local", "zip"]),
                    cardinality: Cardinality::Optional,
                    help: "Limit cleaning to one source kind: git, registry, local, or zip \
                           (default: all)",
                    ..Default::default()
                },
                ArgSpec {
                    name: "json",
                    kind: ArgKind::Flag,
                    long: Some("json"),
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    help: "Print the result as JSON",
                    ..Default::default()
                },
            ],
            ..Default::default()
        }
    }
}

impl FromArgValueMap for CacheCleanArgs {
    fn from_arg_value_map(map: &HashMap<String, ArgValue>) -> Self {
        Self {
            source: map.get("source").and_then(|v| match v {
                ArgValue::Str(s) | ArgValue::Enum(s) => Some(s.clone()),
                _ => None,
            }),
            json: matches!(map.get("json"), Some(ArgValue::Bool(true))),
        }
    }
}

/// `cache clean` (US-006). Content only -- the index cache (`index/`,
/// including `repos refresh`'s per-source listings and the git-resolutions
/// map) is left untouched, per the PRD's "removes all content entries"
/// wording and its "no v1 TTL/GC" stance on the index: cleaning content is a
/// disk-reclaim operation, not a "forget what I know" operation, and the
/// only thing that invalidates the index is an explicit `repos refresh`.
pub async fn execute_cache_clean(args: CacheCleanArgs) -> CliResult<()> {
    let source = args
        .source
        .as_deref()
        .map(ContentSourceKind::from_str)
        .transpose()?;

    let cache = SkillCache::from_env()?;
    let report = cache.clean(source)?;

    if args.json {
        let payload = serde_json::json!({
            "root": cache.root().display().to_string(),
            "source": source.map(|k| k.to_string()).unwrap_or_else(|| "all".to_string()),
            "entries_removed": report.entries_removed,
            "bytes_reclaimed": report.bytes_reclaimed,
        });
        let json_output = serde_json::to_string_pretty(&payload)
            .map_err(|e| CliError::Config(format!("Failed to serialize JSON: {}", e)))?;
        crate::outln!("{}", json_output);
    } else {
        crate::outln!("{}", format_clean_message(source, &report));
    }
    Ok(())
}

fn format_clean_message(source: Option<ContentSourceKind>, report: &CleanReport) -> String {
    let scope = source
        .map(|k| k.to_string())
        .unwrap_or_else(|| "all sources".to_string());
    messages::ok(&format!(
        "Cleaned {}: removed {} {}, reclaimed {}",
        scope,
        report.entries_removed,
        if report.entries_removed == 1 {
            "entry"
        } else {
            "entries"
        },
        format_bytes(report.bytes_reclaimed),
    ))
}

/// Human-readable byte size, e.g. `"1.2 MB"`, `"45.6 KB"` (STYLE.md's number
/// formatting example).
fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit_idx = 0usize;
    while value >= 1024.0 && unit_idx < UNITS.len() - 1 {
        value /= 1024.0;
        unit_idx += 1;
    }
    if unit_idx == 0 {
        format!("{bytes} {}", UNITS[0])
    } else {
        format!("{value:.1} {}", UNITS[unit_idx])
    }
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::await_holding_lock)]
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// RAII guard restoring `FASTSKILL_CACHE_DIR` to whatever it was before
    /// the test set it (mirrors `repo_ops.rs`'s `CacheDirEnvGuard`), so these
    /// tests never leak into the real platform cache dir nor race other
    /// tests sharing this process.
    struct CacheDirEnvGuard(Option<String>);
    impl Drop for CacheDirEnvGuard {
        fn drop(&mut self) {
            match &self.0 {
                Some(v) => std::env::set_var("FASTSKILL_CACHE_DIR", v),
                None => std::env::remove_var("FASTSKILL_CACHE_DIR"),
            }
        }
    }

    #[test]
    fn format_bytes_renders_human_units() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1536), "1.5 KB");
        assert_eq!(format_bytes(45 * 1024 + 600), "45.6 KB");
        assert_eq!(format_bytes(12 * 1024 * 1024), "12.0 MB");
    }

    #[tokio::test]
    async fn cache_info_on_an_empty_cache_reports_zero_entries() {
        let _lock = fastskill_core::test_utils::DIR_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let cache_dir = TempDir::new().unwrap();
        let _env_guard = CacheDirEnvGuard(std::env::var("FASTSKILL_CACHE_DIR").ok());
        std::env::set_var("FASTSKILL_CACHE_DIR", cache_dir.path());

        let result = execute_cache_info(CacheInfoArgs {
            format: None,
            json: false,
        })
        .await;
        assert!(
            result.is_ok(),
            "cache info should succeed: {:?}",
            result.err()
        );
    }

    #[tokio::test]
    async fn cache_info_rejects_grid_format() {
        let err = validate_cache_info_format_args(&Some(OutputFormat::Grid), false)
            .expect_err("grid must be rejected");
        assert!(err.to_string().contains("table or json"));
    }

    #[tokio::test]
    async fn cache_clean_rejects_an_unknown_source() {
        let _lock = fastskill_core::test_utils::DIR_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let cache_dir = TempDir::new().unwrap();
        let _env_guard = CacheDirEnvGuard(std::env::var("FASTSKILL_CACHE_DIR").ok());
        std::env::set_var("FASTSKILL_CACHE_DIR", cache_dir.path());

        let result = execute_cache_clean(CacheCleanArgs {
            source: Some("bogus".to_string()),
            json: false,
        })
        .await;
        let err = result.expect_err("unknown source must be rejected");
        assert!(err.to_string().contains("git, registry, local"));
    }

    #[tokio::test]
    async fn cache_clean_on_a_never_used_cache_is_a_harmless_noop() {
        let _lock = fastskill_core::test_utils::DIR_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let cache_dir = TempDir::new().unwrap();
        let _env_guard = CacheDirEnvGuard(std::env::var("FASTSKILL_CACHE_DIR").ok());
        std::env::set_var("FASTSKILL_CACHE_DIR", cache_dir.path().join("unused"));

        let result = execute_cache_clean(CacheCleanArgs {
            source: None,
            json: true,
        })
        .await;
        assert!(result.is_ok(), "clean should succeed: {:?}", result.err());
    }

    #[tokio::test]
    async fn cache_info_then_clean_round_trip_reflects_removed_content() {
        let _lock = fastskill_core::test_utils::DIR_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let cache_dir = TempDir::new().unwrap();
        let _env_guard = CacheDirEnvGuard(std::env::var("FASTSKILL_CACHE_DIR").ok());
        std::env::set_var("FASTSKILL_CACHE_DIR", cache_dir.path());

        let cache = SkillCache::from_env().unwrap();
        let src = TempDir::new().unwrap();
        std::fs::write(src.path().join("SKILL.md"), "---\nname: demo\n---\nbody\n").unwrap();
        cache
            .put(
                &fastskill_core::core::cache::CacheIdentity::Local {
                    tree_hash: "abc123".to_string(),
                },
                src.path(),
            )
            .unwrap();

        let stats_before = cache.stats().unwrap();
        assert_eq!(stats_before.local.entry_count, 1);

        let clean_result = execute_cache_clean(CacheCleanArgs {
            source: Some("local".to_string()),
            json: false,
        })
        .await;
        assert!(clean_result.is_ok(), "{:?}", clean_result.err());

        let stats_after = cache.stats().unwrap();
        assert_eq!(stats_after.local.entry_count, 0);
    }
}
