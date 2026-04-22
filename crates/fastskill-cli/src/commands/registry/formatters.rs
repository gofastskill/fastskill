use crate::error::CliResult;
use fastskill_core::core::repository::RepositoryConfig;
use fastskill_core::core::repository::RepositoryDefinition;
use fastskill_core::core::repository::RepositoryType;

pub fn format_repository_list(repos: &[&RepositoryDefinition]) -> String {
    if repos.is_empty() {
        "No repositories configured.".to_string()
    } else {
        let mut output = format!("Configured Repositories ({}):\n", repos.len());
        for repo in repos {
            let repo_type_str = repo_type_to_string(&repo.repo_type);
            output.push_str(&format!(
                "  • {} (type: {}, priority: {})\n",
                repo.name, repo_type_str, repo.priority
            ));
        }
        output
    }
}

pub fn format_repository_details(repo: &RepositoryDefinition) -> String {
    let mut output = String::new();
    let repo_type_str = repo_type_to_string(&repo.repo_type);

    output.push_str(&format!("Repository: {}\n", repo.name));
    output.push_str(&format!("  Type: {}\n", repo_type_str));
    output.push_str(&format!("  Priority: {}\n", repo.priority));

    match &repo.config {
        RepositoryConfig::GitMarketplace { url, branch, tag } => {
            output.push_str(&format!("  URL: {}\n", url));
            if let Some(b) = branch {
                output.push_str(&format!("  Branch: {}\n", b));
            }
            if let Some(t) = tag {
                output.push_str(&format!("  Tag: {}\n", t));
            }
        }
        RepositoryConfig::HttpRegistry { index_url } => {
            output.push_str(&format!("  Index URL: {}\n", index_url));
        }
        RepositoryConfig::ZipUrl { base_url } => {
            output.push_str(&format!("  Base URL: {}\n", base_url));
        }
        RepositoryConfig::Local { path } => {
            output.push_str(&format!("  Path: {}\n", path.display()));
        }
    }

    if let Some(auth) = &repo.auth {
        output.push_str(&format!("  Auth: {:?}\n", auth));
    }

    if let Some(storage) = &repo.storage {
        output.push_str(&format!("  Storage: {:?}\n", storage));
    }

    output
}

pub(crate) fn repo_type_to_string(repo_type: &RepositoryType) -> &'static str {
    match repo_type {
        RepositoryType::GitMarketplace => "git-marketplace",
        RepositoryType::HttpRegistry => "http-registry",
        RepositoryType::ZipUrl => "zip-url",
        RepositoryType::Local => "local",
    }
}

pub fn format_table_output(
    summaries: &[fastskill_core::core::registry_index::SkillSummary],
    all_versions: bool,
) -> CliResult<()> {
    let headers = if all_versions {
        vec!["Scope", "Name", "Description", "Version", "Published"]
    } else {
        vec![
            "Scope",
            "Name",
            "Description",
            "Latest Version",
            "Published",
        ]
    };

    let mut col_widths = vec![0; headers.len()];
    for (i, header) in headers.iter().enumerate() {
        col_widths[i] = header.len();
    }

    for summary in summaries {
        col_widths[0] = col_widths[0].max(summary.scope.len());
        col_widths[1] = col_widths[1].max(summary.name.len());
        let desc_len = summary.description.len().min(50);
        col_widths[2] = col_widths[2].max(desc_len);
        col_widths[3] = col_widths[3].max(summary.latest_version.len());
        col_widths[4] = col_widths[4].max(10);
    }

    let header_row: Vec<String> = headers
        .iter()
        .enumerate()
        .map(|(i, h)| format!("{:width$}", h, width = col_widths[i]))
        .collect();
    println!("\n{}", header_row.join("  "));
    println!("{}", "-".repeat(header_row.join("  ").len()));

    for summary in summaries {
        let description = if summary.description.len() > 50 {
            format!("{}...", &summary.description[..47])
        } else {
            summary.description.clone()
        };

        let published = summary
            .published_at
            .map(|dt| dt.format("%Y-%m-%d").to_string())
            .unwrap_or_else(|| "N/A".to_string());

        let row = [
            format!("{:width$}", summary.scope, width = col_widths[0]),
            format!("{:width$}", summary.name, width = col_widths[1]),
            format!("{:width$}", description, width = col_widths[2]),
            format!("{:width$}", summary.latest_version, width = col_widths[3]),
            format!("{:width$}", published, width = col_widths[4]),
        ];
        println!("{}", row.join("  "));
    }

    println!();
    Ok(())
}

pub fn format_grid_output(
    summaries: &[fastskill_core::core::registry_index::SkillSummary],
    _all_versions: bool,
) -> CliResult<()> {
    if summaries.is_empty() {
        println!("No skills found.");
        return Ok(());
    }

    for summary in summaries {
        println!(
            "- {}/{} ({})",
            summary.scope, summary.name, summary.latest_version
        );
    }
    Ok(())
}

pub fn format_xml_output(
    summaries: &[fastskill_core::core::registry_index::SkillSummary],
) -> CliResult<()> {
    println!("{}", summaries_to_xml(summaries));
    Ok(())
}

