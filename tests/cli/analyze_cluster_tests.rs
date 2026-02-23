//! Integration tests for `fastskill analyze cluster` and `fastskill analyze duplicates`

#![allow(clippy::all, clippy::unwrap_used, clippy::expect_used)]

use fastskill::{VectorIndexService, VectorIndexServiceImpl};
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

use super::snapshot_helpers::{
    assert_snapshot_with_settings, cli_snapshot_settings, run_fastskill_command_with_env,
};

// --------------------------------------------------------------------------
// TOML template
// --------------------------------------------------------------------------

/// skill-project.toml content for tests.
/// skills_directory is a relative path from the project root (temp_dir).
const PROJECT_TOML: &str = r#"[dependencies]

[tool.fastskill]
skills_directory = ".claude/skills"

[tool.fastskill.embedding]
openai_base_url = "https://api.openai.com/v1"
embedding_model = "text-embedding-3-small"
"#;

// --------------------------------------------------------------------------
// Fixture helpers
// --------------------------------------------------------------------------

/// Set up a fresh temp workspace with a skill-project.toml and an empty skills dir.
///
/// Returns the TempDir (keep alive for test duration) and the skills dir path.
fn setup_workspace() -> (TempDir, PathBuf) {
    let temp = TempDir::new().unwrap();
    let skills_dir = temp.path().join(".claude").join("skills");
    fs::create_dir_all(&skills_dir).unwrap();
    fs::write(temp.path().join("skill-project.toml"), PROJECT_TOML).unwrap();
    (temp, skills_dir)
}

/// L2-normalize a vector (returns normalized copy).
fn normalize_vec(mut v: Vec<f32>) -> Vec<f32> {
    let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 1e-10 {
        for x in v.iter_mut() {
            *x /= norm;
        }
    }
    v
}

/// Add a skill to the vector index (creates SKILL.md stub + DB entry).
async fn add_skill(
    index: &VectorIndexServiceImpl,
    skills_dir: &Path,
    id: &str,
    embedding: Vec<f32>,
) {
    let skill_dir = skills_dir.join(id);
    fs::create_dir_all(&skill_dir).unwrap();
    fs::write(
        skill_dir.join("SKILL.md"),
        format!("# Skill\nName: {}\n", id),
    )
    .unwrap();
    index
        .add_or_update_skill(
            id,
            skill_dir,
            serde_json::json!({"name": id}),
            embedding,
            "test_hash",
        )
        .await
        .unwrap();
}

/// Creates a cluster fixture:
/// - "aaa-*" skills: embeddings near (1,0,0,0)  — "git" cluster (2 skills)
/// - "bbb-*" skills: embeddings near (0,1,0,0)  — "review" cluster (4 skills)
async fn create_cluster_fixture() -> TempDir {
    let (temp, skills_dir) = setup_workspace();
    let index = VectorIndexServiceImpl::with_default_path(&skills_dir);

    // Git cluster — near (1, 0, 0, 0)
    add_skill(
        &index,
        &skills_dir,
        "aaa-git-amend",
        normalize_vec(vec![0.9806, 0.1961, 0.0, 0.0]),
    )
    .await;
    add_skill(
        &index,
        &skills_dir,
        "aaa-git-commit",
        normalize_vec(vec![0.9950, 0.0998, 0.0, 0.0]),
    )
    .await;

    // Review cluster — near (0, 1, 0, 0)
    add_skill(
        &index,
        &skills_dir,
        "bbb-code-review",
        normalize_vec(vec![0.0, 0.9950, 0.0998, 0.0]),
    )
    .await;
    add_skill(
        &index,
        &skills_dir,
        "bbb-pr-review",
        normalize_vec(vec![0.0, 0.9806, 0.1961, 0.0]),
    )
    .await;
    add_skill(
        &index,
        &skills_dir,
        "bbb-review-helper",
        normalize_vec(vec![0.0, 0.9700, 0.2425, 0.0]),
    )
    .await;
    add_skill(
        &index,
        &skills_dir,
        "bbb-pr-writer",
        normalize_vec(vec![0.0, 0.9553, 0.2958, 0.0]),
    )
    .await;

    temp
}

