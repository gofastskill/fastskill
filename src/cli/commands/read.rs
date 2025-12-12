//! Read command - streams skill documentation to stdout

use crate::cli::error::{CliError, CliResult};
use clap::Args;
use fastskill::FastSkillService;
use std::sync::Arc;

/// Read skill documentation
#[derive(Debug, Args)]
pub struct ReadArgs {
    /// Skill ID to read
    #[arg(help = "Skill identifier (e.g., 'pptx', 'scope/pptx', 'pptx@1.2.3')")]
    skill_id: String,
}

/// Execute the read command
pub async fn execute_read(service: Arc<FastSkillService>, args: ReadArgs) -> CliResult<()> {
    // T007, T022: Parse and validate skill ID
    let skill_id = fastskill::SkillId::new(args.skill_id.clone()).map_err(|e| {
        eprintln!("Error: Invalid skill ID format: '{}'", args.skill_id);
        eprintln!();
        eprintln!("Expected format:");
        eprintln!("  - id");
        eprintln!("  - scope/id");
        eprintln!("  - id@version or scope/id@version (if supported)");
        CliError::Validation(format!("Invalid skill ID format: {}", e))
    })?;

    // T011, T021: Implement skill resolution via skill_manager().get_skill()
    // Try exact match first
    let skill =
        match service.skill_manager().get_skill(&skill_id).await.map_err(CliError::Service)? {
            Some(skill) => skill,
            None => {
                // T024: If no exact match, check for multiple partial matches
                let all_skills =
                    service.skill_manager().list_skills(None).await.map_err(CliError::Service)?;

                let matching_skills: Vec<_> = all_skills
                    .iter()
                    .filter(|s| {
                        let skill_id_str = s.id.to_string();
                        skill_id_str.ends_with(&format!("/{}", args.skill_id))
                            || skill_id_str == args.skill_id
                            || s.name == args.skill_id
                    })
                    .collect();

                // T024: If multiple matches found, return error with disambiguation instructions
                if matching_skills.len() > 1 {
                    eprintln!("Error: Multiple skills match '{}':", args.skill_id);
                    eprintln!();
                    for skill in &matching_skills {
                        eprintln!("  - {} ({})", skill.id, skill.version);
                    }
                    eprintln!();
                    eprintln!("To disambiguate, use:");
                    eprintln!("  - Version qualifier: <skill-id>@<version>");
                    eprintln!("  - Full identifier: <scope>/<id>");
                    return Err(CliError::Validation(format!(
                        "Multiple skills match '{}'",
                        args.skill_id
                    )));
                }

                // If single match found, use it; otherwise return not found error
                if matching_skills.is_empty() {
                    // T023: Implement error message formatting with "Searched:" and "Try:" sections
                    eprintln!("Error: Skill '{}' not found", args.skill_id);
                    eprintln!();
                    eprintln!("Searched:");
                    eprintln!("  {}", service.config().skill_storage_path.display());
                    eprintln!();
                    eprintln!("Try:");
                    eprintln!("  fastskill list");
                    eprintln!("  fastskill add <source>");
                    eprintln!("  fastskill install");
                    return Err(CliError::Validation(format!(
                        "Skill '{}' not found",
                        args.skill_id
                    )));
                }

                // We know there's exactly one match at this point
                matching_skills[0].clone()
            }
        };

    // T012: Implement base directory extraction from skill_file.parent()
    let base_dir = skill
        .skill_file
        .parent()
        .ok_or_else(|| CliError::Config("Failed to determine skill base directory".to_string()))?;

    // T015: Implement absolute path resolution using canonicalize()
    let base_dir_absolute = base_dir
        .canonicalize()
        .map_err(|e| CliError::Config(format!("Failed to resolve absolute path: {}", e)))?;

    // T044: Implement file size check (500KB limit) before reading
    let metadata = std::fs::metadata(&skill.skill_file).map_err(|e| {
        eprintln!("Error: Failed to load skill '{}': {}", args.skill_id, e);
        CliError::Io(e)
    })?;

    const MAX_FILE_SIZE: u64 = 512_000; // 500KB = 512,000 bytes
    if metadata.len() > MAX_FILE_SIZE {
        eprintln!(
            "Error: Skill '{}' documentation exceeds size limit",
            args.skill_id
        );
        eprintln!();
        eprintln!("File size: {} bytes", metadata.len());
        eprintln!("Maximum size: 500KB ({} bytes)", MAX_FILE_SIZE);
        eprintln!();
        eprintln!("Please reduce the size of SKILL.md or split content into reference files.");
        return Err(CliError::Validation(format!(
            "File size {} exceeds maximum of {} bytes",
            metadata.len(),
            MAX_FILE_SIZE
        )));
    }

    // T013, T045: Implement file reading with error handling for corrupted/unreadable files
    let content = std::fs::read_to_string(&skill.skill_file).map_err(|e| {
        eprintln!("Error: Failed to load skill '{}': {}", args.skill_id, e);
        eprintln!();
        match e.kind() {
            std::io::ErrorKind::PermissionDenied => {
                eprintln!("Permission denied: {}", skill.skill_file.display());
            }
            std::io::ErrorKind::NotFound => {
                eprintln!("File not readable: {}", skill.skill_file.display());
            }
            _ => {
                eprintln!("I/O error: {}", e);
            }
        }
        CliError::Io(e)
    })?;

    // T014: Implement structured output format (header, base directory, content, footer)
    // T016: Ensure plain text output with no ANSI colors or formatting codes
    println!("Reading: {}", args.skill_id);
    println!("Base directory: {}", base_dir_absolute.display());
    println!();
    print!("{}", content);
    if !content.ends_with('\n') {
        println!();
    }
    println!();
    println!("Skill read: {}", args.skill_id);

    Ok(())
}
