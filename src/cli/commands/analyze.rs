//! Diagnostic and analysis commands for skill relationships and quality

use crate::cli::error::{CliError, CliResult};
use clap::{Args, Subcommand};
use fastskill::core::vector_index::IndexedSkill;
use fastskill::FastSkillService;
use serde::Serialize;
use std::collections::HashSet;

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

    /// Similarity threshold for duplicate detection
    #[arg(
        long,
        default_value = "0.95",
        value_parser = validate_threshold,
        help = "Threshold for flagging potential duplicates (0.0 to 1.0)"
    )]
    pub duplicate_threshold: f32,
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
        let json_output = serde_json::to_string_pretty(&similarity_matrix)
            .map_err(|e| CliError::Validation(format!("Failed to serialize JSON: {}", e)))?;
        println!("{}", json_output);
    } else {
        print_similarity_table(&similarity_matrix, args.full);
    }

    // Identify and display potential duplicates
    if !args.json {
        let duplicates = find_potential_duplicates(&similarity_matrix, args.duplicate_threshold);
        if !duplicates.is_empty() {
            println!("\n{}", "=".repeat(80));
            println!(
                "Potential duplicates (similarity >= {:.2}):",
                args.duplicate_threshold
            );
            println!("{}", "=".repeat(80));
            for (skill_a, skill_b, sim) in duplicates {
                println!("  {} <-> {} (similarity: {:.3})", skill_a, skill_b, sim);
            }
            println!("\nConsider reviewing these pairs for consolidation.");
        }
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

/// A skill and its most similar skills
#[derive(Debug, Serialize)]
struct SimilarityPair {
    /// Skill identifier
    skill_id: String,
    /// Human-readable skill name
    name: String,
    /// List of (skill_id, similarity_score) tuples, sorted by similarity descending
    similar_skills: Vec<(String, f32)>,
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

/// Finds potential duplicate skills by reusing the similarity matrix data
///
/// Returns pairs with similarity >= duplicate_threshold, sorted by similarity descending.
/// Uses a HashSet to avoid reporting the same pair twice (A-B and B-A).
fn find_potential_duplicates(
    similarity_matrix: &[SimilarityPair],
    duplicate_threshold: f32,
) -> Vec<(String, String, f32)> {
    let mut duplicates = Vec::new();
    let mut seen = HashSet::new();

    for pair in similarity_matrix {
        for (similar_id, similarity) in &pair.similar_skills {
            if *similarity >= duplicate_threshold {
                // Create canonical ordering (alphabetically first ID, then second)
                let (id_a, id_b) = if pair.skill_id < *similar_id {
                    (pair.skill_id.as_str(), similar_id.as_str())
                } else {
                    (similar_id.as_str(), pair.skill_id.as_str())
                };

                // Use a unique key to track seen pairs
                let key = format!("{}|{}", id_a, id_b);
                if seen.insert(key) {
                    duplicates.push((id_a.to_string(), id_b.to_string(), *similarity));
                }
            }
        }
    }

    // Sort by similarity descending (highest first)
    duplicates.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));

    // Limit to top 20 potential duplicates
    duplicates.truncate(20);

    duplicates
}

/// Prints similarity matrix in human-readable table format
fn print_similarity_table(matrix: &[SimilarityPair], full: bool) {
    if matrix.is_empty() {
        println!("\nNo similarities found matching the criteria.");
        return;
    }

    println!("\n{}", "=".repeat(80));
    println!("Similarity Matrix");
    println!("{}", "=".repeat(80));

    for pair in matrix {
        println!("\n{} ({})", pair.name, pair.skill_id);

        if pair.similar_skills.is_empty() {
            println!("  No similar skills found above threshold");
            continue;
        }

        if full {
            println!("  Similar skills:");
            for (similar_id, similarity) in &pair.similar_skills {
                println!("    {:60} {:.3}", similar_id, similarity);
            }
        } else {
            // Show condensed view (already limited by args.limit during matrix build)
            let skills_str: Vec<String> = pair
                .similar_skills
                .iter()
                .map(|(id, sim)| format!("{} ({:.3})", id, sim))
                .collect();

            if skills_str.is_empty() {
                println!("  No similar skills above threshold");
            } else {
                println!("  Top similar: {}", skills_str.join(", "));
            }
        }
    }

    println!("\n{}", "=".repeat(80));
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

    #[test]
    fn test_find_potential_duplicates_no_duplicates() {
        let matrix = vec![SimilarityPair {
            skill_id: "skill-a".to_string(),
            name: "Skill A".to_string(),
            similar_skills: vec![("skill-b".to_string(), 0.5)],
        }];

        let duplicates = find_potential_duplicates(&matrix, 0.95);
        assert!(
            duplicates.is_empty(),
            "Should find no duplicates below threshold"
        );
    }

    #[test]
    fn test_find_potential_duplicates_with_duplicates() {
        let matrix = vec![
            SimilarityPair {
                skill_id: "skill-a".to_string(),
                name: "Skill A".to_string(),
                similar_skills: vec![("skill-b".to_string(), 0.98), ("skill-c".to_string(), 0.50)],
            },
            SimilarityPair {
                skill_id: "skill-b".to_string(),
                name: "Skill B".to_string(),
                similar_skills: vec![
                    ("skill-a".to_string(), 0.98), // Should not be duplicated
                ],
            },
        ];

        let duplicates = find_potential_duplicates(&matrix, 0.95);
        assert_eq!(
            duplicates.len(),
            1,
            "Should find exactly one duplicate pair"
        );
        assert_eq!(duplicates[0].2, 0.98, "Similarity should be 0.98");

        // Check canonical ordering (skill-a comes before skill-b alphabetically)
        assert_eq!(duplicates[0].0, "skill-a");
        assert_eq!(duplicates[0].1, "skill-b");
    }
}
