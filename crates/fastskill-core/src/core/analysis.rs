//! Analysis utilities for skill similarity and duplicate detection

use serde::Serialize;
use std::collections::HashSet;

/// A skill and its most similar skills, used in similarity matrix computation
#[derive(Debug, Serialize)]
pub struct SimilarityPair {
    /// Skill identifier
    pub skill_id: String,
    /// Human-readable skill name
    pub name: String,
    /// List of (skill_id, similarity_score) tuples, sorted by similarity descending
    pub similar_skills: Vec<(String, f32)>,
}

/// Finds potential duplicate skills by reusing the similarity matrix data
///
/// Returns pairs with similarity >= duplicate_threshold, sorted by similarity descending.
/// Uses a HashSet to avoid reporting the same pair twice (A-B and B-A).
pub fn find_potential_duplicates(
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

#[cfg(test)]
mod tests {
    use super::*;

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

/// Severity levels for duplicate / similarity detection.
/// Provides canonical minimum-similarity thresholds, replacing the magic
/// literals (0.93, 0.98, 0.88) previously scattered through analyze.rs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeverityLevel {
    High,
    Medium,
    Low,
}

impl SeverityLevel {
    /// Minimum cosine similarity for this severity level.
    pub fn min_similarity(self) -> f32 {
        match self {
            SeverityLevel::High => 0.93,
            SeverityLevel::Medium => 0.98,
            SeverityLevel::Low => 0.88,
        }
    }
}

pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }

    let (dot, norm_a, norm_b) = a
        .iter()
        .zip(b.iter())
        .fold((0.0f32, 0.0f32, 0.0f32), |(d, na, nb), (x, y)| {
            (d + x * y, na + x * x, nb + y * y)
        });

    let norm_a = norm_a.sqrt();
    let norm_b = norm_b.sqrt();

    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }

    dot / (norm_a * norm_b)
}

/// L2-normalizes a vector in place.
pub fn l2_normalize(v: &mut [f32]) {
    let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 1e-10 {
        for x in v.iter_mut() {
            *x /= norm;
        }
    }
}

/// Runs spherical k-means on pre-normalized embedding vectors.
///
/// Returns a cluster assignment (index 0..k-1) for each embedding.
/// Embeddings must be L2-normalized before calling this function.
pub fn spherical_kmeans(embeddings: &[Vec<f32>], k: usize, max_iters: usize) -> Vec<usize> {
    if embeddings.is_empty() || k == 0 {
        return Vec::new();
    }
    let n = embeddings.len();
    let k = k.min(n);
    let dim = embeddings[0].len();

    let mut centroids: Vec<Vec<f32>> = (0..k)
        .map(|i| {
            let idx = (i * n) / k;
            let mut c = embeddings[idx].clone();
            l2_normalize(&mut c);
            c
        })
        .collect();

    let mut assignments = vec![0usize; n];

    for _ in 0..max_iters {
        let mut changed = false;
        for (i, emb) in embeddings.iter().enumerate() {
            let best = (0..k)
                .max_by(|&a, &b| {
                    let dot_a: f32 = emb.iter().zip(&centroids[a]).map(|(x, y)| x * y).sum();
                    let dot_b: f32 = emb.iter().zip(&centroids[b]).map(|(x, y)| x * y).sum();
                    dot_a
                        .partial_cmp(&dot_b)
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .unwrap_or(0);
            if assignments[i] != best {
                assignments[i] = best;
                changed = true;
            }
        }
        if !changed {
            break;
        }

        let mut new_centroids = vec![vec![0.0f32; dim]; k];
        let mut counts = vec![0usize; k];
        for (i, emb) in embeddings.iter().enumerate() {
            let c = assignments[i];
            for (d, val) in emb.iter().enumerate() {
                new_centroids[c][d] += val;
            }
            counts[c] += 1;
        }
        for c in 0..k {
            if counts[c] == 0 {
                let farthest = embeddings
                    .iter()
                    .enumerate()
                    .max_by(|(_, emb_a), (_, emb_b)| {
                        let dist_a: f32 = centroids[c]
                            .iter()
                            .zip(emb_a.iter())
                            .map(|(x, y)| (x - y).powi(2))
                            .sum();
                        let dist_b: f32 = centroids[c]
                            .iter()
                            .zip(emb_b.iter())
                            .map(|(x, y)| (x - y).powi(2))
                            .sum();
                        dist_a
                            .partial_cmp(&dist_b)
                            .unwrap_or(std::cmp::Ordering::Equal)
                    })
                    .map(|(i, _)| i)
                    .unwrap_or(0);
                new_centroids[c] = embeddings[farthest].clone();
            }
            l2_normalize(&mut new_centroids[c]);
        }
        centroids = new_centroids;
    }

    assignments
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod kmeans_tests {
    use super::*;

    #[test]
    fn test_l2_normalize_unit_vector() {
        let mut v = vec![3.0f32, 4.0f32];
        l2_normalize(&mut v);
        let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!(
            (norm - 1.0f32).abs() < 1e-6,
            "normalized vector must have unit norm"
        );
    }

    #[test]
    fn test_l2_normalize_zero_vector() {
        let mut v = vec![0.0f32, 0.0f32];
        l2_normalize(&mut v);
        assert_eq!(v, vec![0.0f32, 0.0f32]);
    }

    #[test]
    fn test_spherical_kmeans_basic() {
        // Two clearly separated clusters
        let mut e1 = vec![1.0f32, 0.0];
        let mut e2 = vec![1.0f32, 0.0];
        let mut e3 = vec![0.0f32, 1.0];
        let mut e4 = vec![0.0f32, 1.0];
        l2_normalize(&mut e1);
        l2_normalize(&mut e2);
        l2_normalize(&mut e3);
        l2_normalize(&mut e4);

        let embeddings = vec![e1, e2, e3, e4];
        let assignments = spherical_kmeans(&embeddings, 2, 10);
        assert_eq!(assignments.len(), 4);
        // First two should be in the same cluster, last two in another
        assert_eq!(assignments[0], assignments[1]);
        assert_eq!(assignments[2], assignments[3]);
        assert_ne!(assignments[0], assignments[2]);
    }

    #[test]
    fn test_spherical_kmeans_empty() {
        let result = spherical_kmeans(&[], 3, 10);
        assert!(result.is_empty());
    }

    #[test]
    fn test_severity_level_thresholds() {
        assert!((SeverityLevel::High.min_similarity() - 0.93).abs() < 1e-6);
        assert!((SeverityLevel::Medium.min_similarity() - 0.98).abs() < 1e-6);
        assert!((SeverityLevel::Low.min_similarity() - 0.88).abs() < 1e-6);
    }
}
