use fastskill_core::OutputFormat;

/// One reconciled list row. The lifecycle-only fields live at the CLI edge so
/// adding them does not expand the core's legacy renderer contract.
#[derive(Debug, Clone, serde::Serialize)]
pub(super) struct ListRow {
    pub id: String,
    pub name: String,
    pub description: String,
    pub version: Option<String>,
    pub in_manifest: bool,
    pub in_lock: bool,
    pub installed: bool,
    pub source_path: Option<String>,
    pub source_type: Option<String>,
    pub missing_from_folder: bool,
    pub missing_from_lock: bool,
    pub missing_from_manifest: bool,
    pub desired_constraint: Option<String>,
    pub locked_version: Option<String>,
    pub actual_version: Option<String>,
    pub reconciliation: String,
    pub owners: Vec<String>,
    pub groups: Vec<String>,
    pub mutable: bool,
    pub override_active: bool,
    pub extraneous: bool,
}

pub(super) fn format_list_results(
    rows: &[ListRow],
    format: OutputFormat,
    details: bool,
) -> Result<String, String> {
    match format {
        OutputFormat::Table => Ok(format_table(rows, details)),
        OutputFormat::Json => serde_json::to_string_pretty(rows).map_err(|error| error.to_string()),
        OutputFormat::Grid => Ok(format_grid(rows)),
        OutputFormat::Xml => Ok(format_xml(rows)),
    }
}

fn format_table(rows: &[ListRow], details: bool) -> String {
    if rows.is_empty() {
        return "No skills found.".to_string();
    }
    let headers: Vec<&str> = if details {
        vec![
            "ID",
            "Name",
            "Description",
            "Version",
            "Manifest",
            "Lock",
            "Installed",
            "Source Path",
            "Type",
            "Flags",
        ]
    } else {
        vec!["ID", "Name", "Description", "Flags"]
    };
    let values = rows
        .iter()
        .map(|row| {
            if details {
                vec![
                    row.id.clone(),
                    row.name.clone(),
                    row.description.clone(),
                    row.version.clone().unwrap_or_else(|| "-".to_string()),
                    presence(row.in_manifest),
                    presence(row.in_lock),
                    presence(row.installed),
                    row.source_path.clone().unwrap_or_else(|| "-".to_string()),
                    row.source_type.clone().unwrap_or_else(|| "-".to_string()),
                    flags(row),
                ]
            } else {
                vec![
                    row.id.clone(),
                    row.name.clone(),
                    row.description.clone(),
                    flags(row),
                ]
            }
        })
        .collect::<Vec<_>>();
    let widths = headers
        .iter()
        .enumerate()
        .map(|(index, header)| {
            values
                .iter()
                .map(|row| row[index].len())
                .max()
                .unwrap_or(0)
                .max(header.len())
        })
        .collect::<Vec<_>>();
    let render = |cells: &[String]| {
        cells
            .iter()
            .enumerate()
            .map(|(index, value)| format!("{:width$}", value, width = widths[index]))
            .collect::<Vec<_>>()
            .join("  ")
    };
    let header = render(
        &headers
            .iter()
            .map(|value| (*value).to_string())
            .collect::<Vec<_>>(),
    );
    let mut output = format!("\n{header}\n{}\n", "-".repeat(header.len()));
    for row in values {
        output.push_str(&render(&row));
        output.push('\n');
    }
    output.push('\n');
    output
}

fn presence(value: bool) -> String {
    if value { "Y" } else { "-" }.to_string()
}

fn format_grid(rows: &[ListRow]) -> String {
    if rows.is_empty() {
        return "No skills found.".to_string();
    }
    rows.iter()
        .map(|row| {
            let source = if row.source_type.is_some() || row.source_path.is_some() {
                format!(" [{}]", row.source_type.as_deref().unwrap_or("unknown"))
            } else {
                String::new()
            };
            format!(
                "  - {} (v{}){}\n",
                row.name,
                row.version.as_deref().unwrap_or("unknown"),
                source
            )
        })
        .collect()
}

fn format_xml(rows: &[ListRow]) -> String {
    let mut xml = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<skills>\n");
    for row in rows {
        xml.push_str(&format!(
            "  <skill id=\"{}\">\n    <name>{}</name>\n    <description>{}</description>\n",
            fastskill_core::output::escape_xml(&row.id),
            fastskill_core::output::escape_xml(&row.name),
            fastskill_core::output::escape_xml(&row.description)
        ));
        for (tag, value) in [
            ("version", row.version.as_deref()),
            ("source_path", row.source_path.as_deref()),
            ("source_type", row.source_type.as_deref()),
        ] {
            if let Some(value) = value {
                xml.push_str(&format!(
                    "    <{tag}>{}</{tag}>\n",
                    fastskill_core::output::escape_xml(value)
                ));
            }
        }
        xml.push_str(&format!(
            "    <in_manifest>{}</in_manifest>\n    <in_lock>{}</in_lock>\n    <installed>{}</installed>\n",
            row.in_manifest, row.in_lock, row.installed
        ));
        let flags = flags(row);
        if flags != "-" {
            xml.push_str(&format!(
                "    <flags>{}</flags>\n",
                fastskill_core::output::escape_xml(&flags)
            ));
        }
        xml.push_str("  </skill>\n");
    }
    xml.push_str("</skills>\n");
    xml
}

