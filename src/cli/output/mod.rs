//! Output formatting module providing consistent formatting across commands
//!
//! This module provides shared output formatting capabilities that can be used
//! by multiple CLI commands to ensure consistent output styling.

use crate::cli::search::SearchResultItem;
use serde_json;
use std::fmt;

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
fn escape_xml(input: &str) -> String {
    input
        .replace("&", "&amp;")
        .replace("<", "&lt;")
        .replace(">", "&gt;")
        .replace("\"", "&quot;")
        .replace("'", "&apos;")
}
