//! Cluster command — group skills by semantic similarity using spherical k-means.

use super::helpers::get_skill_name;
use super::AnalysisContext;
use crate::commands::common::validate_format_args;
use crate::error::{CliError, CliResult};
use clap::Args;
use fastskill_core::core::analysis::skill_similarity;
use fastskill_core::OutputFormat;
use serde::Serialize;

#[derive(Debug, Args, Clone)]
pub struct ClusterArgs {
    #[arg(
        short = 'k',
        long,
        default_value = "5",
        help = "Number of clusters to create"
    )]
    pub num_clusters: usize,
    #[arg(
        long,
        default_value = "1",
        help = "Hide clusters with fewer than N skills"
    )]
    pub min_size: usize,
    #[arg(long, value_enum, help = "Output format: table, json, grid, xml")]
    pub format: Option<OutputFormat>,
    #[arg(long, help = "Shorthand for --format json")]
    pub json: bool,
}

#[derive(Debug, Serialize, Clone)]
pub(super) struct ClusterMember {
    pub skill_id: String,
    pub name: String,
    pub distance_to_centroid: f32,
}

#[derive(Debug, Serialize, Clone)]
pub(super) struct ClusterOutput {
    pub cluster_id: usize,
    pub representative: String,
    pub size: usize,
    pub members: Vec<ClusterMember>,
}

/// Execute the cluster command
pub async fn execute_cluster(ctx: AnalysisContext, args: ClusterArgs) -> CliResult<()> {
    let format = validate_format_args(&args.format, args.json)?;
    let use_json = format == OutputFormat::Json;
    let mut all_skills = ctx.skills;

    let n = all_skills.len();
    let mut k = args.num_clusters;

    if k == 0 {
        k = 1;
    }
    if k > n {
        if !use_json {
            eprintln!("Warning: only {} skills available, reducing k to {}", n, n);
        }
        k = n;
    }

    if !use_json {
        println!("Clustering {} skills into {} groups...", n, k);
    }

    all_skills.sort_by(|a, b| a.id.cmp(&b.id));

    if all_skills[0].embedding.is_empty() {
        return Err(CliError::Validation(
            "Skills have empty embeddings. Run 'fastskill reindex' to rebuild the index."
                .to_string(),
        ));
    }

    let embeddings: Vec<Vec<f32>> = all_skills.iter().map(|s| s.embedding.clone()).collect();
    let (assignments, centroids) = fastskill_core::core::analysis::cluster_skills(&embeddings, k);

    let mut clusters: Vec<ClusterOutput> = (0..k)
        .map(|c_idx| {
            let mut members: Vec<ClusterMember> = all_skills
                .iter()
                .enumerate()
                .filter(|(skill_idx, _)| assignments[*skill_idx] == c_idx)
                .map(|(_, skill)| {
                    let sim = skill_similarity(&skill.embedding, &centroids[c_idx]);
                    ClusterMember {
                        skill_id: skill.id.clone(),
                        name: get_skill_name(&skill.frontmatter_json),
                        distance_to_centroid: (1.0 - sim).max(0.0),
                    }
                })
                .collect();

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

    clusters.sort_by(|a, b| {
        b.size
            .cmp(&a.size)
            .then_with(|| a.representative.cmp(&b.representative))
    });

    for (i, cluster) in clusters.iter_mut().enumerate() {
        cluster.cluster_id = i;
    }

    let clusters: Vec<ClusterOutput> = clusters
        .into_iter()
        .filter(|c| c.size >= args.min_size)
        .collect();

    if use_json {
        let json_output = serde_json::to_string_pretty(&clusters)
            .map_err(|e| CliError::Validation(format!("Failed to serialize JSON: {}", e)))?;
        println!("{}", json_output);
    } else {
        print_cluster_output(&clusters, n, k);
    }

    Ok(())
}

pub(super) fn print_cluster_output(
    clusters: &[ClusterOutput],
    total_skills: usize,
    actual_k: usize,
) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_print_cluster_output_empty() {
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
        #[allow(clippy::expect_used)]
        let json = serde_json::to_string(&cluster).expect("Cluster serialization should not fail");
        assert!(json.contains("my-skill"));
        assert!(json.contains("cluster_id"));
        assert!(json.contains("representative"));
    }
}
