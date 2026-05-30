//! Matrix command — pairwise similarity grid between all indexed skills.

use super::helpers::get_skill_name;
use super::AnalysisContext;
use crate::commands::common::validate_format_args;
use crate::error::CliResult;
use clap::Args;
use fastskill_core::core::analysis::{skill_similarity, SimilarityPair};
use fastskill_core::OutputFormat;
use std::collections::HashMap;

#[derive(Debug, Args, Clone)]
pub struct MatrixArgs {
    #[arg(long, value_enum, help = "Output format: table, json, grid, xml")]
    pub format: Option<OutputFormat>,
    #[arg(long, help = "Shorthand for --format json")]
    pub json: bool,
    #[arg(long, default_value = "0.0", value_parser = super::validate_threshold,
          help = "Minimum similarity threshold to display (0.0 to 1.0)")]
    pub threshold: f32,
    #[arg(
        short,
        long,
        default_value = "10",
        help = "Number of similar skills to show per skill"
    )]
    pub limit: usize,
    #[arg(long, help = "Show all similar skills instead of top-N summary")]
    pub full: bool,
}

/// Execute the matrix command
pub async fn execute_matrix(ctx: AnalysisContext, args: MatrixArgs) -> CliResult<()> {
    let format = validate_format_args(&args.format, args.json)?;
    let use_json = format == OutputFormat::Json;
    let all_skills = ctx.skills;

    if !use_json && all_skills.len() > 50 {
        let total_comparisons = all_skills.len() * (all_skills.len() - 1) / 2;
        println!(
            "Calculating {} pairwise comparisons for {} skills...",
            total_comparisons,
            all_skills.len()
        );
        println!("This may take a moment for large skill collections.");
    } else if !use_json {
        println!(
            "Calculating pairwise similarities for {} skills...",
            all_skills.len()
        );
    }

    let similarity_matrix = build_similarity_matrix(&all_skills, &args);

    if use_json {
        let filtered: Vec<&SimilarityPair> = if args.threshold > 0.0 {
            similarity_matrix
                .iter()
                .filter(|p| !p.similar_skills.is_empty())
                .collect()
        } else {
            similarity_matrix.iter().collect()
        };
        let json_output = serde_json::to_string_pretty(&filtered).map_err(|e| {
            crate::error::CliError::Validation(format!("Failed to serialize JSON: {}", e))
        })?;
        println!("{}", json_output);
    } else {
        print!("{}", build_grid_string(&similarity_matrix, args.threshold));
    }

    Ok(())
}

/// Builds the similarity matrix by comparing all skill pairs
fn build_similarity_matrix(
    all_skills: &[fastskill_core::core::vector_index::IndexedSkill],
    args: &MatrixArgs,
) -> Vec<SimilarityPair> {
    all_skills
        .iter()
        .map(|skill_a| {
            let mut similarities: Vec<(String, f32)> = all_skills
                .iter()
                .enumerate()
                .filter(|(j, _)| skill_a.id != all_skills[*j].id)
                .map(|(_, skill_b)| {
                    let similarity = skill_similarity(&skill_a.embedding, &skill_b.embedding);
                    (skill_b.id.clone(), similarity)
                })
                .filter(|(_, similarity)| *similarity >= args.threshold)
                .collect();

            similarities.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

            if !args.full {
                similarities.truncate(args.limit);
            }

            SimilarityPair {
                skill_id: skill_a.id.clone(),
                name: get_skill_name(&skill_a.frontmatter_json),
                similar_skills: similarities,
            }
        })
        .collect()
}

/// Truncates a column header ID to at most 15 Unicode characters.
fn truncate_header(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() > 15 {
        let truncated: String = chars.into_iter().take(15).collect();
        format!("{}..", truncated)
    } else {
        s.to_string()
    }
}

