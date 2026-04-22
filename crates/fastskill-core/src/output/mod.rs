//! Output formatting module providing consistent formatting across commands
//!
//! This module provides shared output formatting capabilities that can be used
//! by multiple CLI commands to ensure consistent output styling.

use crate::core::SkillDefinition;
use crate::search::SearchResultItem;
use serde_json;
use std::fmt;

/// One row for the list table: union of all skills with presence and gap flags.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ListRow {
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
}

/// Supported output formats
#[derive(Debug, Clone, PartialEq)]
pub enum OutputFormat {
    Table,
    Json,
    Grid,
    Xml,
}

impl fmt::Display for OutputFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OutputFormat::Table => write!(f, "table"),
            OutputFormat::Json => write!(f, "json"),
            OutputFormat::Grid => write!(f, "grid"),
            OutputFormat::Xml => write!(f, "xml"),
        }
    }
}

impl std::str::FromStr for OutputFormat {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "table" => Ok(OutputFormat::Table),
            "json" => Ok(OutputFormat::Json),
            "grid" => Ok(OutputFormat::Grid),
            "xml" => Ok(OutputFormat::Xml),
            _ => Err(format!(
                "Invalid format '{}'. Supported formats: table, json, grid, xml",
                s
            )),
        }
    }
}

/// Format search results in the specified output format
pub fn format_search_results(
    results: &[SearchResultItem],
    format: OutputFormat,
    query: &str,
) -> Result<String, String> {
    match format {
        OutputFormat::Table => format_search_results_as_table(results, query),
        OutputFormat::Json => format_search_results_as_json(results),
        OutputFormat::Grid => format_search_results_as_grid(results, query),
        OutputFormat::Xml => format_search_results_as_xml(results),
    }
}

/// Format search results as ASCII table
fn format_search_results_as_table(
    results: &[SearchResultItem],
    query: &str,
) -> Result<String, String> {
    if results.is_empty() {
        return Ok(format!("No skills found matching '{}'", query));
    }

    let mut output = String::new();

    output.push_str(&format!(
        "Found {} skills matching '{}':\n\n",
        results.len(),
        query
    ));

    // Determine column widths
    let mut max_id_width = 2; // "ID"
    let mut max_name_width = 4; // "Name"
    let mut max_desc_width = 11; // "Description"
    let mut max_source_width = 6; // "Source"
    let mut max_sim_width = 9; // "Similarity"

    for item in results {
        max_id_width = max_id_width.max(item.id.len());
        max_name_width = max_name_width.max(item.name.len());
        max_desc_width = max_desc_width.max(
            item.description
                .as_deref()
                .unwrap_or("No description")
                .len()
                .min(50),
        );
        max_source_width = max_source_width.max(item.source.len());
        if let Some(sim) = item.similarity {
            let sim_str = format!("{:.3}", sim);
            max_sim_width = max_sim_width.max(sim_str.len());
        }
    }

    // Create table header
    let header = if results.iter().any(|r| r.similarity.is_some()) {
        format!(
            "+-{}-+-{}-+-{}-+-{}-+-{}-+\n| {:<width_id$} | {:<width_name$} | {:<width_desc$} | {:<width_source$} | {:<width_sim$} |\n+-{}-+-{}-+-{}-+-{}-+-{}-+",
            "-".repeat(max_id_width),
            "-".repeat(max_name_width),
            "-".repeat(max_desc_width),
            "-".repeat(max_source_width),
            "-".repeat(max_sim_width),
            "ID",
            "Name",
            "Description",
            "Source",
            "Similarity",
            "-".repeat(max_id_width),
            "-".repeat(max_name_width),
            "-".repeat(max_desc_width),
            "-".repeat(max_source_width),
            "-".repeat(max_sim_width),
            width_id = max_id_width,
            width_name = max_name_width,
            width_desc = max_desc_width,
            width_source = max_source_width,
            width_sim = max_sim_width
        )
    } else {
        format!(
            "+-{}-+-{}-+-{}-+-{}-+\n| {:<width_id$} | {:<width_name$} | {:<width_desc$} | {:<width_source$} |\n+-{}-+-{}-+-{}-+-{}-+",
            "-".repeat(max_id_width),
            "-".repeat(max_name_width),
            "-".repeat(max_desc_width),
            "-".repeat(max_source_width),
            "ID",
            "Name",
            "Description",
            "Source",
            "-".repeat(max_id_width),
            "-".repeat(max_name_width),
            "-".repeat(max_desc_width),
            "-".repeat(max_source_width),
            width_id = max_id_width,
            width_name = max_name_width,
            width_desc = max_desc_width,
            width_source = max_source_width
        )
    };

    output.push_str(&header);
    output.push('\n');

    // Add rows
    for item in results {
        let desc = item.description.as_deref().unwrap_or("No description");
        let desc_str = if desc.len() > 50 {
            format!("{}...", &desc[..47])
        } else {
            desc.to_string()
        };

        let row = if let Some(sim) = item.similarity {
            format!(
                "| {:<width_id$} | {:<width_name$} | {:<width_desc$} | {:<width_source$} | {:<width_sim$} |",
                item.id,
                item.name,
                desc_str,
                item.source,
                format!("{:.3}", sim),
                width_id = max_id_width,
                width_name = max_name_width,
                width_desc = max_desc_width,
                width_source = max_source_width,
                width_sim = max_sim_width
            )
        } else {
            format!(
                "| {:<width_id$} | {:<width_name$} | {:<width_desc$} | {:<width_source$} |",
                item.id,
                item.name,
                desc_str,
                item.source,
                width_id = max_id_width,
                width_name = max_name_width,
                width_desc = max_desc_width,
                width_source = max_source_width
            )
        };
        output.push_str(&row);
        output.push('\n');
    }

    // Add bottom border
    let footer = if results.iter().any(|r| r.similarity.is_some()) {
        format!(
            "+-{}-+-{}-+-{}-+-{}-+-{}-+",
            "-".repeat(max_id_width),
            "-".repeat(max_name_width),
            "-".repeat(max_desc_width),
            "-".repeat(max_source_width),
            "-".repeat(max_sim_width)
        )
    } else {
        format!(
            "+-{}-+-{}-+-{}-+-{}-+",
            "-".repeat(max_id_width),
            "-".repeat(max_name_width),
            "-".repeat(max_desc_width),
            "-".repeat(max_source_width)
        )
    };
    output.push_str(&footer);

    Ok(output)
}

