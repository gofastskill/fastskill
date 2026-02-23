//! Diagnostic and analysis commands for skill relationships and quality

use crate::cli::error::{CliError, CliResult};
use clap::{Args, Subcommand};
use fastskill::core::analysis::SimilarityPair;
use fastskill::core::vector_index::IndexedSkill;
use fastskill::FastSkillService;
use std::collections::HashMap;

/// Analyze command arguments
#[derive(Debug, Args)]
#[command(
    after_help = "Examples:\n  fastskill analyze matrix\n  fastskill analyze matrix --threshold 0.8"
)]
pub struct AnalyzeCommand {
    #[command(subcommand)]
    pub command: AnalyzeSubcommand,
}

/// Analyze subcommands
#[derive(Debug, Subcommand)]
pub enum AnalyzeSubcommand {
    /// Show similarity matrix between all skills
    #[command(
        about = "Show pairwise similarity matrix for all indexed skills",
        after_help = "Shows top-N most similar skills for each skill.\n\n\
            Use --threshold to filter low-similarity pairs.\n\
            Use --limit to control how many similar skills to show per skill.\n\
            Use --full to show all pairs instead of a condensed view."
    )]
    Matrix(MatrixArgs),

    /// Group skills by semantic similarity (FUTURE)
    #[command(about = "Group skills by semantic similarity (not yet implemented)")]
    Cluster(ClusterArgs),

    /// Find potential duplicate skills (FUTURE)
    #[command(about = "Find semantically duplicate or very similar skills (not yet implemented)")]
    Duplicates(DuplicatesArgs),
}

/// Matrix command arguments
#[derive(Debug, Args, Clone)]
pub struct MatrixArgs {
    /// Output in JSON format
    #[arg(long, help = "Output in JSON format")]
    pub json: bool,

    /// Similarity threshold (0.0 to 1.0) - only show pairs above this threshold
    #[arg(
        long,
        default_value = "0.0",
        value_parser = validate_threshold,
        help = "Minimum similarity threshold to display (0.0 to 1.0)"
    )]
    pub threshold: f32,

    /// Limit number of similar skills to show per skill
    #[arg(
        short,
        long,
        default_value = "10",
        help = "Number of similar skills to show per skill"
    )]
    pub limit: usize,

    /// Show full matrix (all pairs) instead of top-N per skill
    #[arg(long, help = "Show all similar skills instead of top-N summary")]
    pub full: bool,
}

/// Cluster command arguments (placeholder for future implementation)
#[derive(Debug, Args, Clone)]
pub struct ClusterArgs {
    /// Number of clusters to create
    #[arg(short = 'k', long, default_value = "5")]
    pub num_clusters: usize,
}

/// Duplicates command arguments (placeholder for future implementation)
#[derive(Debug, Args, Clone)]
pub struct DuplicatesArgs {
    /// Similarity threshold for considering skills as duplicates
    #[arg(long, default_value = "0.95")]
    pub threshold: f32,
}

/// Validates that threshold is between 0.0 and 1.0
fn validate_threshold(s: &str) -> Result<f32, String> {
    let value: f32 = s
        .parse()
        .map_err(|_| format!("'{}' is not a valid number", s))?;

    if !(0.0..=1.0).contains(&value) {
        return Err(format!(
            "threshold must be between 0.0 and 1.0, got {}",
            value
        ));
    }

    Ok(value)
}

/// Main dispatcher for analyze subcommands
pub async fn execute_analyze(service: &FastSkillService, command: AnalyzeCommand) -> CliResult<()> {
    match command.command {
        AnalyzeSubcommand::Matrix(args) => execute_matrix(service, args).await,
        AnalyzeSubcommand::Cluster(_) => Err(CliError::Config(
            "Cluster command not yet implemented. Stay tuned for future releases!".to_string(),
        )),
        AnalyzeSubcommand::Duplicates(_) => Err(CliError::Config(
            "Duplicates command not yet implemented. Stay tuned for future releases!".to_string(),
        )),
    }
}

/// Execute the matrix command
pub async fn execute_matrix(service: &FastSkillService, args: MatrixArgs) -> CliResult<()> {
    // Get vector index service
    let vector_index_service = service.vector_index_service().ok_or_else(|| {
        CliError::Config("Vector index not available. Run 'fastskill reindex' first.".to_string())
    })?;

    // Get all indexed skills
    let all_skills: Vec<IndexedSkill> = vector_index_service
        .get_all_skills()
        .await
        .map_err(|e| CliError::Validation(format!("Failed to get indexed skills: {}", e)))?;

    if all_skills.is_empty() {
        println!("No skills indexed. Run 'fastskill reindex' first.");
        return Ok(());
    }

    // Show progress for large collections
    if !args.json && all_skills.len() > 50 {
        let total_comparisons = all_skills.len() * (all_skills.len() - 1) / 2;
        println!(
            "Calculating {} pairwise comparisons for {} skills...",
            total_comparisons,
            all_skills.len()
        );
        println!("This may take a moment for large skill collections.");
    } else if !args.json {
        println!(
            "Calculating pairwise similarities for {} skills...",
            all_skills.len()
        );
    }

    // Build similarity matrix
    let similarity_matrix = build_similarity_matrix(&all_skills, &args);

    // Output results
    if args.json {
        let filtered: Vec<&SimilarityPair> = if args.threshold > 0.0 {
            similarity_matrix
                .iter()
                .filter(|p| !p.similar_skills.is_empty())
                .collect()
        } else {
            similarity_matrix.iter().collect()
        };
        let json_output = serde_json::to_string_pretty(&filtered)
            .map_err(|e| CliError::Validation(format!("Failed to serialize JSON: {}", e)))?;
        println!("{}", json_output);
    } else {
        print_similarity_table(&similarity_matrix, args.threshold);
    }

    Ok(())
}

