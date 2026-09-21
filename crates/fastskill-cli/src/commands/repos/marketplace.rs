use crate::error::{CliError, CliResult};
use crate::utils::messages;
use fastskill_core::core::manifest::MetadataSection;
use fastskill_core::core::metadata::parse_yaml_frontmatter;
use fastskill_core::core::sources::{
    ClaudeCodeMarketplaceJson, ClaudeCodeMetadata, ClaudeCodeOwner, ClaudeCodePlugin,
    MarketplaceSkill,
};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use toml;
use tracing::info;
use walkdir::WalkDir;

/// A skill found by the scan, together with the folder it was found in.
///
/// ADR-0014: the folder is what a catalog entry must point at. Before that
/// decision the scan returned bare [`MarketplaceSkill`]s and the writer
/// reconstructed each path as `./{id}`, which named a real folder only when a
/// skill happened to sit at the repository root under its own id.
#[derive(Debug)]
pub struct ScannedSkill {
    pub skill: MarketplaceSkill,
    pub dir: PathBuf,
}

/// The directory a catalog's `./` paths resolve against: the one holding
/// `.claude-plugin/`, or the catalog file's own directory when it sits at a
/// repository root. It is derived from where the catalog is *written*, not
/// from the directory that was scanned, because those differ whenever a scan
/// is narrowed (`marketplace create ./skills --output .claude-plugin/...`).
fn catalog_root(output_path: &Path) -> CliResult<PathBuf> {
    // Resolve against the working directory first. `--output
    // .claude-plugin/marketplace.json` is the documented spelling, and a
    // relative path's parent chain runs out before the root is reached --
    // stripping `.claude-plugin` from it leaves an empty path, which every
    // skill directory "starts with", so every entry came out as `./` plus the
    // skill's whole absolute path.
    let cwd = std::env::current_dir().map_err(CliError::Io)?;
    let absolute = if output_path.is_absolute() {
        output_path.to_path_buf()
    } else {
        cwd.join(output_path)
    };
    let dir = absolute.parent().map(Path::to_path_buf).unwrap_or(cwd);
    let dir = if dir.file_name().is_some_and(|n| n == ".claude-plugin") {
        dir.parent().map(Path::to_path_buf).unwrap_or(dir)
    } else {
        dir
    };
    // The root must exist to be canonicalized; the output directory itself is
    // created later, so fall back to the uncanonicalized path rather than
    // failing here.
    Ok(dir.canonicalize().unwrap_or(dir))
}

/// `./`-prefixed, `/`-separated path from `root` to `skill_dir`.
fn catalog_relative_path(root: &Path, skill_dir: &Path) -> CliResult<String> {
    let relative = skill_dir.strip_prefix(root).map_err(|_| {
        CliError::Validation(format!(
            "Skill at {} is outside the catalog root {}. A catalog can only list skills \
             beneath the directory its paths resolve against; move the skill under that \
             root, or write the catalog somewhere that contains it.",
            skill_dir.display(),
            root.display()
        ))
    })?;
    let joined = relative
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/");
    if joined.is_empty() {
        return Err(CliError::Validation(format!(
            "Skill at {} is the catalog root itself, so it has no path within the catalog",
            skill_dir.display()
        )));
    }
    Ok(format!("./{}", joined))
}