/// Creates a duplicates fixture with one pair per severity level:
///
/// - critical (≥0.98):  dup-crit-a / dup-crit-b  — near dim-0
/// - high     (≥0.93):  dup-high-a / dup-high-b  — near dim-2
/// - medium   (≥0.88):  dup-med-a  / dup-med-b   — near dim-4
///
/// Cross-pair similarity ≈ 0 (orthogonal 6D sub-spaces).
async fn create_duplicates_fixture() -> TempDir {
    let (temp, skills_dir) = setup_workspace();
    let index = VectorIndexServiceImpl::with_default_path(&skills_dir);

    // Critical pair (~0.9990)
    add_skill(
        &index,
        &skills_dir,
        "dup-crit-a",
        vec![1.0, 0.0, 0.0, 0.0, 0.0, 0.0],
    )
    .await;
    add_skill(
        &index,
        &skills_dir,
        "dup-crit-b",
        normalize_vec(vec![0.9990, 0.0447, 0.0, 0.0, 0.0, 0.0]),
    )
    .await;

    // High pair (~0.9397)
    add_skill(
        &index,
        &skills_dir,
        "dup-high-a",
        vec![0.0, 0.0, 1.0, 0.0, 0.0, 0.0],
    )
    .await;
    add_skill(
        &index,
        &skills_dir,
        "dup-high-b",
        normalize_vec(vec![0.0, 0.0, 0.9397, 0.342, 0.0, 0.0]),
    )
    .await;

    // Medium pair (~0.9010)
    add_skill(
        &index,
        &skills_dir,
        "dup-med-a",
        vec![0.0, 0.0, 0.0, 0.0, 1.0, 0.0],
    )
    .await;
    add_skill(
        &index,
        &skills_dir,
        "dup-med-b",
        normalize_vec(vec![0.0, 0.0, 0.0, 0.0, 0.9010, 0.4338]),
    )
    .await;

    temp
}

/// Creates a duplicates fixture with only medium and high pairs (no critical).
async fn create_duplicates_no_critical_fixture() -> TempDir {
    let (temp, skills_dir) = setup_workspace();
    let index = VectorIndexServiceImpl::with_default_path(&skills_dir);

    // High pair (~0.9397) — near dim-0
    add_skill(&index, &skills_dir, "high-a", vec![1.0, 0.0, 0.0, 0.0]).await;
    add_skill(
        &index,
        &skills_dir,
        "high-b",
        normalize_vec(vec![0.9397, 0.342, 0.0, 0.0]),
    )
    .await;

    // Medium pair (~0.901) — near dim-2
    add_skill(&index, &skills_dir, "med-a", vec![0.0, 0.0, 1.0, 0.0]).await;
    add_skill(
        &index,
        &skills_dir,
        "med-b",
        normalize_vec(vec![0.0, 0.0, 0.9010, 0.4338]),
    )
    .await;

    temp
}

/// Run a fastskill command with the given working directory and no extra env vars.
fn run_in(args: &[&str], working_dir: &Path) -> super::snapshot_helpers::CommandResult {
    run_fastskill_command_with_env(args, &[], Some(working_dir))
}

fn assert_cli_snapshot(name: &str, stdout: &str) {
    assert_snapshot_with_settings(name, stdout, &cli_snapshot_settings());
}

fn assert_cli_snapshot_stderr(name: &str, stderr: &str) {
    assert_snapshot_with_settings(name, stderr, &cli_snapshot_settings());
}