/// Builds the similarity matrix by comparing all skill pairs
fn build_similarity_matrix(all_skills: &[IndexedSkill], args: &MatrixArgs) -> Vec<SimilarityPair> {
    all_skills
        .iter()
        .map(|skill_a| {
            let mut similarities: Vec<(String, f32)> = all_skills
                .iter()
                .enumerate()
                .filter(|(j, _)| skill_a.id != all_skills[*j].id)
                .map(|(_, skill_b)| {
                    let similarity = cosine_similarity(&skill_a.embedding, &skill_b.embedding);
                    (skill_b.id.clone(), similarity)
                })
                .filter(|(_, similarity)| *similarity >= args.threshold)
                .collect();

            // Sort by similarity descending (highest first)
            similarities.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

            // Only truncate if not showing full matrix
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

/// Extracts skill name from metadata JSON
fn get_skill_name(metadata: &serde_json::Value) -> String {
    metadata
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("Unknown")
        .to_string()
}

/// Calculates cosine similarity between two embedding vectors
///
/// Formula: cos(θ) = (A · B) / (||A|| × ||B||)
///
/// Returns a value between -1.0 (opposite) and 1.0 (identical).
/// For normalized embeddings (like OpenAI's), values are typically between 0.0 and 1.0.
fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() {
        return 0.0;
    }

    if a.is_empty() {
        return 0.0;
    }

    let mut dot_product = 0.0_f32;
    let mut norm_a = 0.0_f32;
    let mut norm_b = 0.0_f32;

    for i in 0..a.len() {
        dot_product += a[i] * b[i];
        norm_a += a[i] * a[i];
        norm_b += b[i] * b[i];
    }

    let norm_a = norm_a.sqrt();
    let norm_b = norm_b.sqrt();

    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }

    dot_product / (norm_a * norm_b)
}

/// Prints the similarity grid to stdout.
///
/// This is a thin wrapper around `build_grid_string` that prints the result.
fn print_similarity_table(matrix: &[SimilarityPair], threshold: f32) {
    print!("{}", build_grid_string(matrix, threshold));
}

/// Truncates a column header ID to at most 17 characters.
///
/// If the ID exceeds 18 characters, it is shortened to 15 chars with a ".." suffix.
fn truncate_header(s: &str) -> String {
    if s.len() > 18 {
        format!("{}..", &s[..15])
    } else {
        s.to_string()
    }
}

/// Builds the ASCII grid string for the similarity matrix.
///
/// Pure function — does not print anything; enables unit testing without capturing stdout.
///
/// When `threshold > 0.0`, skills with no entries in `similar_skills` are omitted from
/// both rows and columns. When `threshold == 0.0` (the default), all skills are shown.
fn build_grid_string(matrix: &[SimilarityPair], threshold: f32) -> String {
    let sep = "=".repeat(80);
    let header_line = if threshold > 0.0 {
        format!("Similarity Matrix (threshold: {:.2})", threshold)
    } else {
        "Similarity Matrix".to_string()
    };

    // Filter to skills that have at least one similar-skill entry when threshold is active
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

    // IDs of visible skills — these become both rows and columns
    let ids: Vec<&str> = visible.iter().map(|p| p.skill_id.as_str()).collect();

    // Build a score lookup from the full matrix data (both directions, since cosine is symmetric)
    let mut scores: HashMap<String, f32> = HashMap::new();
    for pair in matrix {
        for (other_id, score) in &pair.similar_skills {
            let key_fwd = format!("{}|{}", pair.skill_id, other_id);
            scores.insert(key_fwd, *score);
            // Insert reverse only if not already present (avoid overwriting a direct entry)
            let key_rev = format!("{}|{}", other_id, pair.skill_id);
            scores.entry(key_rev).or_insert(*score);
        }
    }

    // Compute column headers (possibly truncated)
    let col_headers: Vec<String> = ids.iter().map(|id| truncate_header(id)).collect();

    // Column width = max(header display length, 5)
    let col_widths: Vec<usize> = col_headers.iter().map(|h| h.len().max(5)).collect();

    // Row label area width = length of longest skill ID
    let row_label_width = ids.iter().map(|id| id.len()).max().unwrap_or(0);

    // --- Column header row ---
    out.push('\n');
    out.push_str(&" ".repeat(row_label_width));
    for (i, header) in col_headers.iter().enumerate() {
        let w = col_widths[i];
        out.push_str(&format!("  {:>width$}", header, width = w));
    }
    out.push('\n');

    // --- Data rows ---
    for row_pair in &visible {
        let row_id = row_pair.skill_id.as_str();
        out.push_str(&format!("{:<width$}", row_id, width = row_label_width));

        for (col_idx, col_id) in ids.iter().enumerate() {
            let w = col_widths[col_idx];
            out.push_str("  ");

            if row_id == *col_id {
                // Diagonal cell: em-dash centred in column width.
                // "—" is 3 UTF-8 bytes but 1 display character; compute padding manually.
                let pad_total = w.saturating_sub(1);
                let left = pad_total / 2;
                let right = pad_total - left;
                out.push_str(&format!("{}{}{}", " ".repeat(left), "—", " ".repeat(right)));
            } else {
                let key = format!("{}|{}", row_id, col_id);
                if let Some(&score) = scores.get(&key) {
                    out.push_str(&format!("{:>width$.3}", score, width = w));
                } else {
                    // Score not available (below threshold or truncated by limit)
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

    #[test]
    fn test_cosine_similarity_identical_vectors() {
        let a = vec![1.0, 2.0, 3.0];
        let b = vec![1.0, 2.0, 3.0];
        let similarity = cosine_similarity(&a, &b);
        assert!(
            (similarity - 1.0).abs() < 0.001,
            "Identical vectors should have similarity ~1.0"
        );
    }

    #[test]
    fn test_cosine_similarity_orthogonal_vectors() {
        let a = vec![1.0, 0.0];
        let b = vec![0.0, 1.0];
        let similarity = cosine_similarity(&a, &b);
        assert!(
            similarity.abs() < 0.001,
            "Orthogonal vectors should have similarity ~0.0"
        );
    }

    #[test]
    fn test_cosine_similarity_opposite_vectors() {
        let a = vec![1.0, 2.0, 3.0];
        let b = vec![-1.0, -2.0, -3.0];
        let similarity = cosine_similarity(&a, &b);
        assert!(
            (similarity + 1.0).abs() < 0.001,
            "Opposite vectors should have similarity ~-1.0"
        );
    }

    #[test]
    fn test_cosine_similarity_different_lengths() {
        let a = vec![1.0, 2.0];
        let b = vec![1.0, 2.0, 3.0];
        let similarity = cosine_similarity(&a, &b);
        assert_eq!(
            similarity, 0.0,
            "Different length vectors should return 0.0"
        );
    }

    #[test]
    fn test_cosine_similarity_empty_vectors() {
        let a: Vec<f32> = vec![];
        let b: Vec<f32> = vec![];
        let similarity = cosine_similarity(&a, &b);
        assert_eq!(similarity, 0.0, "Empty vectors should return 0.0");
    }

    #[test]
    fn test_validate_threshold_valid() {
        assert!(validate_threshold("0.0").is_ok());
        assert!(validate_threshold("0.5").is_ok());
        assert!(validate_threshold("1.0").is_ok());
    }

    #[test]
    fn test_validate_threshold_invalid() {
        assert!(validate_threshold("-0.1").is_err());
        assert!(validate_threshold("1.1").is_err());
        assert!(validate_threshold("abc").is_err());
    }

    #[test]
    fn test_get_skill_name_with_name() {
        let metadata = serde_json::json!({
            "name": "Test Skill",
            "version": "1.0.0"
        });
        assert_eq!(get_skill_name(&metadata), "Test Skill");
    }

    #[test]
    fn test_get_skill_name_without_name() {
        let metadata = serde_json::json!({
            "version": "1.0.0"
        });
        assert_eq!(get_skill_name(&metadata), "Unknown");
    }

    // --- Grid unit tests ---

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

        // 3 header labels and 3 data rows expected
        let data_rows: Vec<&str> = grid
            .lines()
            .filter(|l| l.starts_with("alpha") || l.starts_with("beta") || l.starts_with("gamma"))
            .collect();
        assert_eq!(data_rows.len(), 3, "Should have 3 data rows for 3 skills");

        // The header row starts with spaces (row-label area) and contains all column IDs
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

        // Every data row should contain the em-dash for the diagonal
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
        // skill-a has a match above threshold; skill-b does not
        let matrix = vec![
            make_pair("skill-a", vec![("skill-b", 0.9)]),
            make_pair("skill-b", vec![]),
        ];

        let grid = build_grid_string(&matrix, 0.8);

        // skill-b should not appear as a row or column label
        assert!(
            grid.contains("skill-a"),
            "skill-a should appear in the grid"
        );
        // skill-b may appear in the body as a score lookup, but not as a standalone row
        let lines: Vec<&str> = grid.lines().collect();
        let row_lines: Vec<&&str> = lines.iter().filter(|l| l.starts_with("skill-b")).collect();
        assert!(
            row_lines.is_empty(),
            "skill-b should not be a row label when it has no matches above threshold"
        );
    }

    #[test]
    fn test_build_grid_default_shows_all_skills() {
        // Both skills have empty similar_skills; with threshold=0.0 both should appear
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
}