fn flags(row: &ListRow) -> String {
    let mut parts = Vec::new();
    if row.missing_from_folder {
        parts.push("missing from folder");
    }
    if row.missing_from_lock {
        parts.push("missing from lock");
    }
    if row.missing_from_manifest {
        parts.push("missing from manifest");
    }
    let reconciliation_already_shown = matches!(
        row.reconciliation.as_str(),
        "missing-lock" if row.missing_from_lock
    ) || matches!(
        row.reconciliation.as_str(),
        "missing-content" if row.missing_from_folder
    ) || row.reconciliation == "extraneous" && row.extraneous;
    if row.reconciliation != "ok" && !reconciliation_already_shown {
        parts.push(row.reconciliation.as_str());
    }
    if row.mutable {
        parts.push("mutable");
    }
    if row.override_active {
        parts.push("override");
    }
    if row.extraneous {
        parts.push("extraneous");
    }
    if parts.is_empty() {
        "-".to_string()
    } else {
        parts.join("; ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row() -> ListRow {
        ListRow {
            id: "demo<&\"".to_string(),
            name: "Demo".to_string(),
            description: "Description".to_string(),
            version: Some("1.2.3".to_string()),
            in_manifest: true,
            in_lock: true,
            installed: true,
            source_path: Some("skills/demo".to_string()),
            source_type: Some("local".to_string()),
            missing_from_folder: false,
            missing_from_lock: false,
            missing_from_manifest: false,
            desired_constraint: Some("^1".to_string()),
            locked_version: Some("1.2.3".to_string()),
            actual_version: Some("1.2.3".to_string()),
            reconciliation: "ok".to_string(),
            owners: vec!["direct".to_string()],
            groups: vec!["default".to_string()],
            mutable: false,
            override_active: false,
            extraneous: false,
        }
    }

    #[test]
    fn every_format_preserves_reconciliation_facts() {
        let row = row();
        let table =
            format_list_results(std::slice::from_ref(&row), OutputFormat::Table, true).unwrap();
        assert!(table.contains("Manifest") && table.contains("skills/demo"));
        let compact =
            format_list_results(std::slice::from_ref(&row), OutputFormat::Table, false).unwrap();
        assert!(compact.contains("Description") && !compact.contains("Manifest"));
        let grid =
            format_list_results(std::slice::from_ref(&row), OutputFormat::Grid, false).unwrap();
        assert!(grid.contains("Demo (v1.2.3) [local]"));
        let json =
            format_list_results(std::slice::from_ref(&row), OutputFormat::Json, false).unwrap();
        assert!(json.contains("\"desired_constraint\": \"^1\""));
        let xml = format_list_results(&[row], OutputFormat::Xml, false).unwrap();
        assert!(xml.contains("demo&lt;&amp;&quot;") && xml.contains("<version>1.2.3</version>"));
    }

    #[test]
    fn empty_and_incomplete_rows_cover_status_flags_and_fallbacks() {
        for format in [OutputFormat::Table, OutputFormat::Grid] {
            assert_eq!(
                format_list_results(&[], format, false).unwrap(),
                "No skills found."
            );
        }
        let mut row = row();
        row.version = None;
        row.source_type = None;
        row.source_path = Some("unknown".to_string());
        row.in_manifest = false;
        row.in_lock = false;
        row.installed = false;
        row.missing_from_folder = true;
        row.missing_from_lock = true;
        row.missing_from_manifest = true;
        row.reconciliation = "constraint-mismatch".to_string();
        row.mutable = true;
        row.override_active = true;
        row.extraneous = true;
        let table = format_table(std::slice::from_ref(&row), true);
        assert!(table.contains("missing from folder"));
        assert!(table.contains("constraint-mismatch"));
        assert!(table.contains("mutable; override; extraneous"));
        assert!(format_grid(std::slice::from_ref(&row)).contains("vunknown) [unknown]"));
        let xml = format_xml(&[row]);
        assert!(!xml.contains("<version>") && xml.contains("<flags>"));
    }

    #[test]
    fn duplicate_reconciliation_flags_are_not_rendered_twice() {
        for (reconciliation, folder, lock, extraneous) in [
            ("missing-content", true, false, false),
            ("missing-lock", false, true, false),
            ("extraneous", false, false, true),
        ] {
            let mut row = row();
            row.reconciliation = reconciliation.to_string();
            row.missing_from_folder = folder;
            row.missing_from_lock = lock;
            row.extraneous = extraneous;
            let rendered = flags(&row);
            assert_eq!(
                rendered.matches(reconciliation).count(),
                usize::from(extraneous)
            );
        }
    }
}