/// Format search results as JSON
fn format_search_results_as_json(results: &[SearchResultItem]) -> Result<String, String> {
    serde_json::to_string_pretty(results).map_err(|e| format!("Failed to serialize to JSON: {}", e))
}

/// Format search results as grid (simple table format)
fn format_search_results_as_grid(
    results: &[SearchResultItem],
    query: &str,
) -> Result<String, String> {
    if results.is_empty() {
        return Ok(format!("No skills found matching '{}'", query));
    }

    let mut output = String::new();
    output.push_str(&format!(
        "Found {} skills matching '{}':\n\n",
        results.len(),
        query
    ));

    for item in results {
        output.push_str(&format!("  - {}", item.name));
        if let Some(desc) = &item.description {
            output.push_str(&format!(": {}", desc));
        }
        output.push_str(&format!(" ({})", item.source));
        if let Some(sim) = item.similarity {
            output.push_str(&format!(" [{:.3}]", sim));
        }
        output.push('\n');
    }

    Ok(output)
}

/// Format search results as XML
fn format_search_results_as_xml(results: &[SearchResultItem]) -> Result<String, String> {
    let mut xml = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<skills>\n");

    for item in results {
        xml.push_str(&format!(
            "  <skill id=\"{}\" source=\"{}\">\n",
            escape_xml(&item.id),
            escape_xml(&item.source)
        ));
        xml.push_str(&format!("    <name>{}</name>\n", escape_xml(&item.name)));

        if let Some(description) = &item.description {
            xml.push_str(&format!(
                "    <description>{}</description>\n",
                escape_xml(description)
            ));
        }

        if let Some(similarity) = item.similarity {
            xml.push_str(&format!("    <similarity>{:.3}</similarity>\n", similarity));
        }

        if let Some(path) = &item.path {
            xml.push_str(&format!("    <path>{}</path>\n", escape_xml(path)));
        }

        if let Some(repository) = &item.repository {
            xml.push_str(&format!(
                "    <repository>{}</repository>\n",
                escape_xml(repository)
            ));
        }

        xml.push_str("  </skill>\n");
    }

    xml.push_str("</skills>\n");
    Ok(xml)
}

/// Escape XML special characters
pub fn escape_xml(input: &str) -> String {
    input
        .replace("&", "&amp;")
        .replace("<", "&lt;")
        .replace(">", "&gt;")
        .replace("\"", "&quot;")
        .replace("'", "&apos;")
}

/// Format list results in the specified output format
pub fn format_list_results(
    rows: &[ListRow],
    format: OutputFormat,
    details: bool,
) -> Result<String, String> {
    match format {
        OutputFormat::Table => format_list_table(rows, details),
        OutputFormat::Json => serde_json::to_string_pretty(rows).map_err(|e| e.to_string()),
        OutputFormat::Grid => format_list_grid(rows, details),
        OutputFormat::Xml => format_list_xml(rows),
    }
}

