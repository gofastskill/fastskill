use crate::cli::error::{CliError, CliResult};
use crate::cli::utils::messages;
use fastskill::core::manifest::MetadataSection;
use fastskill::core::metadata::parse_yaml_frontmatter;
use fastskill::core::sources::{
    ClaudeCodeMarketplaceJson, ClaudeCodeMetadata, ClaudeCodeOwner, ClaudeCodePlugin,
    MarketplaceSkill,
};
use std::fs;
use std::path::{Path, PathBuf};
use toml;
use tracing::{info, warn};
use walkdir::WalkDir;

pub async fn execute_create(
    path: PathBuf,
    output: Option<PathBuf>,
    _base_url: Option<String>,
    name: Option<String>,
    owner_name: Option<String>,
    owner_email: Option<String>,
    description: Option<String>,
    version: Option<String>,
) -> CliResult<()> {
    let skill_dir = path
        .canonicalize()
        .map_err(|e| CliError::Validation(format!("Failed to resolve path: {}", e)))?;

    info!("Scanning directory for skills: {}", skill_dir.display());

    let skills = scan_directory_for_skills(&skill_dir)?;

    if skills.is_empty() {
        return Err(CliError::Validation(format!(
            "No skills found in directory: {}",
            skill_dir.display()
        )));
    }

    let skills_count = skills.len();
    info!("Found {} skills", skills_count);

    let output_path =
        output.unwrap_or_else(|| skill_dir.join(".claude-plugin").join("marketplace.json"));

    let repo_name = name
        .or_else(|| {
            skill_dir
                .file_name()
                .and_then(|n| n.to_str().map(|s| s.to_string()))
        })
        .ok_or_else(|| {
            CliError::Validation(
                "Repository name is required. Use --name or ensure directory has a name."
                    .to_string(),
            )
        })?;

    let skill_paths: Vec<String> = skills
        .iter()
        .map(|skill| format!("./{}", skill.id))
        .collect();

    let plugin = ClaudeCodePlugin {
        name: repo_name.clone(),
        description: description.clone(),
        source: Some("./".to_string()),
        strict: Some(false),
        skills: skill_paths,
    };

    let marketplace = ClaudeCodeMarketplaceJson {
        name: repo_name,
        owner: if owner_name.is_some() || owner_email.is_some() {
            Some(ClaudeCodeOwner {
                name: owner_name.unwrap_or_else(|| "Unknown".to_string()),
                email: owner_email,
            })
        } else {
            None
        },
        metadata: if description.is_some() || version.is_some() {
            Some(ClaudeCodeMetadata {
                description,
                version,
            })
        } else {
            None
        },
        plugins: vec![plugin],
    };

    let json_content = serde_json::to_string_pretty(&marketplace).map_err(|e| {
        CliError::Validation(format!("Failed to serialize marketplace.json: {}", e))
    })?;

    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent).map_err(|e| {
            CliError::Validation(format!("Failed to create output directory: {}", e))
        })?;
    }

    fs::write(&output_path, json_content)
        .map_err(|e| CliError::Validation(format!("Failed to write marketplace.json: {}", e)))?;

    println!(
        "{}",
        messages::ok(&format!(
            "Created marketplace.json: {}",
            output_path.display()
        ))
    );
    println!("   Found {} skills", skills_count);

    Ok(())
}

pub fn scan_directory_for_skills(dir: &Path) -> CliResult<Vec<MarketplaceSkill>> {
    let mut skills = Vec::new();

    for entry in WalkDir::new(dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name() == "SKILL.md")
    {
        let skill_path = entry.path();
        let skill_dir = skill_path
            .parent()
            .ok_or_else(|| CliError::Validation("SKILL.md has no parent directory".to_string()))?;

        match extract_skill_metadata(skill_dir, skill_path) {
            Ok(skill) => {
                info!("Found skill: {} ({})", skill.name, skill.id);
                skills.push(skill);
            }
            Err(e) => {
                warn!(
                    "Failed to extract skill from {}: {}",
                    skill_dir.display(),
                    e
                );
                continue;
            }
        }
    }

    Ok(skills)
}

pub fn extract_skill_metadata(skill_dir: &Path, skill_file: &Path) -> CliResult<MarketplaceSkill> {
    let skill_project_path = skill_dir.join("skill-project.toml");
    let skill_metadata = if skill_project_path.exists() {
        if let Ok(skill_project_content) = fs::read_to_string(&skill_project_path) {
            #[derive(serde::Deserialize)]
            struct SkillProjectToml {
                metadata: Option<MetadataSection>,
            }

            if let Ok(skill_project) = toml::from_str::<SkillProjectToml>(&skill_project_content) {
                skill_project.metadata
            } else {
                None
            }
        } else {
            None
        }
    } else {
        None
    };

    let skill_content = fs::read_to_string(skill_file)
        .map_err(|e| CliError::Validation(format!("Failed to read SKILL.md: {}", e)))?;

    let frontmatter = parse_yaml_frontmatter(&skill_content).map_err(|e| {
        CliError::Validation(format!("Failed to parse SKILL.md frontmatter: {}", e))
    })?;

    let id = skill_metadata
        .as_ref()
        .ok_or_else(|| {
            CliError::Validation(format!(
                "skill-project.toml is required but not found in: {}",
                skill_dir.display()
            ))
        })?
        .id
        .clone()
        .ok_or_else(|| {
            CliError::Validation(
                "skill-project.toml [metadata] section must have a non-empty 'id' field"
                    .to_string(),
            )
        })?;

    let name = frontmatter.name.clone();

    let description = skill_metadata
        .as_ref()
        .and_then(|m| m.description.clone())
        .filter(|d| !d.is_empty())
        .unwrap_or_else(|| frontmatter.description.clone());

    let version = if let Some(metadata) = skill_metadata.as_ref() {
        metadata
            .version
            .clone()
            .unwrap_or_else(|| frontmatter.version.unwrap_or_else(|| "1.0.0".to_string()))
    } else {
        frontmatter.version.unwrap_or_else(|| "1.0.0".to_string())
    };

    let author = skill_metadata
        .as_ref()
        .and_then(|m| m.author.clone())
        .or_else(|| frontmatter.author.clone());

    let download_url = skill_metadata.as_ref().and_then(|m| m.download_url.clone());

    if skill_metadata.is_some() {
        info!("Using metadata from skill-project.toml for skill: {}", id);
    }

    Ok(MarketplaceSkill {
        id,
        name,
        description,
        version,
        author,
        download_url,
    })
}