/// Parse JSON from CLI stdout, skipping the "FastSkill X.Y.Z" version banner
/// and any tracing log lines (which carry ANSI escape codes and start with \x1b).
/// The JSON content always starts on a line beginning with a plain `[` or `{`.
fn parse_json(stdout: &str) -> serde_json::Value {
    // Skip lines until we reach the first line whose first non-whitespace byte
    // is '[' or '{' — that is the start of the JSON output.
    // Tracing lines begin with \x1b (ESC) for ANSI colour, so they are safely
    // skipped by this check even though they contain '[' later in the line.
    let json_str: String = stdout
        .lines()
        .skip_while(|line| {
            let t = line.trim_start();
            !t.starts_with('[') && !t.starts_with('{')
        })
        .collect::<Vec<&str>>()
        .join("\n");
    serde_json::from_str(json_str.trim())
        .unwrap_or_else(|e| panic!("Output is not valid JSON: {}\nRaw stdout: {}", e, stdout))
}

// --------------------------------------------------------------------------
// cluster: help
// --------------------------------------------------------------------------

#[test]
fn test_cluster_help() {
    let result = run_fastskill_command_with_env(&["analyze", "cluster", "--help"], &[], None);
    assert!(result.success, "cluster --help should succeed");
    assert!(
        result.stdout.contains("cluster") || result.stderr.contains("cluster"),
        "Output should mention cluster"
    );
    assert!(
        result.stdout.contains("-k") || result.stdout.contains("num-clusters"),
        "Help should mention -k/--num-clusters: {}",
        result.stdout
    );
    assert!(
        result.stdout.contains("min-size"),
        "Help should mention --min-size: {}",
        result.stdout
    );
    assert!(
        result.stdout.contains("json"),
        "Help should mention --json: {}",
        result.stdout
    );
}

// --------------------------------------------------------------------------
// cluster: no index (empty skills dir)
// --------------------------------------------------------------------------

#[tokio::test]
async fn test_cluster_no_index() {
    let (temp, _skills_dir) = setup_workspace();
    // No skills added → index is empty
    let result = run_in(&["analyze", "cluster"], temp.path());
    assert!(result.success, "cluster with empty index should exit 0");
    assert!(
        result.stdout.contains("No skills indexed") || result.stdout.contains("reindex"),
        "Should tell user to reindex: stdout={}",
        result.stdout
    );
}

// --------------------------------------------------------------------------
// cluster: 6 skills, k=2 → two stable clusters
// --------------------------------------------------------------------------

#[tokio::test]
async fn test_cluster_k2_six_skills() {
    let temp = create_cluster_fixture().await;
    let result = run_in(&["analyze", "cluster", "-k", "2"], temp.path());

    assert!(
        result.success,
        "cluster -k 2 should succeed; stderr={}",
        result.stderr
    );

    let out = &result.stdout;

    // Should mention clustering message
    assert!(
        out.contains("Clustering 6 skills into 2 groups"),
        "Should show clustering message: {}",
        out
    );

    // Should show exactly 2 clusters
    assert!(out.contains("Cluster 1"), "Should have Cluster 1: {}", out);
    assert!(out.contains("Cluster 2"), "Should have Cluster 2: {}", out);
    assert!(
        !out.contains("Cluster 3"),
        "Should not have Cluster 3: {}",
        out
    );

    // Summary should show 2 clusters
    assert!(
        out.contains("Summary: 2 clusters"),
        "Should have summary with 2 clusters: {}",
        out
    );

    // The review cluster (4 skills) and git cluster (2 skills) should appear
    // Cluster 1 should be the larger one (4 bbb-* skills)
    // Cluster 2 should be the smaller one (2 aaa-* skills)
    assert!(
        out.contains("bbb-") || out.contains("aaa-"),
        "Should mention skill IDs: {}",
        out
    );

    // Members section should appear
    assert!(out.contains("Members:"), "Should list members: {}", out);
}

// --------------------------------------------------------------------------
// cluster: JSON output
// --------------------------------------------------------------------------

