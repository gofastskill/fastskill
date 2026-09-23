//! Find configuration keys that parsed without error and then had no effect.
//!
//! serde skips unknown keys by default, and `deny_unknown_fields` cannot be
//! used on types with `#[serde(flatten)]` or on untagged and internally tagged
//! enums, which is where most of our configuration lives. Instead, serialize the
//! parsed value back and report every key the input has that the round trip
//! lost. That also catches keys dropped because an untagged enum picked a
//! variant that has no field for them.

use serde::Serialize;

/// Dotted paths (under `prefix`) of keys in `raw` that `parsed` does not keep.
///
/// Array elements are addressed by index, as in `repositories.0.brnach`. An
/// empty table is not reported: fields such as `manifests = {}` are skipped on
/// serialization when empty and carry no setting either way.
pub fn dropped_keys<T: Serialize>(raw: &toml::Value, parsed: &T, prefix: &str) -> Vec<String> {
    let Ok(round_trip) = toml::Value::try_from(parsed) else {
        return Vec::new();
    };
    let mut dropped = Vec::new();
    collect(raw, &round_trip, prefix, &mut dropped);
    dropped
}

fn collect(raw: &toml::Value, kept: &toml::Value, path: &str, dropped: &mut Vec<String>) {
    match (raw, kept) {
        (toml::Value::Table(raw), toml::Value::Table(kept)) => {
            for (key, value) in raw {
                let child = format!("{path}.{key}");
                match kept.get(key) {
                    Some(kept) => collect(value, kept, &child, dropped),
                    None if value.as_table().is_some_and(|table| table.is_empty()) => {}
                    None => dropped.push(child),
                }
            }
        }
        (toml::Value::Array(raw), toml::Value::Array(kept)) if raw.len() == kept.len() => {
            for (index, (raw, kept)) in raw.iter().zip(kept).enumerate() {
                collect(raw, kept, &format!("{path}.{index}"), dropped);
            }
        }
        _ => {}
    }
}

/// The error text for dropped keys in a file.
pub fn unknown_keys_message(file: &str, keys: &[String]) -> String {
    format!(
        "{file} has settings that FastSkill does not recognise: {}. Check the spelling, \
         or remove them if they are meant for a newer FastSkill.",
        keys.join(", ")
    )
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Serialize, Deserialize)]
    struct Outer {
        #[serde(default)]
        items: Vec<Item>,
        #[serde(default)]
        depth: u32,
    }

    #[derive(Serialize, Deserialize)]
    struct Item {
        name: String,
        #[serde(flatten)]
        connection: Connection,
    }

    #[derive(Serialize, Deserialize)]
    #[serde(untagged)]
    enum Connection {
        Registry { index_url: String },
        Git { url: String },
    }

    fn dropped(text: &str) -> Vec<String> {
        let raw: toml::Value = toml::from_str(text).unwrap();
        let parsed: Outer = toml::from_str(text).unwrap();
        dropped_keys(&raw, &parsed, "root")
    }

    #[test]
    fn known_keys_and_defaults_are_not_reported() {
        assert!(dropped("depth = 2\n[[items]]\nname = \"a\"\nurl = \"u\"\n").is_empty());
        assert!(dropped("").is_empty());
    }

    #[test]
    fn misspelt_top_level_and_flattened_keys_are_reported() {
        assert_eq!(
            dropped("dpeth = 2\n[[items]]\nname = \"a\"\nurl = \"u\"\nbrnach = \"b\"\n"),
            vec!["root.dpeth", "root.items.0.brnach"]
        );
    }

    #[test]
    fn keys_lost_to_an_untagged_variant_are_reported() {
        assert_eq!(
            dropped("[[items]]\nname = \"a\"\nindex_url = \"i\"\nurl = \"u\"\n"),
            vec!["root.items.0.url"]
        );
    }
}
