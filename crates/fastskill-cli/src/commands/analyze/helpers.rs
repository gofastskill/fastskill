//! Shared helpers used by matrix, cluster, and duplicates subcommands.

use fastskill_core::core::vector_index::IndexedSkill;

/// Extracts skill name from metadata JSON
pub(super) fn get_skill_name(metadata: &serde_json::Value) -> String {
    metadata
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("Unknown")
        .to_string()
}

/// Get the file modification time as an ISO 8601 UTC string
pub(super) fn get_file_mtime(path: &std::path::Path) -> Option<String> {
    let metadata = std::fs::metadata(path).ok()?;
    let modified = metadata.modified().ok()?;
    let dt = chrono::DateTime::<chrono::Utc>::from(modified);
    Some(dt.format("%Y-%m-%dT%H:%M:%SZ").to_string())
}

/// Compute a suggestion for a duplicate pair based on file modification times.
///
/// Both timing branches use `SystemTime` values obtained from filesystem metadata so that
/// duration arithmetic is consistent — there is no mix of UNIX_EPOCH and `now()` origins.
pub(super) fn compute_suggestion(skill_a: &IndexedSkill, skill_b: &IndexedSkill) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_skill_name_with_name() {
        let metadata = serde_json::json!({ "name": "Test Skill", "version": "1.0.0" });
        assert_eq!(get_skill_name(&metadata), "Test Skill");
    }

    #[test]
    fn test_get_skill_name_without_name() {
        let metadata = serde_json::json!({ "version": "1.0.0" });
        assert_eq!(get_skill_name(&metadata), "Unknown");
    }
}