#[tokio::test]
async fn test_cluster_json_output() {
    let temp = create_cluster_fixture().await;
    let result = run_in(&["analyze", "cluster", "-k", "2", "--json"], temp.path());

    assert!(
        result.success,
        "cluster --json should succeed; stderr={}",
        result.stderr
    );

    let json: serde_json::Value = parse_json(&result.stdout);

    let arr = json.as_array().expect("JSON output should be an array");
    assert_eq!(arr.len(), 2, "Should have 2 clusters in JSON output");

    // Each element should have the required fields
    for cluster in arr {
        assert!(
            cluster.get("cluster_id").is_some(),
            "Should have cluster_id"
        );
        assert!(
            cluster.get("representative").is_some(),
            "Should have representative"
        );
        assert!(cluster.get("size").is_some(), "Should have size");
        let members = cluster.get("members").expect("Should have members");
        let member_arr = members.as_array().expect("members should be array");
        assert!(!member_arr.is_empty(), "Members should not be empty");

        // Check member fields
        for member in member_arr {
            assert!(
                member.get("skill_id").is_some(),
                "member should have skill_id"
            );
            assert!(member.get("name").is_some(), "member should have name");
            assert!(
                member.get("distance_to_centroid").is_some(),
                "member should have distance_to_centroid"
            );
        }
    }

    // cluster_id values should be 0 and 1
    let ids: Vec<u64> = arr
        .iter()
        .map(|c| c["cluster_id"].as_u64().unwrap())
        .collect();
    assert!(ids.contains(&0), "Should have cluster_id=0");
    assert!(ids.contains(&1), "Should have cluster_id=1");

    // Sorted by size descending
    let first_size = arr[0]["size"].as_u64().unwrap();
    let second_size = arr[1]["size"].as_u64().unwrap();
    assert!(
        first_size >= second_size,
        "Clusters should be sorted by size desc: {} >= {}",
        first_size,
        second_size
    );

    // The larger cluster should be the review cluster (4 skills)
    assert_eq!(
        first_size, 4,
        "Largest cluster should have 4 skills (bbb-*): got {}",
        first_size
    );
    assert_eq!(
        second_size, 2,
        "Smaller cluster should have 2 skills (aaa-*): got {}",
        second_size
    );
}

// --------------------------------------------------------------------------
// cluster: k > N → warning in stderr, reduces to N
// --------------------------------------------------------------------------

#[tokio::test]
async fn test_cluster_k_greater_than_n() {
    let temp = create_cluster_fixture().await;
    // fixture has 6 skills; request k=10
    let result = run_in(&["analyze", "cluster", "-k", "10"], temp.path());

    assert!(
        result.success,
        "cluster -k 10 (N=6) should still succeed; stderr={}",
        result.stderr
    );

    // Warning should be in stderr
    assert!(
        result.stderr.contains("Warning") || result.stderr.contains("reducing k"),
        "Should warn about reducing k: stderr={}",
        result.stderr
    );

    // Should produce 6 groups (k reduced to N=6)
    let out = &result.stdout;
    assert!(
        out.contains("Clustering 6 skills into 6 groups"),
        "Should cluster into 6 (reduced) groups: {}",
        out
    );
    assert!(
        out.contains("Summary: 6 clusters"),
        "Summary should show 6 clusters: {}",
        out
    );
}

// --------------------------------------------------------------------------
// cluster: --min-size filters small clusters
// --------------------------------------------------------------------------

#[tokio::test]
async fn test_cluster_min_size_filters_small_clusters() {
    let temp = create_cluster_fixture().await;
    // 6 skills: 4 in "bbb" cluster and 2 in "aaa" cluster
    // With k=2 and --min-size 4, the small cluster (2 skills) should be hidden

    let result = run_in(
        &["analyze", "cluster", "-k", "2", "--min-size", "4"],
        temp.path(),
    );

    assert!(
        result.success,
        "cluster -k 2 --min-size 4 should succeed; stderr={}",
        result.stderr
    );

    let out = &result.stdout;

    // Should show Cluster 1 (the large one with 4 bbb-* skills)
    assert!(out.contains("Cluster 1"), "Should show Cluster 1: {}", out);

    // Should NOT show Cluster 2 (the small one with 2 skills, below --min-size 4)
    assert!(
        !out.contains("Cluster 2"),
        "Should not show Cluster 2 (below min-size 4): {}",
        out
    );

    // The cluster shown should have 4 skills
    assert!(
        out.contains("4 skills"),
        "Should show cluster with 4 skills: {}",
        out
    );
}