pub fn format_repository_list_grid(repos: &[&RepositoryDefinition]) -> String {
    if repos.is_empty() {
        return "No repositories configured.".to_string();
    }

    let mut output = String::new();
    for repo in repos {
        let repo_type_str = repo_type_to_string(&repo.repo_type);
        output.push_str(&format!(
            "- {} [{}] priority={}\n",
            repo.name, repo_type_str, repo.priority
        ));
    }
    output
}

pub fn format_repository_details_grid(repo: &RepositoryDefinition) -> String {
    let mut output = String::new();
    output.push_str(&format!("name={}\n", repo.name));
    output.push_str(&format!("type={}\n", repo_type_to_string(&repo.repo_type)));
    output.push_str(&format!("priority={}\n", repo.priority));

    match &repo.config {
        RepositoryConfig::GitMarketplace { url, branch, tag } => {
            output.push_str(&format!("url={}\n", url));
            if let Some(b) = branch {
                output.push_str(&format!("branch={}\n", b));
            }
            if let Some(t) = tag {
                output.push_str(&format!("tag={}\n", t));
            }
        }
        RepositoryConfig::HttpRegistry { index_url } => {
            output.push_str(&format!("index_url={}\n", index_url));
        }
        RepositoryConfig::ZipUrl { base_url } => {
            output.push_str(&format!("base_url={}\n", base_url));
        }
        RepositoryConfig::Local { path } => {
            output.push_str(&format!("path={}\n", path.display()));
        }
    }

    output
}

pub fn format_repository_list_xml(repos: &[&RepositoryDefinition]) -> String {
    let mut xml = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<repositories>\n");
    for repo in repos {
        xml.push_str(&format!(
            "  <repository name=\"{}\" type=\"{}\" priority=\"{}\" />\n",
            escape_xml(&repo.name),
            repo_type_to_string(&repo.repo_type),
            repo.priority
        ));
    }
    xml.push_str("</repositories>\n");
    xml
}

pub fn format_repository_details_xml(repo: &RepositoryDefinition) -> String {
    let mut xml = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<repository>\n");
    xml.push_str(&format!("  <name>{}</name>\n", escape_xml(&repo.name)));
    xml.push_str(&format!(
        "  <type>{}</type>\n",
        repo_type_to_string(&repo.repo_type)
    ));
    xml.push_str(&format!("  <priority>{}</priority>\n", repo.priority));

    match &repo.config {
        RepositoryConfig::GitMarketplace { url, branch, tag } => {
            xml.push_str("  <config kind=\"git-marketplace\">\n");
            xml.push_str(&format!("    <url>{}</url>\n", escape_xml(url)));
            if let Some(b) = branch {
                xml.push_str(&format!("    <branch>{}</branch>\n", escape_xml(b)));
            }
            if let Some(t) = tag {
                xml.push_str(&format!("    <tag>{}</tag>\n", escape_xml(t)));
            }
            xml.push_str("  </config>\n");
        }
        RepositoryConfig::HttpRegistry { index_url } => {
            xml.push_str("  <config kind=\"http-registry\">\n");
            xml.push_str(&format!(
                "    <index_url>{}</index_url>\n",
                escape_xml(index_url)
            ));
            xml.push_str("  </config>\n");
        }
        RepositoryConfig::ZipUrl { base_url } => {
            xml.push_str("  <config kind=\"zip-url\">\n");
            xml.push_str(&format!(
                "    <base_url>{}</base_url>\n",
                escape_xml(base_url)
            ));
            xml.push_str("  </config>\n");
        }
        RepositoryConfig::Local { path } => {
            xml.push_str("  <config kind=\"local\">\n");
            xml.push_str(&format!(
                "    <path>{}</path>\n",
                escape_xml(&path.display().to_string())
            ));
            xml.push_str("  </config>\n");
        }
    }

    xml.push_str("</repository>\n");
    xml
}

fn summaries_to_xml(summaries: &[fastskill_core::core::registry_index::SkillSummary]) -> String {
    let mut xml = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<skills>\n");
    for summary in summaries {
        xml.push_str(&format!(
            "  <skill scope=\"{}\" name=\"{}\">\n",
            escape_xml(&summary.scope),
            escape_xml(&summary.name)
        ));
        xml.push_str(&format!(
            "    <description>{}</description>\n",
            escape_xml(&summary.description)
        ));
        xml.push_str(&format!(
            "    <latest_version>{}</latest_version>\n",
            escape_xml(&summary.latest_version)
        ));
        if let Some(published_at) = summary.published_at {
            xml.push_str(&format!(
                "    <published>{}</published>\n",
                published_at.format("%Y-%m-%d")
            ));
        }
        xml.push_str("  </skill>\n");
    }
    xml.push_str("</skills>\n");
    xml
}

fn escape_xml(input: &str) -> String {
    input
        .replace("&", "&amp;")
        .replace("<", "&lt;")
        .replace(">", "&gt;")
        .replace("\"", "&quot;")
        .replace("'", "&apos;")
}
