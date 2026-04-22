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