// --------------------------------------------------------------------------
// cluster: --min-size JSON only shows qualifying clusters
// --------------------------------------------------------------------------

#[tokio::test]
async fn test_cluster_min_size_json() {
    let temp = create_cluster_fixture().await;

    let result = run_in(
        &["analyze", "cluster", "-k", "2", "--min-size", "4", "--json"],
        temp.path(),
    );

    assert!(
        result.success,
        "cluster -k 2 --min-size 4 --json should succeed; stderr={}",
        result.stderr
    );

    let json: serde_json::Value = parse_json(&result.stdout);
    let arr = json.as_array().expect("Should be array");

    // Only clusters with size >= 4 should appear
    assert!(
        !arr.is_empty(),
        "At least one cluster should pass min-size=4"
    );
    for cluster in arr {
        let size = cluster["size"].as_u64().unwrap();
        assert!(
            size >= 4,
            "All returned clusters should have size >= 4, got {}",
            size
        );
    }
}

// --------------------------------------------------------------------------
// cluster: k=1 — all in one cluster
// --------------------------------------------------------------------------

#[tokio::test]
async fn test_cluster_k1() {
    let temp = create_cluster_fixture().await;
    let result = run_in(&["analyze", "cluster", "-k", "1"], temp.path());

    assert!(
        result.success,
        "cluster -k 1 should succeed; stderr={}",
        result.stderr
    );

    let out = &result.stdout;
    assert!(
        out.contains("Clustering 6 skills into 1"),
        "Should show 1 group: {}",
        out
    );
    // Summary: 1 cluster
    assert!(
        out.contains("Summary: 1 cluster"),
        "Summary should show 1 cluster: {}",
        out
    );
    // Should have 6 members total
    assert!(
        out.contains("6 skills"),
        "Cluster should show 6 skills: {}",
        out
    );
}

// --------------------------------------------------------------------------
// duplicates: help
// --------------------------------------------------------------------------

#[test]
fn test_duplicates_help() {
    let result = run_fastskill_command_with_env(&["analyze", "duplicates", "--help"], &[], None);
    assert!(result.success, "duplicates --help should succeed");
    let out = &result.stdout;
    assert!(
        out.contains("threshold"),
        "Help should mention --threshold: {}",
        out
    );
    assert!(
        out.contains("limit"),
        "Help should mention --limit: {}",
        out
    );
    assert!(
        out.contains("severity"),
        "Help should mention --severity: {}",
        out
    );
    assert!(out.contains("json"), "Help should mention --json: {}", out);
}

// --------------------------------------------------------------------------
// duplicates: no index (empty skills dir)
// --------------------------------------------------------------------------

#[tokio::test]
async fn test_duplicates_no_index() {
    let (temp, _) = setup_workspace();
    let result = run_in(&["analyze", "duplicates"], temp.path());
    assert!(result.success, "duplicates with empty index should exit 0");
    assert!(
        result.stdout.contains("No skills indexed") || result.stdout.contains("reindex"),
        "Should tell user to reindex: stdout={}",
        result.stdout
    );
}

// --------------------------------------------------------------------------
// duplicates: all three severity levels in fixture
// --------------------------------------------------------------------------

