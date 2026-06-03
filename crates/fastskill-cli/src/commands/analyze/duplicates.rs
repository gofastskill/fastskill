//! Duplicates command — find semantically duplicate or very similar skills.

use super::helpers::{compute_suggestion, get_file_mtime, get_skill_name};
use super::AnalysisContext;
use crate::commands::common::validate_format_args;
use crate::error::{CliError, CliResult};
use cli_framework::command::{FromArgValueMap, IntoCommandSpec};
use cli_framework::spec::arg_spec::{ArgKind, ArgSpec, ArgValueType, Cardinality};
use cli_framework::spec::command_tree::CommandSpec;
use cli_framework::spec::value::ArgValue;
use fastskill_core::core::analysis::skill_similarity;
use fastskill_core::OutputFormat;
use serde::Serialize;
use std::collections::HashMap;

#[derive(Debug, Clone, Default)]
pub enum SeverityFilter {
    #[default]
    All,
    Medium,
    High,
    Critical,
}

#[derive(Debug, Clone)]
pub struct DuplicatesArgs {
    pub threshold: f32,
    pub limit: usize,
    pub severity: SeverityFilter,
    pub format: Option<OutputFormat>,
    pub json: bool,
}

fn parse_output_format(s: &str) -> Option<fastskill_core::OutputFormat> {
    match s {
        "table" => Some(fastskill_core::OutputFormat::Table),
        "json" => Some(fastskill_core::OutputFormat::Json),
        "grid" => Some(fastskill_core::OutputFormat::Grid),
        "xml" => Some(fastskill_core::OutputFormat::Xml),
        _ => None,
    }
}

fn parse_severity(s: &str) -> SeverityFilter {
    match s {
        "medium" => SeverityFilter::Medium,
        "high" => SeverityFilter::High,
        "critical" => SeverityFilter::Critical,
        _ => SeverityFilter::All,
    }
}

