//! Diagnostic and analysis commands for skill relationships and quality

use crate::cli::error::{CliError, CliResult};
use clap::{Args, Subcommand};
use fastskill::core::analysis::SimilarityPair;
use fastskill::core::vector_index::IndexedSkill;
use fastskill::FastSkillService;
use serde::Serialize;
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

    /// Group skills by semantic similarity
    #[command(
        about = "Group skills by semantic similarity",
        after_help = "Examples:\n  fastskill analyze cluster\n  fastskill analyze cluster -k 8\n  fastskill analyze cluster -k 10 --min-size 2\n  fastskill analyze cluster --json | jq '.[] | .representative'"
    )]
    Cluster(ClusterArgs),

    /// Find semantically duplicate or very similar skills
    #[command(
        about = "Find semantically duplicate or very similar skills",
        after_help = "Examples:\n  fastskill analyze duplicates\n  fastskill analyze duplicates --threshold 0.92\n  fastskill analyze duplicates --severity critical\n  fastskill analyze duplicates --json"
    )]
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

/// Cluster command arguments
#[derive(Debug, Args, Clone)]
pub struct ClusterArgs {
    /// Number of clusters to create
    #[arg(
        short = 'k',
        long,
        default_value = "5",
        help = "Number of clusters to create"
    )]
    pub num_clusters: usize,

    /// Suppress clusters with fewer than this many members
    #[arg(
        long,
        default_value = "1",
        help = "Hide clusters with fewer than N skills"
    )]
    pub min_size: usize,

    /// Output in JSON format
    #[arg(long, help = "Output in JSON format")]
    pub json: bool,
}

/// Severity filter for the duplicates command
#[derive(Debug, Clone, Default, clap::ValueEnum)]
pub enum SeverityFilter {
    /// Show all pairs above threshold
    #[default]
    All,
    /// Show medium, high, and critical pairs
    Medium,
    /// Show only high and critical pairs (similarity >= 0.93)
    High,
    /// Show only critical pairs (similarity >= 0.98)
    Critical,
}

/// Duplicates command arguments
#[derive(Debug, Args, Clone)]
pub struct DuplicatesArgs {
    /// Minimum cosine similarity to report
    #[arg(
        long,
        default_value = "0.88",
        value_parser = validate_threshold,
        help = "Minimum similarity to report"
    )]
    pub threshold: f32,

    /// Maximum number of pairs to display
    #[arg(long, default_value = "20", help = "Maximum number of pairs to show")]
    pub limit: usize,

    /// Only show pairs at or above this severity level
    #[arg(
        long,
        default_value = "all",
        value_enum,
        help = "Filter by minimum severity: critical, high, medium, all"
    )]
    pub severity: SeverityFilter,

    /// Output in JSON format
    #[arg(long, help = "Output in JSON format")]
    pub json: bool,
}

// --------------------------------------------------------------------------
// Output types for cluster
// --------------------------------------------------------------------------

#[derive(Debug, Serialize, Clone)]
struct ClusterMember {
    skill_id: String,
    name: String,
    distance_to_centroid: f32,
}

#[derive(Debug, Serialize, Clone)]
struct ClusterOutput {
    cluster_id: usize,
    representative: String,
    size: usize,
    members: Vec<ClusterMember>,
}

// --------------------------------------------------------------------------
// Output types for duplicates
// --------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
enum SeverityLevel {
    Critical,
    High,
    Medium,
}

impl SeverityLevel {
    fn label(&self) -> &'static str {
        match self {
            SeverityLevel::Critical => "[CRITICAL]",
            SeverityLevel::High => "[HIGH]    ",
            SeverityLevel::Medium => "[MEDIUM]  ",
        }
    }
}

#[derive(Debug, Serialize)]
struct DuplicateSkillInfo {
    id: String,
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    modified_at: Option<String>,
}

#[derive(Debug, Serialize)]
struct DuplicatePair {
    severity: SeverityLevel,
    similarity: f32,
    skill_a: DuplicateSkillInfo,
    skill_b: DuplicateSkillInfo,
    suggestion: String,
}