#[tokio::test]
async fn test_duplicates_all_pairs() {
    let temp = create_duplicates_fixture().await;
    let result = run_in(&["analyze", "duplicates"], temp.path());

    assert!(
        result.success,
        "duplicates should succeed; stderr={}",
        result.stderr
    );

    let out = &result.stdout;

    // Should show scanning message
    assert!(out.contains("Scanning"), "Should show scanning: {}", out);

    // Should find pairs
    assert!(
        out.contains("Found") && out.contains("pairs"),
        "Should find pairs: {}",
        out
    );

    // All three severity levels should appear
    assert!(out.contains("[CRITICAL]"), "Should show CRITICAL: {}", out);
    assert!(out.contains("[HIGH]"), "Should show HIGH: {}", out);
    assert!(out.contains("[MEDIUM]"), "Should show MEDIUM: {}", out);

    // Should use ↔ separator
    assert!(out.contains('↔'), "Should use ↔: {}", out);

    // Should show Suggestion
    assert!(
        out.contains("Suggestion:"),
        "Should show Suggestion: {}",
        out
    );

    // Should show summary
    assert!(out.contains("Summary:"), "Should show Summary: {}", out);
    assert!(
        out.contains("critical"),
        "Summary should mention critical: {}",
        out
    );
    assert!(out.contains("high"), "Summary should mention high: {}", out);
    assert!(
        out.contains("medium"),
        "Summary should mention medium: {}",
        out
    );

    // Should show the remove hint
    assert!(
        out.contains("fastskill remove"),
        "Should mention remove command: {}",
        out
    );
}

// --------------------------------------------------------------------------
// duplicates: JSON output
// --------------------------------------------------------------------------

#[tokio::test]
async fn test_duplicates_json_output() {
    let temp = create_duplicates_fixture().await;
    let result = run_in(&["analyze", "duplicates", "--json"], temp.path());

    assert!(
        result.success,
        "duplicates --json should succeed; stderr={}",
        result.stderr
    );

    let json: serde_json::Value = parse_json(&result.stdout);

    // Top-level fields
    assert!(
        json.get("threshold").is_some(),
        "JSON should have threshold"
    );
    assert!(
        json.get("effective_floor").is_some(),
        "JSON should have effective_floor"
    );
    assert!(
        json.get("total_pairs").is_some(),
        "JSON should have total_pairs"
    );

    let pairs = json["pairs"].as_array().expect("pairs should be an array");
    assert!(!pairs.is_empty(), "Should find at least one pair");

    // Each pair should have required fields
    for pair in pairs {
        assert!(pair.get("severity").is_some(), "Pair should have severity");
        assert!(
            pair.get("similarity").is_some(),
            "Pair should have similarity"
        );
        assert!(pair.get("skill_a").is_some(), "Pair should have skill_a");
        assert!(pair.get("skill_b").is_some(), "Pair should have skill_b");
        assert!(
            pair.get("suggestion").is_some(),
            "Pair should have suggestion"
        );

        let skill_a = &pair["skill_a"];
        assert!(skill_a.get("id").is_some(), "skill_a should have id");
        assert!(skill_a.get("name").is_some(), "skill_a should have name");
    }

    // Should have all three severity levels
    let severities: Vec<&str> = pairs
        .iter()
        .filter_map(|p| p["severity"].as_str())
        .collect();
    assert!(
        severities.contains(&"critical"),
        "Should have critical: {:?}",
        severities
    );
    assert!(
        severities.contains(&"high"),
        "Should have high: {:?}",
        severities
    );
    assert!(
        severities.contains(&"medium"),
        "Should have medium: {:?}",
        severities
    );

    // Default effective_floor should be ~0.88
    let effective_floor = json["effective_floor"].as_f64().unwrap();
    assert!(
        (effective_floor - 0.88).abs() < 0.01,
        "Default effective_floor should be ~0.88, got {}",
        effective_floor
    );
}

// --------------------------------------------------------------------------
// duplicates: --severity critical → no results (no-critical fixture)
// --------------------------------------------------------------------------

#[tokio::test]
async fn test_duplicates_severity_critical_no_results() {
    let temp = create_duplicates_no_critical_fixture().await;

    let result = run_in(
        &["analyze", "duplicates", "--severity", "critical"],
        temp.path(),
    );

    assert!(
        result.success,
        "duplicates --severity critical should succeed; stderr={}",
        result.stderr
    );

    let out = &result.stdout;
    assert!(
        out.contains("No duplicate pairs found"),
        "Should show 'No duplicate pairs found': {}",
        out
    );
}