impl IntoCommandSpec for DuplicatesArgs {
    fn command_spec() -> CommandSpec {
        CommandSpec {
            summary: "Find semantically duplicate or very similar skills",
            syntax: Some("analyze duplicates [OPTIONS]"),
            category: Some("analysis"),
            args: vec![
                ArgSpec {
                    name: "threshold",
                    kind: ArgKind::Option,
                    long: Some("threshold"),
                    value_type: ArgValueType::Float,
                    cardinality: Cardinality::Optional,
                    default: Some(ArgValue::Float(0.88)),
                    help: "Minimum similarity to report",
                    ..Default::default()
                },
                ArgSpec {
                    name: "limit",
                    kind: ArgKind::Option,
                    long: Some("limit"),
                    value_type: ArgValueType::Int,
                    cardinality: Cardinality::Optional,
                    default: Some(ArgValue::Int(20)),
                    help: "Maximum number of pairs to show",
                    ..Default::default()
                },
                ArgSpec {
                    name: "severity",
                    kind: ArgKind::Option,
                    long: Some("severity"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: Some(ArgValue::Str("all".to_string())),
                    help: "Filter by minimum severity: critical, high, medium, all",
                    ..Default::default()
                },
                ArgSpec {
                    name: "format",
                    kind: ArgKind::Option,
                    long: Some("format"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    help: "Output format: table, json, grid, xml",
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

impl FromArgValueMap for DuplicatesArgs {
    fn from_arg_value_map(map: &HashMap<String, ArgValue>) -> Self {
        DuplicatesArgs {
            threshold: match map.get("threshold") {
                Some(ArgValue::Float(f)) => *f as f32,
                _ => 0.88,
            },
            limit: match map.get("limit") {
                Some(ArgValue::Int(n)) => *n as usize,
                _ => 20,
            },
            severity: map
                .get("severity")
                .and_then(|v| {
                    if let ArgValue::Str(s) = v {
                        Some(s.as_str())
                    } else {
                        None
                    }
                })
                .map(parse_severity)
                .unwrap_or_default(),
            format: map
                .get("format")
                .and_then(|v| {
                    if let ArgValue::Str(s) = v {
                        Some(s.as_str())
                    } else {
                        None
                    }
                })
                .and_then(parse_output_format),
            json: matches!(map.get("json"), Some(ArgValue::Bool(true))),
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub(super) enum SeverityLevel {
    Critical,
    High,
    Medium,
}

impl SeverityLevel {
    pub(super) fn label(&self) -> &'static str {
        match self {
            SeverityLevel::Critical => "[CRITICAL]",
            SeverityLevel::High => "[HIGH]    ",
            SeverityLevel::Medium => "[MEDIUM]  ",
        }
    }
}

#[derive(Debug, Serialize)]
pub(super) struct DuplicateSkillInfo {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified_at: Option<String>,
}

#[derive(Debug, Serialize)]
pub(super) struct DuplicatePair {
    pub severity: SeverityLevel,
    pub similarity: f32,
    pub skill_a: DuplicateSkillInfo,
    pub skill_b: DuplicateSkillInfo,
    pub suggestion: String,
}

#[derive(Debug, Serialize)]
pub(super) struct DuplicatesJsonOutput {
    pub threshold: f32,
    pub effective_floor: f32,
    pub total_pairs: usize,
    pub pairs: Vec<DuplicatePair>,
}

/// Execute the duplicates command
pub async fn execute_duplicates(ctx: AnalysisContext, args: DuplicatesArgs) -> CliResult<()> {
    let format = validate_format_args(&args.format, args.json)?;
    let use_json = format == OutputFormat::Json;
    let all_skills = ctx.skills;

    let n = all_skills.len();
    let s_floor = severity_floor_value(&args.severity, args.threshold);
    let effective_floor = args.threshold.max(s_floor);

    if !use_json {
        println!(
            "Scanning {} skills for duplicates (threshold: {:.2})...",
            n, args.threshold
        );
    }

    let mut pairs: Vec<DuplicatePair> = Vec::new();
    for i in 0..n {
        for j in (i + 1)..n {
            let sim = skill_similarity(&all_skills[i].embedding, &all_skills[j].embedding);
            if sim < effective_floor {
                continue;
            }

            let (skill_a, skill_b) = if all_skills[i].id <= all_skills[j].id {
                (&all_skills[i], &all_skills[j])
            } else {
                (&all_skills[j], &all_skills[i])
            };

            let Some(severity) = classify_severity(sim) else {
                continue;
            };
            let suggestion = compute_suggestion(skill_a, skill_b);
            let modified_at_a = get_file_mtime(&skill_a.skill_path);
            let modified_at_b = get_file_mtime(&skill_b.skill_path);

            pairs.push(DuplicatePair {
                severity,
                similarity: sim,
                skill_a: DuplicateSkillInfo {
                    id: skill_a.id.clone(),
                    name: get_skill_name(&skill_a.frontmatter_json),
                    modified_at: modified_at_a,
                },
                skill_b: DuplicateSkillInfo {
                    id: skill_b.id.clone(),
                    name: get_skill_name(&skill_b.frontmatter_json),
                    modified_at: modified_at_b,
                },
                suggestion,
            });
        }
    }

    pairs.sort_by(|a, b| {
        b.similarity
            .partial_cmp(&a.similarity)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.skill_a.id.cmp(&b.skill_a.id))
    });

    pairs.truncate(args.limit);

    if use_json {
        let output = DuplicatesJsonOutput {
            threshold: args.threshold,
            effective_floor,
            total_pairs: pairs.len(),
            pairs,
        };
        let json_output = serde_json::to_string_pretty(&output)
            .map_err(|e| CliError::Validation(format!("Failed to serialize JSON: {}", e)))?;
        println!("{}", json_output);
    } else if pairs.is_empty() {
        println!("No duplicate pairs found above threshold.");
    } else {
        println!("Found {} potential duplicate pairs\n", pairs.len());
        for pair in &pairs {
            println!(
                "{}  {:.3}  {}  ↔  {}",
                pair.severity.label(),
                pair.similarity,
                pair.skill_a.id,
                pair.skill_b.id
            );
            println!("  Suggestion: {}", pair.suggestion);
            println!();
        }

        struct SeverityCounts {
            critical: usize,
            high: usize,
            medium: usize,
        }
        let counts = pairs.iter().fold(
            SeverityCounts {
                critical: 0,
                high: 0,
                medium: 0,
            },
            |mut acc, p| {
                match p.severity {
                    SeverityLevel::Critical => acc.critical += 1,
                    SeverityLevel::High => acc.high += 1,
                    SeverityLevel::Medium => acc.medium += 1,
                }
                acc
            },
        );
        let summary = [
            (counts.critical, "critical"),
            (counts.high, "high"),
            (counts.medium, "medium"),
        ]
        .into_iter()
        .filter(|(n, _)| *n > 0)
        .map(|(n, label)| format!("{} {}", n, label))
        .collect::<Vec<_>>()
        .join(", ");
        println!("Summary: {}", summary);
        println!("Run 'fastskill remove <skill-id>' to remove a skill after review.");
    }

    Ok(())
}

/// Returns the severity floor value for a given filter
pub(super) fn severity_floor_value(filter: &SeverityFilter, threshold: f32) -> f32 {
    match filter {
        SeverityFilter::All => 0.0,
        SeverityFilter::Medium => threshold,
        SeverityFilter::High => 0.93,
        SeverityFilter::Critical => 0.98,
    }
}

/// Classify a similarity score into a severity level
pub(super) fn classify_severity(similarity: f32) -> Option<SeverityLevel> {
    if similarity >= 0.98 {
        Some(SeverityLevel::Critical)
    } else if similarity >= 0.93 {
        Some(SeverityLevel::High)
    } else if similarity >= 0.88 {
        Some(SeverityLevel::Medium)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_severity_critical() {
        assert_eq!(classify_severity(0.99), Some(SeverityLevel::Critical));
        assert_eq!(classify_severity(0.98), Some(SeverityLevel::Critical));
        assert_eq!(classify_severity(1.0), Some(SeverityLevel::Critical));
    }

    #[test]
    fn test_classify_severity_high() {
        assert_eq!(classify_severity(0.94), Some(SeverityLevel::High));
        assert_eq!(classify_severity(0.93), Some(SeverityLevel::High));
        let just_below_critical = 0.97999_f32;
        assert_eq!(
            classify_severity(just_below_critical),
            Some(SeverityLevel::High)
        );
    }

    #[test]
    fn test_classify_severity_medium() {
        assert_eq!(classify_severity(0.90), Some(SeverityLevel::Medium));
        assert_eq!(classify_severity(0.88), Some(SeverityLevel::Medium));
        let just_below_high = 0.929_f32;
        assert_eq!(
            classify_severity(just_below_high),
            Some(SeverityLevel::Medium)
        );
    }

    #[test]
    fn test_classify_severity_below_medium_is_none() {
        assert_eq!(classify_severity(0.879), None);
        assert_eq!(classify_severity(0.5), None);
    }

    #[test]
    fn test_severity_floor_all() {
        assert_eq!(severity_floor_value(&SeverityFilter::All, 0.88), 0.0);
    }

    #[test]
    fn test_severity_floor_medium() {
        assert_eq!(severity_floor_value(&SeverityFilter::Medium, 0.88), 0.88);
        assert_eq!(severity_floor_value(&SeverityFilter::Medium, 0.80), 0.80);
    }

    #[test]
    fn test_severity_floor_high() {
        assert_eq!(severity_floor_value(&SeverityFilter::High, 0.88), 0.93);
        assert_eq!(severity_floor_value(&SeverityFilter::High, 0.80), 0.93);
    }

    #[test]
    fn test_severity_floor_critical() {
        assert_eq!(severity_floor_value(&SeverityFilter::Critical, 0.88), 0.98);
    }

    #[test]
    fn test_effective_floor_threshold_dominates() {
        let floor = severity_floor_value(&SeverityFilter::High, 0.99);
        let effective = 0.99_f32.max(floor);
        assert!((effective - 0.99).abs() < 1e-6);
    }

    #[test]
    fn test_effective_floor_severity_dominates() {
        let floor = severity_floor_value(&SeverityFilter::High, 0.80);
        let effective = 0.80_f32.max(floor);
        assert!((effective - 0.93).abs() < 1e-6);
    }

    #[test]
    fn test_duplicates_json_output_serialization() {
        let output = DuplicatesJsonOutput {
            threshold: 0.88,
            effective_floor: 0.93,
            total_pairs: 1,
            pairs: vec![DuplicatePair {
                severity: SeverityLevel::High,
                similarity: 0.95,
                skill_a: DuplicateSkillInfo {
                    id: "skill-a".to_string(),
                    name: "Skill A".to_string(),
                    modified_at: Some("2026-01-01T00:00:00Z".to_string()),
                },
                skill_b: DuplicateSkillInfo {
                    id: "skill-b".to_string(),
                    name: "Skill B".to_string(),
                    modified_at: None,
                },
                suggestion: "Review both — similar modification time".to_string(),
            }],
        };
        #[allow(clippy::expect_used)]
        let json =
            serde_json::to_string_pretty(&output).expect("Output serialization should not fail");
        assert!(json.contains("\"effective_floor\""));
        assert!(json.contains("0.93"));
        assert!(json.contains("\"high\""));
        assert!(json.contains("skill-a"));
        assert!(!json.contains("\"modified_at\": null"));
    }
}