#[derive(Debug, Serialize)]
struct DuplicatesJsonOutput {
    threshold: f32,
    effective_floor: f32,
    total_pairs: usize,
    pairs: Vec<DuplicatePair>,
}

// --------------------------------------------------------------------------
// Validators
// --------------------------------------------------------------------------

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

// --------------------------------------------------------------------------
// Main dispatcher
// --------------------------------------------------------------------------

/// Main dispatcher for analyze subcommands
pub async fn execute_analyze(service: &FastSkillService, command: AnalyzeCommand) -> CliResult<()> {
    match command.command {
        AnalyzeSubcommand::Matrix(args) => execute_matrix(service, args).await,
        AnalyzeSubcommand::Cluster(args) => execute_cluster(service, args).await,
        AnalyzeSubcommand::Duplicates(args) => execute_duplicates(service, args).await,
    }
}

// --------------------------------------------------------------------------
// Matrix command
// --------------------------------------------------------------------------

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

// --------------------------------------------------------------------------
// Cluster command
// --------------------------------------------------------------------------

/// Execute the cluster command
pub async fn execute_cluster(service: &FastSkillService, args: ClusterArgs) -> CliResult<()> {
    let vector_index_service = service.vector_index_service().ok_or_else(|| {
        CliError::Config("Vector index not available. Run 'fastskill reindex' first.".to_string())
    })?;

    let mut all_skills: Vec<IndexedSkill> = vector_index_service
        .get_all_skills()
        .await
        .map_err(|e| CliError::Validation(format!("Failed to get indexed skills: {}", e)))?;

    if all_skills.is_empty() {
        println!("No skills indexed. Run 'fastskill reindex' first.");
        return Ok(());
    }

    let n = all_skills.len();
    let mut k = args.num_clusters;

    if k == 0 {
        k = 1;
    }

    if k > n {
        if !args.json {
            eprintln!("Warning: only {} skills available, reducing k to {}", n, n);
        }
        k = n;
    }

    if !args.json {
        println!("Clustering {} skills into {} groups...", n, k);
    }

    // Sort by id lexicographically for deterministic initialization
    all_skills.sort_by(|a, b| a.id.cmp(&b.id));

    if all_skills[0].embedding.is_empty() {
        return Err(CliError::Validation(
            "Skills have empty embeddings. Run 'fastskill reindex' to rebuild the index."
                .to_string(),
        ));
    }

    // Run spherical k-means
    let (assignments, centroids) = run_spherical_kmeans(&all_skills, k);

    // Build cluster outputs
    let mut clusters: Vec<ClusterOutput> = (0..k)
        .map(|c_idx| {
            let mut members: Vec<ClusterMember> = all_skills
                .iter()
                .enumerate()
                .filter(|(skill_idx, _)| assignments[*skill_idx] == c_idx)
                .map(|(_, skill)| {
                    let sim = cosine_similarity(&skill.embedding, &centroids[c_idx]);
                    let distance = (1.0 - sim).max(0.0);
                    ClusterMember {
                        skill_id: skill.id.clone(),
                        name: get_skill_name(&skill.frontmatter_json),
                        distance_to_centroid: distance,
                    }
                })
                .collect();

            // Sort members by distance ascending (closest first)
            members.sort_by(|a, b| {
                a.distance_to_centroid
                    .partial_cmp(&b.distance_to_centroid)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });

            let representative = members
                .first()
                .map(|m| m.skill_id.clone())
                .unwrap_or_default();

            ClusterOutput {
                cluster_id: c_idx,
                representative,
                size: members.len(),
                members,
            }
        })
        .collect();

    // Sort by size descending, ties by representative id lexicographically
    clusters.sort_by(|a, b| {
        b.size
            .cmp(&a.size)
            .then_with(|| a.representative.cmp(&b.representative))
    });

    // Reassign cluster_id (0-indexed, ordered by size desc)
    for (i, cluster) in clusters.iter_mut().enumerate() {
        cluster.cluster_id = i;
    }

    // Filter by min_size
    let clusters: Vec<ClusterOutput> = clusters
        .into_iter()
        .filter(|c| c.size >= args.min_size)
        .collect();

    if args.json {
        let json_output = serde_json::to_string_pretty(&clusters)
            .map_err(|e| CliError::Validation(format!("Failed to serialize JSON: {}", e)))?;
        println!("{}", json_output);
    } else {
        print_cluster_output(&clusters, n, k);
    }

    Ok(())
}

