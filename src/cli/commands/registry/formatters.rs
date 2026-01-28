use crate::cli::error::CliResult;
use fastskill::core::repository::RepositoryConfig;
use fastskill::core::repository::RepositoryDefinition;
use fastskill::core::repository::RepositoryType;

pub fn format_repository_list(repos: &[&RepositoryDefinition]) -> String {
    if repos.is_empty() {
        "No repositories configured.".to_string()
    } else {
        let mut output = format!("Configured Repositories ({}):\n", repos.len());
        for repo in repos {
            let repo_type_str = repo_type_to_string(repo.repo_type.clone());
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
    let repo_type_str = repo_type_to_string(repo.repo_type.clone());

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

fn repo_type_to_string(repo_type: RepositoryType) -> &'static str {
    match repo_type {
        RepositoryType::GitMarketplace => "git-marketplace",
        RepositoryType::HttpRegistry => "http-registry",
        RepositoryType::ZipUrl => "zip-url",
        RepositoryType::Local => "local",
    }
}

pub fn format_grid_output(
    summaries: &[fastskill::core::registry_index::SkillSummary],
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
