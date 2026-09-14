#![allow(clippy::unwrap_used)]
use super::*;
use crate::core::service::SkillId;

fn formats() -> [OutputFormat; 4] {
    [
        OutputFormat::Table,
        OutputFormat::Grid,
        OutputFormat::Json,
        OutputFormat::Xml,
    ]
}

#[test]
fn formats_roundtrip_and_reject_unknown_values() {
    for format in formats() {
        assert_eq!(
            format
                .to_string()
                .to_uppercase()
                .parse::<OutputFormat>()
                .unwrap(),
            format
        );
    }
    assert!("yaml"
        .parse::<OutputFormat>()
        .unwrap_err()
        .contains("Supported formats"));
    assert_eq!(escape_xml("<&>\"'"), "&lt;&amp;&gt;&quot;&apos;");
}

#[test]
fn search_formats_cover_empty_sparse_and_complete_results() {
    let sparse = SearchResultItem {
        id: "sample".into(),
        name: "Sample".into(),
        description: None,
        source: "local".into(),
        similarity: None,
        path: None,
        repository: None,
        version: None,
        install_command: None,
    };
    let complete = SearchResultItem {
        description: Some("A".repeat(60)),
        similarity: Some(0.875),
        path: Some("a&b".into()),
        repository: Some("repo<name>".into()),
        ..sparse.clone()
    };
    for format in formats() {
        let empty = format_search_results(&[], format.clone(), "query").unwrap();
        let single =
            format_search_results(std::slice::from_ref(&sparse), format.clone(), "query").unwrap();
        let multiple =
            format_search_results(&[sparse.clone(), complete.clone()], format.clone(), "query")
                .unwrap();
        assert!(single.contains("Sample"));
        assert!(multiple.contains("Sample"));
        match format {
            OutputFormat::Table => {
                assert!(empty.contains("No skills"));
                assert!(single.contains("No description"));
                assert!(!single.contains("Similarity"));
                assert!(multiple.contains("Similarity"));
                assert!(multiple.contains(&format!("{}...", "A".repeat(47))));
            }
            OutputFormat::Grid => {
                assert!(empty.is_empty());
                assert!(multiple.contains("[0.875]"));
            }
            OutputFormat::Json => {
                let parsed: serde_json::Value = serde_json::from_str(&multiple).unwrap();
                assert_eq!(parsed.as_array().unwrap().len(), 2);
                assert_eq!(parsed[1]["similarity"], 0.875);
            }
            OutputFormat::Xml => {
                assert!(multiple.contains("a&amp;b"));
                assert!(multiple.contains("repo&lt;name&gt;"));
            }
        }
    }
}

#[test]
fn list_formats_preserve_presence_flags_and_optional_metadata() {
    let sparse = ListRow {
        id: "sample".into(),
        name: "Sample".into(),
        description: "description & more".into(),
        version: None,
        in_manifest: false,
        in_lock: false,
        installed: false,
        source_path: None,
        source_type: None,
        missing_from_folder: false,
        missing_from_lock: false,
        missing_from_manifest: false,
    };
    let complete = ListRow {
        version: Some("1.2.3".into()),
        in_manifest: true,
        in_lock: true,
        installed: true,
        source_path: Some("source&path".into()),
        source_type: Some("local".into()),
        missing_from_folder: true,
        missing_from_lock: true,
        missing_from_manifest: true,
        ..sparse.clone()
    };
    assert_eq!(build_list_flags_str(&sparse), "-");
    assert_eq!(
        build_list_flags_str(&complete),
        "missing from folder; missing from lock; missing from manifest"
    );
    for format in formats() {
        for details in [false, true] {
            let empty = format_list_results(&[], format.clone(), details).unwrap();
            assert!(!empty.is_empty());
            let output =
                format_list_results(&[sparse.clone(), complete.clone()], format.clone(), details)
                    .unwrap();
            assert!(output.contains("Sample"));
            match format {
                OutputFormat::Table => {
                    assert!(output.contains("missing from folder"));
                    assert_eq!(output.contains("Source Path"), details);
                }
                OutputFormat::Grid => {
                    assert!(output.contains("vunknown"));
                    assert!(output.contains("[local]"));
                }
                OutputFormat::Json => {
                    let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
                    assert_eq!(parsed[0]["installed"], false);
                    assert_eq!(parsed[1]["installed"], true);
                }
                OutputFormat::Xml => {
                    assert!(output.contains("source&amp;path"));
                    assert!(output.contains("<flags>"));
                }
            }
        }
    }
}

#[test]
fn show_formats_preserve_all_origin_types() {
    let origins = [
        Origin::Git {
            url: "https://example.com/r.git".into(),
            r#ref: Default::default(),
            subdir: None,
        },
        Origin::Local {
            path: "local/path".into(),
            editable: true,
        },
        Origin::ZipUrl {
            url: "https://example.com/a.zip".into(),
        },
        Origin::Repository {
            repo: "repo".into(),
            skill: "sample".into(),
            version: None,
        },
    ];
    let skills: Vec<_> = origins
        .into_iter()
        .map(|origin| {
            SkillDefinition::new(
                SkillId::new("sample".into()).unwrap(),
                "Sample".into(),
                "desc & more".into(),
                "1.2.3".into(),
                origin,
            )
        })
        .collect();
    for format in formats() {
        assert!(!format_show_results(&[], format.clone()).unwrap().is_empty());
        let output = format_show_results(&skills, format.clone()).unwrap();
        assert!(output.contains("Sample"));
        assert!(output.contains("1.2.3"));
        match format {
            OutputFormat::Table | OutputFormat::Xml => {
                for expected in ["git", "local", "zip-url", "repository", "repo/sample"] {
                    assert!(output.contains(expected), "missing {expected}: {output}");
                }
            }
            OutputFormat::Json => {
                let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
                assert_eq!(parsed.as_array().unwrap().len(), 4);
            }
            OutputFormat::Grid => assert!(output.contains("Installed Skills (4)")),
        }
    }
}