/// Spherical k-means: returns (assignments, final_centroids)
///
/// Uses deterministic initialization: sorts skills by id, picks k evenly-spaced indices.
/// Runs up to 100 iterations until convergence (assignments unchanged).
/// Handles empty clusters by redistributing the centroid to the farthest skill.
fn run_spherical_kmeans(skills: &[IndexedSkill], k: usize) -> (Vec<usize>, Vec<Vec<f32>>) {
    let n = skills.len();
    let embedding_dim = skills[0].embedding.len();

    // Initialize centroids: evenly spaced indices (skills already sorted by id)
    let mut centroids: Vec<Vec<f32>> = (0..k)
        .map(|i| {
            let idx = (i * n) / k;
            let mut c = skills[idx].embedding.clone();
            l2_normalize(&mut c);
            c
        })
        .collect();

    let mut assignments: Vec<usize> = vec![0; n];

    for _ in 0..100 {
        // Assignment: each skill → centroid with highest cosine similarity
        let new_assignments: Vec<usize> = skills
            .iter()
            .map(|skill| {
                centroids
                    .iter()
                    .enumerate()
                    .map(|(c_idx, centroid)| (c_idx, cosine_similarity(&skill.embedding, centroid)))
                    .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
                    .map(|(c_idx, _)| c_idx)
                    .unwrap_or(0)
            })
            .collect();

        // Convergence check
        if new_assignments == assignments {
            assignments = new_assignments;
            break;
        }
        assignments = new_assignments;

        // Recompute centroids as element-wise mean of members
        let mut new_centroids: Vec<Vec<f32>> = vec![vec![0.0_f32; embedding_dim]; k];
        let mut cluster_counts: Vec<usize> = vec![0; k];

        for (skill_idx, &c_idx) in assignments.iter().enumerate() {
            cluster_counts[c_idx] += 1;
            for (dim, &val) in skills[skill_idx].embedding.iter().enumerate() {
                new_centroids[c_idx][dim] += val;
            }
        }

        // Handle empty clusters: move centroid to skill farthest from its assigned centroid
        for c_idx in 0..k {
            if cluster_counts[c_idx] == 0 {
                let (farthest_idx, _) = assignments
                    .iter()
                    .enumerate()
                    .map(|(skill_idx, &cluster_idx)| {
                        let sim = cosine_similarity(
                            &skills[skill_idx].embedding,
                            &centroids[cluster_idx],
                        );
                        (skill_idx, sim)
                    })
                    .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
                    .unwrap_or((0, 0.0));

                let old_cluster = assignments[farthest_idx];
                assignments[farthest_idx] = c_idx;
                cluster_counts[old_cluster] -= 1;
                for (centroid_val, skill_val) in new_centroids[old_cluster]
                    .iter_mut()
                    .zip(skills[farthest_idx].embedding.iter())
                {
                    *centroid_val -= *skill_val;
                }
                cluster_counts[c_idx] = 1;
                new_centroids[c_idx] = skills[farthest_idx].embedding.clone();
            }
        }

        // Average and L2-normalize each centroid (spherical step)
        for (c_idx, centroid) in new_centroids.iter_mut().enumerate() {
            let count = cluster_counts[c_idx] as f32;
            if count > 1.0 {
                for val in centroid.iter_mut() {
                    *val /= count;
                }
            }
            l2_normalize(centroid);
        }

        centroids = new_centroids;
    }

    (assignments, centroids)
}

/// L2-normalize a slice in-place
fn l2_normalize(v: &mut [f32]) {
    let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 1e-10 {
        for x in v.iter_mut() {
            *x /= norm;
        }
    }
}