// --------------------------------------------------------------------------
// duplicates: --threshold 0.80 --severity high → effective floor is 0.93
// --------------------------------------------------------------------------

#[tokio::test]
async fn test_duplicates_threshold_080_severity_high() {
    let temp = create_duplicates_fixture().await;

    // effective_floor = max(0.80, 0.93) = 0.93
    // → medium pairs (0.88–0.92) should NOT appear
    // → high and critical pairs (≥0.93) SHOULD appear
    let result = run_in(
        &[
            "analyze",
            "duplicates",
            "--threshold",
            "0.80",
            "--severity",
            "high",
        ],
        temp.path(),
    );

    assert!(
        result.success,
        "duplicates --threshold 0.80 --severity high should succeed; stderr={}",
        result.stderr
    );

    let out = &result.stdout;

    // [MEDIUM] pairs should NOT appear
    assert!(
        !out.contains("[MEDIUM]"),
        "Should not show MEDIUM with effective floor 0.93: {}",
        out
    );

    // [CRITICAL] and/or [HIGH] should appear
    assert!(
        out.contains("[CRITICAL]") || out.contains("[HIGH]"),
        "Should show CRITICAL or HIGH with effective floor 0.93: {}",
        out
    );
}

// --------------------------------------------------------------------------
// duplicates: JSON effective_floor with --threshold 0.80 --severity high
// --------------------------------------------------------------------------

#[tokio::test]
async fn test_duplicates_json_effective_floor() {
    let temp = create_duplicates_fixture().await;

    let result = run_in(
        &[
            "analyze",
            "duplicates",
            "--threshold",
            "0.80",
            "--severity",
            "high",
            "--json",
        ],
        temp.path(),
    );

    assert!(
        result.success,
        "JSON effective floor test should succeed; stderr={}",
        result.stderr
    );

    let json: serde_json::Value = parse_json(&result.stdout);

    // effective_floor should be 0.93 (not 0.80)
    let effective_floor = json["effective_floor"].as_f64().unwrap();
    assert!(
        (effective_floor - 0.93).abs() < 0.01,
        "effective_floor should be 0.93 (not 0.80), got {}",
        effective_floor
    );

    // threshold should still be 0.80
    let threshold = json["threshold"].as_f64().unwrap();
    assert!(
        (threshold - 0.80).abs() < 0.01,
        "threshold should be 0.80, got {}",
        threshold
    );

    // All returned pairs should have similarity >= 0.93
    let pairs = json["pairs"].as_array().unwrap();
    for pair in pairs {
        let severity = pair["severity"].as_str().unwrap();
        assert_ne!(
            severity, "medium",
            "No medium pairs with effective floor 0.93"
        );
        let similarity = pair["similarity"].as_f64().unwrap();
        assert!(
            similarity >= 0.93,
            "All pairs should have similarity >= 0.93, got {}",
            similarity
        );
    }
}

// --------------------------------------------------------------------------
// duplicates: pairs sorted by similarity descending
// --------------------------------------------------------------------------

#[tokio::test]
async fn test_duplicates_sorted_by_similarity() {
    let temp = create_duplicates_fixture().await;
    let result = run_in(&["analyze", "duplicates", "--json"], temp.path());

    assert!(result.success, "Should succeed; stderr={}", result.stderr);

    let json: serde_json::Value = parse_json(&result.stdout);
    let pairs = json["pairs"].as_array().unwrap();

    // Verify sorted by similarity descending
    let mut prev_sim = 2.0_f64;
    for pair in pairs {
        let sim = pair["similarity"].as_f64().unwrap();
        assert!(
            sim <= prev_sim + 1e-6,
            "Pairs should be sorted by similarity desc: {} > {}",
            sim,
            prev_sim
        );
        prev_sim = sim;
    }

    // First pair should be critical (highest similarity)
    if let Some(first_pair) = pairs.first() {
        assert_eq!(
            first_pair["severity"].as_str().unwrap(),
            "critical",
            "First pair should be critical (highest sim)"
        );
    }
}