/// Reject two folders advertising one `id@version`: a catalog cannot say which
/// of them a request for that skill means, and acquisition would fail on the
/// ambiguity later, further from the cause.
fn reject_duplicate_identities(skills: &[ScannedSkill]) -> CliResult<()> {
    let mut seen: HashMap<(&str, &str), &Path> = HashMap::new();
    for scanned in skills {
        let key = (scanned.skill.id.as_str(), scanned.skill.version.as_str());
        if let Some(first) = seen.insert(key, scanned.dir.as_path()) {
            return Err(CliError::Validation(format!(
                "Skills at {} and {} both declare id '{}' version '{}'. Give them distinct \
                 ids or versions in skill-project.toml.",
                first.display(),
                scanned.dir.display(),
                scanned.skill.id,
                scanned.skill.version
            )));
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn execute_create(
    path: PathBuf,
    output: Option<PathBuf>,
    name: Option<String>,
    owner_name: Option<String>,
    owner_email: Option<String>,
    description: Option<String>,
    version: Option<String>,
    check: bool,
) -> CliResult<()> {
    let skill_dir = path
        .canonicalize()
        .map_err(|e| CliError::Validation(format!("Failed to resolve path: {}", e)))?;

    info!("Scanning directory for skills: {}", skill_dir.display());

    let mut skills = scan_directory_for_skills(&skill_dir)?;

    if skills.is_empty() {
        return Err(CliError::Validation(format!(
            "No skills found in directory: {}",
            skill_dir.display()
        )));
    }
    reject_duplicate_identities(&skills)?;

    let skills_count = skills.len();
    info!("Found {} skills", skills_count);

    let output_path =
        output.unwrap_or_else(|| skill_dir.join(".claude-plugin").join("marketplace.json"));
    let root = catalog_root(&output_path)?;

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

    // Claude Code requires `owner`, and rejects the catalog without it.
    let owner_name = owner_name.ok_or_else(|| {
        CliError::Validation(
            "Owner name is required. Use --owner-name: Claude Code rejects a marketplace \
             without an owner."
                .to_string(),
        )
    })?;

    // Deterministic output: a regenerated catalog must differ only where the
    // skills did, so a committed catalog has a reviewable diff and `--check`
    // compares like with like.
    skills.sort_by(|a, b| {
        (&a.skill.id, &a.skill.version, &a.dir).cmp(&(&b.skill.id, &b.skill.version, &b.dir))
    });
    let mut skill_paths: Vec<String> = skills
        .iter()
        .map(|scanned| catalog_relative_path(&root, &scanned.dir))
        .collect::<CliResult<_>>()?;
    skill_paths.sort();

    let plugin = ClaudeCodePlugin {
        name: repo_name.clone(),
        description: description.clone(),
        source: Some("./".to_string()),
        strict: Some(false),
        skills: skill_paths,
    };

    let marketplace = ClaudeCodeMarketplaceJson {
        name: repo_name,
        owner: Some(ClaudeCodeOwner {
            name: owner_name,
            email: owner_email,
        }),
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

    // Trailing newline: this file is meant to be committed, and a file without
    // one shows up as "\ No newline at end of file" in every diff that touches
    // it. `--check` compares the bytes, so it expects the same.
    let json_content = serde_json::to_string_pretty(&marketplace)
        .map(|json| format!("{json}\n"))
        .map_err(|e| {
            CliError::Validation(format!("Failed to serialize marketplace.json: {}", e))
        })?;

    if check {
        return report_check(&output_path, &json_content, skills_count);
    }

    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent).map_err(|e| {
            CliError::Validation(format!("Failed to create output directory: {}", e))
        })?;
    }

    fs::write(&output_path, &json_content)
        .map_err(|e| CliError::Validation(format!("Failed to write marketplace.json: {}", e)))?;

    crate::outln!(
        "{}",
        messages::ok(&format!(
            "Created marketplace.json: {}",
            output_path.display()
        ))
    );
    crate::outln!("   Found {} skills", skills_count);

    Ok(())
}

/// `--check`: compare the committed catalog with what generation would produce
/// and write nothing, so CI fails on a catalog left stale by a skill that was
/// added, moved, renamed, or re-versioned without regenerating.
fn report_check(output_path: &Path, expected: &str, skills_count: usize) -> CliResult<()> {
    let actual = fs::read_to_string(output_path).map_err(|e| {
        CliError::Validation(format!(
            "Cannot check {}: {}. Run `fastskill marketplace create` to generate it.",
            output_path.display(),
            e
        ))
    })?;
    if actual == expected {
        crate::outln!(
            "{}",
            messages::ok(&format!("{} is up to date", output_path.display()))
        );
        crate::outln!("   Found {} skills", skills_count);
        return Ok(());
    }
    Err(CliError::Validation(format!(
        "{} is out of date with the skills on disk. Run `fastskill marketplace create` and \
         commit the result.",
        output_path.display()
    )))
}

/// Walk `dir` for skills, skipping hidden directories.
///
/// A folder that looks like a skill but cannot be read as one is an error, not
/// a warning: skipping it produced a catalog that silently omitted the skill,
/// and the omission only surfaced later as "skill not found" against a catalog
/// that looked complete.
pub fn scan_directory_for_skills(dir: &Path) -> CliResult<Vec<ScannedSkill>> {
    let mut skills = Vec::new();

    for entry in WalkDir::new(dir)
        .into_iter()
        .filter_entry(|e| !is_hidden_child(dir, e))
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name() == "SKILL.md")
    {
        let skill_path = entry.path();
        let skill_dir = skill_path
            .parent()
            .ok_or_else(|| CliError::Validation("SKILL.md has no parent directory".to_string()))?;

        let skill = extract_skill_metadata(skill_dir, skill_path).map_err(|e| {
            CliError::Validation(format!(
                "Cannot catalog the skill at {}: {}",
                skill_dir.display(),
                e
            ))
        })?;
        info!("Found skill: {} ({})", skill.name, skill.id);
        skills.push(ScannedSkill {
            skill,
            dir: skill_dir.to_path_buf(),
        });
    }

    Ok(skills)
}

/// A dot-prefixed directory below the scan root -- `.git`, `.claude-plugin`,
/// editor and tool state. The root itself is exempt so scanning a directory
/// that happens to be hidden still works.
fn is_hidden_child(root: &Path, entry: &walkdir::DirEntry) -> bool {
    entry.path() != root
        && entry.file_type().is_dir()
        && entry
            .file_name()
            .to_str()
            .is_some_and(|n| n.starts_with('.'))
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

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn write_skill(root: &Path, rel: &str, id: &str, version: &str) {
        let dir = root.join(rel);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("SKILL.md"),
            format!("---\nname: {id}\ndescription: d\nversion: {version}\n---\n\n# {id}\n"),
        )
        .unwrap();
        fs::write(
            dir.join("skill-project.toml"),
            format!("[metadata]\nid = \"{id}\"\nversion = \"{version}\"\n"),
        )
        .unwrap();
    }

    /// Paths resolve against the directory holding `.claude-plugin/`, not the
    /// directory that was scanned. Scanning `./skills` while writing the
    /// catalog at the repository root must still produce `./skills/<dir>`.
    #[test]
    fn catalog_root_is_where_the_catalog_is_written() {
        let temp = tempfile::TempDir::new().unwrap();
        let root = temp.path().canonicalize().unwrap();
        fs::create_dir_all(root.join(".claude-plugin")).unwrap();

        assert_eq!(
            catalog_root(&root.join(".claude-plugin").join("marketplace.json")).unwrap(),
            root
        );
        assert_eq!(catalog_root(&root.join("marketplace.json")).unwrap(), root);
    }

    /// A relative `--output` is the documented spelling, and used to leave the
    /// root empty -- which made every entry `./` plus the skill's absolute
    /// path.
    #[test]
    fn a_relative_output_resolves_against_the_working_directory() {
        let cwd = std::env::current_dir().unwrap().canonicalize().unwrap();
        assert_eq!(
            catalog_root(Path::new(".claude-plugin/marketplace.json")).unwrap(),
            cwd
        );
        assert_eq!(catalog_root(Path::new("marketplace.json")).unwrap(), cwd);
    }

    #[test]
    fn relative_paths_are_slash_separated_and_dot_prefixed() {
        let root = Path::new("/repo");
        assert_eq!(
            catalog_relative_path(root, Path::new("/repo/workspace/cli-rust-dev")).unwrap(),
            "./workspace/cli-rust-dev"
        );
        assert_eq!(
            catalog_relative_path(root, Path::new("/repo/one")).unwrap(),
            "./one"
        );
    }

    /// A skill the catalog's `./` paths cannot express is refused outright,
    /// rather than written as a path that resolves to the wrong place.
    #[test]
    fn a_skill_outside_the_catalog_root_is_refused() {
        let root = Path::new("/repo");
        let err = catalog_relative_path(root, Path::new("/elsewhere/skill")).unwrap_err();
        assert!(
            err.to_string().contains("outside the catalog root"),
            "{err}"
        );

        let err = catalog_relative_path(root, root).unwrap_err();
        assert!(err.to_string().contains("catalog root itself"), "{err}");
    }

    /// Two folders claiming one `id@version` make the catalog ambiguous about
    /// which one a request for that skill means, so generation stops here
    /// rather than letting acquisition fail later, further from the cause.
    #[test]
    fn duplicate_identities_are_rejected_and_name_both_folders() {
        let temp = tempfile::TempDir::new().unwrap();
        let root = temp.path();
        write_skill(root, "a/dup", "dup", "1.0.0");
        write_skill(root, "b/dup", "dup", "1.0.0");
        write_skill(root, "c/dup", "dup", "2.0.0");

        let scanned = scan_directory_for_skills(root).unwrap();
        let err = reject_duplicate_identities(&scanned).unwrap_err();
        assert!(err.to_string().contains("both declare id 'dup'"), "{err}");

        // Same id at different versions is not a duplicate.
        let distinct: Vec<_> = scanned
            .into_iter()
            .filter(|s| s.skill.version == "2.0.0")
            .collect();
        reject_duplicate_identities(&distinct).unwrap();
    }

    /// `.git` alone holds thousands of files, and a catalog has no business
    /// listing a skill out of `.git`, `.claude-plugin` or editor state.
    #[test]
    fn hidden_directories_are_not_scanned() {
        let temp = tempfile::TempDir::new().unwrap();
        let root = temp.path();
        write_skill(root, "visible", "visible", "1.0.0");
        write_skill(root, ".hidden/sk", "hidden-skill", "1.0.0");
        write_skill(root, ".git/sk", "git-skill", "1.0.0");

        let found = scan_directory_for_skills(root).unwrap();
        assert_eq!(
            found
                .iter()
                .map(|s| s.skill.id.as_str())
                .collect::<Vec<_>>(),
            vec!["visible"]
        );
    }

    /// A folder that looks like a skill but cannot be read as one used to be
    /// warn-skipped, producing a catalog that silently omitted it; the
    /// omission then surfaced as "skill not found" against a catalog that
    /// looked complete.
    #[test]
    fn an_unreadable_skill_is_an_error_not_a_silent_omission() {
        let temp = tempfile::TempDir::new().unwrap();
        let root = temp.path();
        write_skill(root, "good", "good", "1.0.0");
        let broken = root.join("broken");
        fs::create_dir_all(&broken).unwrap();
        fs::write(
            broken.join("SKILL.md"),
            "---\nname: b\ndescription: d\n---\n",
        )
        .unwrap();

        let err = scan_directory_for_skills(root).unwrap_err();
        assert!(
            err.to_string().contains("Cannot catalog the skill at"),
            "{err}"
        );
        assert!(
            err.to_string().contains("skill-project.toml is required"),
            "{err}"
        );
    }
}