/// Print cluster output in human-readable format
fn print_cluster_output(clusters: &[ClusterOutput], total_skills: usize, actual_k: usize) {
    if clusters.is_empty() {
        println!("\nNo clusters to display (all clusters are below the --min-size threshold).");
        return;
    }

    println!();
    for (display_idx, cluster) in clusters.iter().enumerate() {
        println!(
            "Cluster {} — {} ({} skills)",
            display_idx + 1,
            cluster.representative,
            cluster.size
        );
        println!("  Members:");
        for member in &cluster.members {
            println!(
                "    {:<30} {:.2}",
                member.skill_id, member.distance_to_centroid
            );
        }
        println!();
    }

    let mut largest = &clusters[0];
    let mut smallest = &clusters[0];
    for cluster in clusters.iter().skip(1) {
        if cluster.size > largest.size {
            largest = cluster;
        }
        if cluster.size < smallest.size {
            smallest = cluster;
        }
    }

    println!(
        "Summary: {} clusters  largest: {} skills ({})  smallest: {} skills ({})",
        clusters.len(),
        largest.size,
        largest.representative,
        smallest.size,
        smallest.representative
    );

    // Hint: largest > 2 * mean AND actual_k < total_skills
    if actual_k < total_skills {
        let mean_size = total_skills as f32 / actual_k as f32;
        if largest.size as f32 > 2.0 * mean_size {
            println!(
                "Hint: Re-run with -k {} to split larger clusters further.",
                largest.size
            );
        }
    }
}

// --------------------------------------------------------------------------
// Duplicates command
// --------------------------------------------------------------------------