/// Format list results as table
fn format_list_table(rows: &[ListRow], details: bool) -> Result<String, String> {
    if rows.is_empty() {
        return Ok("No skills found.".to_string());
    }

    let mut output = String::new();
    if details {
        let headers = [
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
        ];
        let mut col_widths = vec![0; headers.len()];
        for (i, h) in headers.iter().enumerate() {
            col_widths[i] = h.len();
        }
        for row in rows {
            col_widths[0] = col_widths[0].max(row.id.len());
            col_widths[1] = col_widths[1].max(row.name.len());
            col_widths[2] = col_widths[2].max(row.description.len());
            col_widths[3] = col_widths[3].max(row.version.as_deref().unwrap_or("-").len());
            col_widths[4] = col_widths[4].max(1);
            col_widths[5] = col_widths[5].max(1);
            col_widths[6] = col_widths[6].max(1);
            col_widths[7] = col_widths[7].max(row.source_path.as_deref().unwrap_or("-").len());
            col_widths[8] = col_widths[8].max(row.source_type.as_deref().unwrap_or("-").len());
            let flags = build_list_flags_str(row);
            col_widths[9] = col_widths[9].max(flags.len());
        }

        let header_row: Vec<String> = headers
            .iter()
            .enumerate()
            .map(|(i, h)| format!("{:width$}", *h, width = col_widths[i]))
            .collect();
        output.push('\n');
        output.push_str(&header_row.join("  "));
        output.push('\n');
        output.push_str(&"-".repeat(header_row.join("  ").len()));
        output.push('\n');

        for row in rows {
            let version = row.version.as_deref().unwrap_or("-");
            let in_manifest = if row.in_manifest { "Y" } else { "-" };
            let in_lock = if row.in_lock { "Y" } else { "-" };
            let installed = if row.installed { "Y" } else { "-" };
            let source_path = row.source_path.as_deref().unwrap_or("-");
            let source_type = row.source_type.as_deref().unwrap_or("-");
            let flags = build_list_flags_str(row);
            let line = [
                format!("{:width$}", row.id, width = col_widths[0]),
                format!("{:width$}", row.name, width = col_widths[1]),
                format!("{:width$}", row.description, width = col_widths[2]),
                format!("{:width$}", version, width = col_widths[3]),
                format!("{:width$}", in_manifest, width = col_widths[4]),
                format!("{:width$}", in_lock, width = col_widths[5]),
                format!("{:width$}", installed, width = col_widths[6]),
                format!("{:width$}", source_path, width = col_widths[7]),
                format!("{:width$}", source_type, width = col_widths[8]),
                format!("{:width$}", flags, width = col_widths[9]),
            ];
            output.push_str(&line.join("  "));
            output.push('\n');
        }
    } else {
        let headers = ["ID", "Name", "Description", "Flags"];
        let mut col_widths = vec![0; headers.len()];
        for (i, h) in headers.iter().enumerate() {
            col_widths[i] = h.len();
        }
        for row in rows {
            col_widths[0] = col_widths[0].max(row.id.len());
            col_widths[1] = col_widths[1].max(row.name.len());
            col_widths[2] = col_widths[2].max(row.description.len());
            let flags = build_list_flags_str(row);
            col_widths[3] = col_widths[3].max(flags.len());
        }

        let header_row: Vec<String> = headers
            .iter()
            .enumerate()
            .map(|(i, h)| format!("{:width$}", *h, width = col_widths[i]))
            .collect();
        output.push('\n');
        output.push_str(&header_row.join("  "));
        output.push('\n');
        output.push_str(&"-".repeat(header_row.join("  ").len()));
        output.push('\n');

        for row in rows {
            let flags = build_list_flags_str(row);
            let line = [
                format!("{:width$}", row.id, width = col_widths[0]),
                format!("{:width$}", row.name, width = col_widths[1]),
                format!("{:width$}", row.description, width = col_widths[2]),
                format!("{:width$}", flags, width = col_widths[3]),
            ];
            output.push_str(&line.join("  "));
            output.push('\n');
        }
    }

    output.push('\n');
    Ok(output)
}

/// Format list results as grid (simple list format)
fn format_list_grid(rows: &[ListRow], _details: bool) -> Result<String, String> {
    if rows.is_empty() {
        return Ok("No skills found.".to_string());
    }

    let mut output = String::new();
    for row in rows {
        output.push_str(&format!(
            "  - {} (v{})",
            row.name,
            row.version.as_deref().unwrap_or("unknown")
        ));
        if row.source_type.is_some() || row.source_path.is_some() {
            output.push_str(&format!(
                " [{}]",
                row.source_type.as_deref().unwrap_or("unknown")
            ));
        }
        output.push('\n');
    }

    Ok(output)
}