/// Builds the ASCII grid string for the similarity matrix.
///
/// Pure function — does not print anything; enables unit testing without capturing stdout.
pub fn build_grid_string(matrix: &[SimilarityPair], threshold: f32) -> String {
    let sep = "=".repeat(80);
    let header_line = if threshold > 0.0 {
        format!("Similarity Matrix (threshold: {:.2})", threshold)
    } else {
        "Similarity Matrix".to_string()
    };

    let visible: Vec<&SimilarityPair> = if threshold > 0.0 {
        matrix
            .iter()
            .filter(|p| !p.similar_skills.is_empty())
            .collect()
    } else {
        matrix.iter().collect()
    };

    let mut out = String::new();
    out.push_str(&format!("\n{}\n{}\n{}\n", sep, header_line, sep));

    if visible.is_empty() {
        out.push_str(&format!(
            "\n  No skills with similarity >= {:.2} found.\n\n{}\n",
            threshold, sep
        ));
        return out;
    }

    let ids: Vec<&str> = visible.iter().map(|p| p.skill_id.as_str()).collect();

    let mut scores: HashMap<String, f32> = HashMap::new();
    for pair in matrix {
        for (other_id, score) in &pair.similar_skills {
            scores
                .entry(format!("{}|{}", pair.skill_id, other_id))
                .or_insert(*score);
            scores
                .entry(format!("{}|{}", other_id, pair.skill_id))
                .or_insert(*score);
        }
    }

    let col_headers: Vec<String> = ids.iter().map(|id| truncate_header(id)).collect();
    let col_widths: Vec<usize> = col_headers.iter().map(|h| h.len().max(5)).collect();
    let row_label_width = ids.iter().map(|id| id.len()).max().unwrap_or(0);

    out.push('\n');
    out.push_str(&" ".repeat(row_label_width));
    for (i, header) in col_headers.iter().enumerate() {
        out.push_str(&format!("  {:>width$}", header, width = col_widths[i]));
    }
    out.push('\n');

    for row_pair in &visible {
        let row_id = row_pair.skill_id.as_str();
        out.push_str(&format!("{:<width$}", row_id, width = row_label_width));

        for (col_idx, col_id) in ids.iter().enumerate() {
            let w = col_widths[col_idx];
            out.push_str("  ");
            if row_id == *col_id {
                let pad_total = w.saturating_sub(1);
                let left = pad_total / 2;
                let right = pad_total - left;
                out.push_str(&format!("{}{}{}", " ".repeat(left), "—", " ".repeat(right)));
            } else {
                let key = format!("{}|{}", row_id, col_id);
                if let Some(&score) = scores.get(&key) {
                    out.push_str(&format!("{:>width$.3}", score, width = w));
                } else {
                    out.push_str(&" ".repeat(w));
                }
            }
        }
        out.push('\n');
    }

    out.push_str(&format!("\n{}\n", sep));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_pair(id: &str, similar: Vec<(&str, f32)>) -> SimilarityPair {
        SimilarityPair {
            skill_id: id.to_string(),
            name: id.to_string(),
            similar_skills: similar
                .into_iter()
                .map(|(s, f)| (s.to_string(), f))
                .collect(),
        }
    }

    #[test]
    fn test_build_grid_correct_dimensions() {
        let matrix = vec![
            make_pair("alpha", vec![("beta", 0.8), ("gamma", 0.7)]),
            make_pair("beta", vec![("alpha", 0.8), ("gamma", 0.6)]),
            make_pair("gamma", vec![("alpha", 0.7), ("beta", 0.6)]),
        ];
        let grid = build_grid_string(&matrix, 0.0);
        let data_rows: Vec<&str> = grid
            .lines()
            .filter(|l| l.starts_with("alpha") || l.starts_with("beta") || l.starts_with("gamma"))
            .collect();
        assert_eq!(data_rows.len(), 3, "Should have 3 data rows for 3 skills");
        let header_present = grid.lines().any(|l| {
            l.contains("alpha")
                && l.contains("beta")
                && l.contains("gamma")
                && l.trim_start().starts_with("alpha")
        });
        assert!(
            header_present,
            "Header row with all column labels should be present"
        );
    }

    #[test]
    fn test_build_grid_diagonal_is_dash() {
        let matrix = vec![
            make_pair("alpha", vec![("beta", 0.8)]),
            make_pair("beta", vec![("alpha", 0.8)]),
        ];
        let grid = build_grid_string(&matrix, 0.0);
        let data_rows: Vec<&str> = grid
            .lines()
            .filter(|l| l.starts_with("alpha") || l.starts_with("beta"))
            .collect();
        assert_eq!(data_rows.len(), 2);
        for row in data_rows {
            assert!(
                row.contains('—'),
                "Each data row should have a diagonal '—': {}",
                row
            );
        }
    }

    #[test]
    fn test_build_grid_threshold_hides_empty_skills() {
        let matrix = vec![
            make_pair("skill-a", vec![("skill-b", 0.9)]),
            make_pair("skill-b", vec![]),
        ];
        let grid = build_grid_string(&matrix, 0.8);
        assert!(
            grid.contains("skill-a"),
            "skill-a should appear in the grid"
        );
        let lines: Vec<&str> = grid.lines().collect();
        let row_lines: Vec<&&str> = lines.iter().filter(|l| l.starts_with("skill-b")).collect();
        assert!(
            row_lines.is_empty(),
            "skill-b should not be a row label when it has no matches above threshold"
        );
    }

    #[test]
    fn test_build_grid_default_shows_all_skills() {
        let matrix = vec![make_pair("skill-a", vec![]), make_pair("skill-b", vec![])];
        let grid = build_grid_string(&matrix, 0.0);
        assert!(
            grid.contains("skill-a"),
            "skill-a should appear with threshold=0.0"
        );
        assert!(
            grid.contains("skill-b"),
            "skill-b should appear with threshold=0.0"
        );
    }

    #[test]
    fn test_truncate_header_unicode_no_panic() {
        let skill_id = "技能_abc_123456789";
        let result = truncate_header(skill_id);
        let char_count = result.chars().count();
        assert!(
            char_count <= 17,
            "result must be at most 15 chars + 2 for '..' = 17, got {}",
            char_count
        );
    }

    #[test]
    fn test_truncate_header_ascii_short() {
        assert_eq!(truncate_header("short-id"), "short-id");
    }

    #[test]
    fn test_truncate_header_ascii_exactly_15() {
        let s = "a".repeat(15);
        assert_eq!(truncate_header(&s), s);
    }

    #[test]
    fn test_truncate_header_ascii_long() {
        let s = "a".repeat(20);
        let result = truncate_header(&s);
        assert!(result.ends_with(".."));
        assert_eq!(result.len(), 17);
    }
}