/// Execute the duplicates command
pub async fn execute_duplicates(service: &FastSkillService, args: DuplicatesArgs) -> CliResult<()> {
    let vector_index_service = service.vector_index_service().ok_or_else(|| {
        CliError::Config("Vector index not available. Run 'fastskill reindex' first.".to_string())
    })?;

    let all_skills: Vec<IndexedSkill> = vector_index_service
        .get_all_skills()
        .await
        .map_err(|e| CliError::Validation(format!("Failed to get indexed skills: {}", e)))?;

    if all_skills.is_empty() {
        println!("No skills indexed. Run 'fastskill reindex' first.");
        return Ok(());
    }

    let n = all_skills.len();
    let s_floor = severity_floor_value(&args.severity, args.threshold);
    let effective_floor = args.threshold.max(s_floor);

    if !args.json {
        println!(
            "Scanning {} skills for duplicates (threshold: {:.2})...",
            n, args.threshold
        );
    }

    // Compute all unique pairs
    let mut pairs: Vec<DuplicatePair> = Vec::new();
    for i in 0..n {
        for j in (i + 1)..n {
            let sim = cosine_similarity(&all_skills[i].embedding, &all_skills[j].embedding);
            if sim < effective_floor {
                continue;
            }

            // Canonical ordering: lexicographically smaller id first
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

    // Sort by similarity descending, then skill_a.id alphabetically
    pairs.sort_by(|a, b| {
        b.similarity
            .partial_cmp(&a.similarity)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.skill_a.id.cmp(&b.skill_a.id))
    });

    // Apply limit
    pairs.truncate(args.limit);

    if args.json {
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

        let critical_count = pairs
            .iter()
            .filter(|p| p.severity == SeverityLevel::Critical)
            .count();
        let high_count = pairs
            .iter()
            .filter(|p| p.severity == SeverityLevel::High)
            .count();
        let medium_count = pairs
            .iter()
            .filter(|p| p.severity == SeverityLevel::Medium)
            .count();

        let mut summary_parts = Vec::new();
        if critical_count > 0 {
            summary_parts.push(format!("{} critical", critical_count));
        }
        if high_count > 0 {
            summary_parts.push(format!("{} high", high_count));
        }
        if medium_count > 0 {
            summary_parts.push(format!("{} medium", medium_count));
        }

        println!("Summary: {}", summary_parts.join(", "));
        println!("Run 'fastskill remove <skill-id>' to remove a skill after review.");
    }

    Ok(())
}

/// Returns the severity floor value for a given filter
fn severity_floor_value(filter: &SeverityFilter, threshold: f32) -> f32 {
    match filter {
        SeverityFilter::All => 0.0,
        SeverityFilter::Medium => threshold,
        SeverityFilter::High => 0.93,
        SeverityFilter::Critical => 0.98,
    }
}

/// Classify a similarity score into a severity level
fn classify_severity(similarity: f32) -> Option<SeverityLevel> {
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

/// Get the file modification time as an ISO 8601 UTC string
fn get_file_mtime(path: &std::path::Path) -> Option<String> {
    let metadata = std::fs::metadata(path).ok()?;
    let modified = metadata.modified().ok()?;
    let dt = chrono::DateTime::<chrono::Utc>::from(modified);
    Some(dt.format("%Y-%m-%dT%H:%M:%SZ").to_string())
}

/// Compute a suggestion for a duplicate pair based on file modification times
fn compute_suggestion(skill_a: &IndexedSkill, skill_b: &IndexedSkill) -> String {
    let mtime_a = std::fs::metadata(&skill_a.skill_path)
        .and_then(|m| m.modified())
        .ok();
    let mtime_b = std::fs::metadata(&skill_b.skill_path)
        .and_then(|m| m.modified())
        .ok();

    match (mtime_a, mtime_b) {
        (Some(ta), Some(tb)) => {
            let diff_secs = if ta > tb {
                ta.duration_since(tb).map(|d| d.as_secs()).unwrap_or(0)
            } else {
                tb.duration_since(ta).map(|d| d.as_secs()).unwrap_or(0)
            };

            if diff_secs > 60 {
                let now = std::time::SystemTime::now();
                let (newer_id, newer_time, older_id, older_time) = if ta > tb {
                    (&skill_a.id, ta, &skill_b.id, tb)
                } else {
                    (&skill_b.id, tb, &skill_a.id, ta)
                };
                let days_newer = now
                    .duration_since(newer_time)
                    .map(|d| d.as_secs() / 86400)
                    .unwrap_or(0);
                let days_older = now
                    .duration_since(older_time)
                    .map(|d| d.as_secs() / 86400)
                    .unwrap_or(0);
                format!(
                    "Keep {} (modified {} days ago), review {} (modified {} days ago)",
                    newer_id, days_newer, older_id, days_older
                )
            } else {
                "Review both — similar modification time".to_string()
            }
        }
        _ => "Review both — similar modification time".to_string(),
    }
}

// --------------------------------------------------------------------------
// Shared helpers
// --------------------------------------------------------------------------

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

// --------------------------------------------------------------------------
// Matrix display helpers (unchanged)
// --------------------------------------------------------------------------

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

// --------------------------------------------------------------------------
// Tests
// --------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // --- cosine_similarity tests ---

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

    // --- validate_threshold tests ---

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

    // --- get_skill_name tests ---

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

    // --- l2_normalize tests ---

    #[test]
    fn test_l2_normalize_unit_vector() {
        let mut v = vec![1.0_f32, 0.0, 0.0];
        l2_normalize(&mut v);
        assert!((v[0] - 1.0).abs() < 1e-6);
        assert!((v[1] - 0.0).abs() < 1e-6);
    }

    #[test]
    fn test_l2_normalize_general_vector() {
        let mut v = vec![3.0_f32, 4.0];
        l2_normalize(&mut v);
        let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!(
            (norm - 1.0).abs() < 1e-5,
            "Normalized vector should have unit norm"
        );
    }

    #[test]
    fn test_l2_normalize_zero_vector() {
        let mut v = vec![0.0_f32, 0.0, 0.0];
        l2_normalize(&mut v); // Should not panic
        assert_eq!(v, vec![0.0, 0.0, 0.0]);
    }

    // --- classify_severity tests ---

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
        // Just below critical
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
        // Just below high
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

    // --- severity_floor_value tests ---

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

    // --- effective_floor tests (threshold.max(severity_floor)) ---

    #[test]
    fn test_effective_floor_threshold_dominates() {
        // threshold=0.99, severity=high → effective = max(0.99, 0.93) = 0.99
        let floor = severity_floor_value(&SeverityFilter::High, 0.99);
        let effective = 0.99_f32.max(floor);
        assert!((effective - 0.99).abs() < 1e-6);
    }

    #[test]
    fn test_effective_floor_severity_dominates() {
        // threshold=0.80, severity=high → effective = max(0.80, 0.93) = 0.93
        let floor = severity_floor_value(&SeverityFilter::High, 0.80);
        let effective = 0.80_f32.max(floor);
        assert!((effective - 0.93).abs() < 1e-6);
    }

    // --- spherical k-means tests ---

    fn make_indexed_skill(id: &str, embedding: Vec<f32>) -> IndexedSkill {
        use std::path::PathBuf;
        IndexedSkill {
            id: id.to_string(),
            skill_path: PathBuf::from(format!("/tmp/{}", id)),
            frontmatter_json: serde_json::json!({"name": id}),
            embedding,
            file_hash: "test".to_string(),
            updated_at: chrono::Utc::now(),
        }
    }

    #[test]
    fn test_spherical_kmeans_two_clear_clusters() {
        // 4 skills: 2 near (1,0,0,0) and 2 near (0,1,0,0)
        let skills = vec![
            make_indexed_skill("aaa-git-amend", vec![0.9806, 0.1961, 0.0, 0.0]),
            make_indexed_skill("aaa-git-commit", vec![0.9950, 0.0998, 0.0, 0.0]),
            make_indexed_skill("bbb-code-review", vec![0.0, 0.9950, 0.0998, 0.0]),
            make_indexed_skill("bbb-pr-review", vec![0.0, 0.9806, 0.1961, 0.0]),
        ];

        let (assignments, _centroids) = run_spherical_kmeans(&skills, 2);

        // Skills 0 and 1 (aaa-*) should be in the same cluster
        assert_eq!(
            assignments[0], assignments[1],
            "aaa-git-amend and aaa-git-commit should be in the same cluster"
        );
        // Skills 2 and 3 (bbb-*) should be in the same cluster
        assert_eq!(
            assignments[2], assignments[3],
            "bbb-code-review and bbb-pr-review should be in the same cluster"
        );
        // The two clusters should be different
        assert_ne!(
            assignments[0], assignments[2],
            "The two clusters should be distinct"
        );
    }

    #[test]
    fn test_spherical_kmeans_k_equals_n() {
        // When k == n, each skill should be in its own cluster
        let skills = vec![
            make_indexed_skill("skill-a", vec![1.0, 0.0]),
            make_indexed_skill("skill-b", vec![0.0, 1.0]),
        ];
        let (assignments, _) = run_spherical_kmeans(&skills, 2);
        assert_ne!(
            assignments[0], assignments[1],
            "k=n: all skills in different clusters"
        );
    }

    #[test]
    fn test_spherical_kmeans_k_equals_one() {
        let skills = vec![
            make_indexed_skill("skill-a", vec![1.0, 0.0]),
            make_indexed_skill("skill-b", vec![0.0, 1.0]),
            make_indexed_skill("skill-c", vec![0.5, 0.5]),
        ];
        let (assignments, _) = run_spherical_kmeans(&skills, 1);
        assert!(
            assignments.iter().all(|&a| a == 0),
            "k=1: all skills in cluster 0"
        );
    }

    // --- print_cluster_output tests (smoke tests) ---

    #[test]
    fn test_print_cluster_output_empty() {
        // Should not panic on empty clusters
        print_cluster_output(&[], 0, 1);
    }

    #[test]
    fn test_cluster_output_json_serialization() {
        let cluster = ClusterOutput {
            cluster_id: 0,
            representative: "my-skill".to_string(),
            size: 2,
            members: vec![ClusterMember {
                skill_id: "my-skill".to_string(),
                name: "My Skill".to_string(),
                distance_to_centroid: 0.05,
            }],
        };
        let json = serde_json::to_string(&cluster).unwrap();
        assert!(json.contains("my-skill"));
        assert!(json.contains("cluster_id"));
        assert!(json.contains("representative"));
    }

    // --- duplicates JSON output tests ---

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

        let json = serde_json::to_string_pretty(&output).unwrap();
        assert!(json.contains("\"effective_floor\""));
        assert!(json.contains("0.93"));
        assert!(json.contains("\"high\""));
        assert!(json.contains("skill-a"));
        // modified_at: None should be omitted (skip_serializing_if)
        assert!(!json.contains("\"modified_at\": null"));
    }
}