// --------------------------------------------------------------------------
// snapshot coverage: required cases from the implementation spec
// --------------------------------------------------------------------------

#[tokio::test]
async fn snapshot_cluster_k2_text() {
    let temp = create_cluster_fixture().await;
    let result = run_in(&["analyze", "cluster", "-k", "2"], temp.path());
    assert!(
        result.success,
        "cluster -k 2 should succeed: {}",
        result.stderr
    );
    assert_cli_snapshot("analyze_cluster_k2_text", &result.stdout);
}

#[tokio::test]
async fn snapshot_cluster_k2_json() {
    let temp = create_cluster_fixture().await;
    let result = run_in(&["analyze", "cluster", "-k", "2", "--json"], temp.path());
    assert!(
        result.success,
        "cluster -k 2 --json should succeed: {}",
        result.stderr
    );
    assert_cli_snapshot("analyze_cluster_k2_json", &result.stdout);
}

#[tokio::test]
async fn snapshot_cluster_k10_reduces_to_n() {
    let temp = create_cluster_fixture().await;
    let result = run_in(&["analyze", "cluster", "-k", "10"], temp.path());
    assert!(
        result.success,
        "cluster -k 10 should succeed after reducing k: {}",
        result.stderr
    );
    assert_cli_snapshot("analyze_cluster_k10_reduce_stdout", &result.stdout);
    assert_cli_snapshot_stderr("analyze_cluster_k10_reduce_stderr", &result.stderr);
}

#[tokio::test]
async fn snapshot_cluster_k2_min_size_4() {
    let temp = create_cluster_fixture().await;
    let result = run_in(
        &["analyze", "cluster", "-k", "2", "--min-size", "4"],
        temp.path(),
    );
    assert!(
        result.success,
        "cluster -k 2 --min-size 4 should succeed: {}",
        result.stderr
    );
    assert_cli_snapshot("analyze_cluster_k2_min_size_4", &result.stdout);
}

#[tokio::test]
async fn snapshot_duplicates_default_text() {
    let temp = create_duplicates_fixture().await;
    let result = run_in(&["analyze", "duplicates"], temp.path());
    assert!(
        result.success,
        "duplicates default should succeed: {}",
        result.stderr
    );
    assert_cli_snapshot("analyze_duplicates_default_text", &result.stdout);
}

#[tokio::test]
async fn snapshot_duplicates_default_json() {
    let temp = create_duplicates_fixture().await;
    let result = run_in(&["analyze", "duplicates", "--json"], temp.path());
    assert!(
        result.success,
        "duplicates --json should succeed: {}",
        result.stderr
    );
    assert_cli_snapshot("analyze_duplicates_default_json", &result.stdout);
}

#[tokio::test]
async fn snapshot_duplicates_severity_critical_no_results() {
    let temp = create_duplicates_no_critical_fixture().await;
    let result = run_in(
        &["analyze", "duplicates", "--severity", "critical"],
        temp.path(),
    );
    assert!(
        result.success,
        "duplicates --severity critical should succeed: {}",
        result.stderr
    );
    assert_cli_snapshot("analyze_duplicates_critical_no_results", &result.stdout);
}

#[tokio::test]
async fn snapshot_duplicates_threshold_080_severity_high() {
    let temp = create_duplicates_fixture().await;
    let result = run_in(
        &[
            "analyze",
            "duplicates",
            "--threshold",
            "0.80",
            "--severity",
            "high",
        ],
        temp.path(),
    );
    assert!(
        result.success,
        "duplicates --threshold 0.80 --severity high should succeed: {}",
        result.stderr
    );
    assert_cli_snapshot(
        "analyze_duplicates_threshold_080_severity_high",
        &result.stdout,
    );
}