/// Format list results as XML
fn format_list_xml(rows: &[ListRow]) -> Result<String, String> {
    let mut xml = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<skills>\n");

    for row in rows {
        xml.push_str(&format!("  <skill id=\"{}\">\n", escape_xml(&row.id)));
        xml.push_str(&format!("    <name>{}</name>\n", escape_xml(&row.name)));
        xml.push_str(&format!(
            "    <description>{}</description>\n",
            escape_xml(&row.description)
        ));
        if let Some(version) = &row.version {
            xml.push_str(&format!("    <version>{}</version>\n", escape_xml(version)));
        }
        xml.push_str(&format!(
            "    <in_manifest>{}</in_manifest>\n",
            row.in_manifest
        ));
        xml.push_str(&format!("    <in_lock>{}</in_lock>\n", row.in_lock));
        xml.push_str(&format!("    <installed>{}</installed>\n", row.installed));
        if let Some(source_path) = &row.source_path {
            xml.push_str(&format!(
                "    <source_path>{}</source_path>\n",
                escape_xml(source_path)
            ));
        }
        if let Some(source_type) = &row.source_type {
            xml.push_str(&format!(
                "    <source_type>{}</source_type>\n",
                escape_xml(source_type)
            ));
        }
        let flags = build_list_flags_str(row);
        if flags != "-" {
            xml.push_str(&format!("    <flags>{}</flags>\n", escape_xml(&flags)));
        }
        xml.push_str("  </skill>\n");
    }

    xml.push_str("</skills>\n");
    Ok(xml)
}

/// Format show results in the specified output format
pub fn format_show_results(
    skills: &[SkillDefinition],
    format: OutputFormat,
) -> Result<String, String> {
    match format {
        OutputFormat::Table => format_show_table(skills),
        OutputFormat::Json => serde_json::to_string_pretty(skills).map_err(|e| e.to_string()),
        OutputFormat::Grid => format_show_grid(skills),
        OutputFormat::Xml => format_show_xml(skills),
    }
}

/// Format show results as table
fn format_show_table(skills: &[SkillDefinition]) -> Result<String, String> {
    if skills.is_empty() {
        return Ok("No skills found.".to_string());
    }

    let mut output = String::new();
    for skill in skills {
        output.push_str(&format!("Skill: {}\n", skill.name));
        output.push_str(&format!("  ID: {}\n", skill.id));
        output.push_str(&format!("  Version: {}\n", skill.version));
        output.push_str(&format!("  Description: {}\n", skill.description));
        if let Some(source_type) = &skill.source_type {
            output.push_str(&format!("  Source Type: {:?}\n", source_type));
        }
        if let Some(source_url) = &skill.source_url {
            output.push_str(&format!("  Source URL: {}\n", source_url));
        }
        output.push('\n');
    }

    Ok(output)
}

/// Format show results as grid (simple list format)
fn format_show_grid(skills: &[SkillDefinition]) -> Result<String, String> {
    if skills.is_empty() {
        return Ok("No skills found.".to_string());
    }

    let mut output = String::new();
    output.push_str(&format!("Installed Skills ({}):\n", skills.len()));
    for skill in skills {
        output.push_str(&format!("  • {} (v{})\n", skill.name, skill.version));
    }

    Ok(output)
}

/// Format show results as XML
fn format_show_xml(skills: &[SkillDefinition]) -> Result<String, String> {
    let mut xml = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<skills>\n");

    for skill in skills {
        xml.push_str(&format!(
            "  <skill id=\"{}\">\n",
            escape_xml(skill.id.as_ref())
        ));
        xml.push_str(&format!("    <name>{}</name>\n", escape_xml(&skill.name)));
        xml.push_str(&format!(
            "    <version>{}</version>\n",
            escape_xml(&skill.version)
        ));
        xml.push_str(&format!(
            "    <description>{}</description>\n",
            escape_xml(&skill.description)
        ));
        if let Some(source_type) = &skill.source_type {
            xml.push_str(&format!(
                "    <source_type>{:?}</source_type>\n",
                source_type
            ));
        }
        if let Some(source_url) = &skill.source_url {
            xml.push_str(&format!(
                "    <source_url>{}</source_url>\n",
                escape_xml(source_url)
            ));
        }
        xml.push_str("  </skill>\n");
    }

    xml.push_str("</skills>\n");
    Ok(xml)
}

fn build_list_flags_str(row: &ListRow) -> String {
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
    if parts.is_empty() {
        "-".to_string()
    } else {
        parts.join("; ")
    }
}
