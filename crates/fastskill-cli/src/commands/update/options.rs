use super::UpdateArgs;
use crate::error::{CliError, CliResult};
use fastskill_core::core::origin::Origin;
use fastskill_core::core::version::VersionConstraint;

pub(super) fn selected_repository(args: &UpdateArgs) -> CliResult<Option<&str>> {
    match (args.repository.as_deref(), args.source.as_deref()) {
        (Some(repository), Some(alias)) if repository != alias => Err(CliError::Validation(
            "--source is a deprecated alias of --repository; their values must match when both are provided"
                .to_string(),
        )),
        (Some(repository), _) => Ok(Some(repository)),
        (None, alias) => Ok(alias),
    }
}

pub(super) fn validate_update_args(args: &UpdateArgs) -> CliResult<()> {
    let repository = selected_repository(args)?;
    if args.check && args.dry_run {
        return Err(CliError::Validation(
            "--check and --dry-run are mutually exclusive".to_string(),
        ));
    }
    if args.offline && args.reindex {
        return Err(CliError::Validation(
            "--offline and --reindex cannot be used together".to_string(),
        ));
    }
    if !matches!(
        args.strategy.as_str(),
        "latest" | "patch" | "minor" | "major"
    ) {
        return Err(CliError::Validation(format!(
            "Invalid strategy: {}. Use: latest, patch, minor, major",
            args.strategy
        )));
    }
    if args.version.is_some() {
        if args.skill_id.is_none() {
            return Err(CliError::Validation(
                "--to-version requires one SKILL_ID".to_string(),
            ));
        }
        if args.strategy_explicit {
            return Err(CliError::Validation(
                "--to-version cannot be combined with --strategy".to_string(),
            ));
        }
        let version = args.version.as_deref().unwrap_or_default();
        let exact = VersionConstraint::parse(version)
            .map_err(|error| CliError::Validation(format!("Invalid --to-version: {error}")))?;
        if exact.as_exact().is_none() {
            return Err(CliError::Validation(
                "--to-version requires an exact semantic version such as 1.2.3".to_string(),
            ));
        }
    }
    if repository.is_some() && args.skill_id.is_none() {
        return Err(CliError::Validation(
            "--repository requires one SKILL_ID".to_string(),
        ));
    }
    if args.bundle.is_some()
        && (args.version.is_some() || repository.is_some() || args.strategy_explicit)
    {
        return Err(CliError::Validation(
            "bundle updates do not accept skill version, repository, or strategy controls"
                .to_string(),
        ));
    }
    Ok(())
}

pub(super) fn strategy_constraint(
    current: &str,
    strategy: &str,
    recorded: &VersionConstraint,
) -> CliResult<VersionConstraint> {
    if recorded.as_exact().is_some() || matches!(strategy, "latest" | "major") {
        return Ok(recorded.clone());
    }
    let current = semver::Version::parse(current).map_err(|error| {
        CliError::Validation(format!(
            "Locked version is not valid semantic version: {error}"
        ))
    })?;
    let upper = match strategy {
        "patch" => {
            let next_minor = current.minor.checked_add(1).ok_or_else(|| {
                CliError::Validation(format!(
                    "Cannot calculate patch update boundary after {current}"
                ))
            })?;
            format!("{}.{next_minor}.0", current.major)
        }
        "minor" => {
            let next_major = current.major.checked_add(1).ok_or_else(|| {
                CliError::Validation(format!(
                    "Cannot calculate minor update boundary after {current}"
                ))
            })?;
            format!("{next_major}.0.0")
        }
        _ => return Ok(recorded.clone()),
    };
    let recorded = recorded.to_string();
    let combined = if recorded == "*" {
        format!(">={current}, <{upper}")
    } else {
        format!("{recorded}, >={current}, <{upper}")
    };
    VersionConstraint::parse(&combined)
        .map_err(|error| CliError::Validation(format!("Invalid update constraint: {error}")))
}

pub(super) fn controlled_origin(
    origin: &Origin,
    current_version: &str,
    args: &UpdateArgs,
) -> CliResult<Origin> {
    let repository = selected_repository(args)?;
    let mut controlled = origin.clone();
    match &mut controlled {
        Origin::Repository { repo, version, .. } => {
            if let Some(repository) = repository {
                *repo = repository.to_string();
            }
            if let Some(target) = &args.version {
                *version = Some(VersionConstraint::parse(target).map_err(|error| {
                    CliError::Validation(format!("Invalid --to-version: {error}"))
                })?);
            } else if args.strategy_explicit {
                let recorded = version
                    .clone()
                    .unwrap_or_else(|| VersionConstraint::parse("*").expect("valid wildcard"));
                *version = Some(strategy_constraint(
                    current_version,
                    &args.strategy,
                    &recorded,
                )?);
            }
        }
        _ if args.version.is_some() || repository.is_some() || args.strategy_explicit => {
            return Err(CliError::Validation(
                "version, repository, and strategy controls only apply to repository-origin skills"
                    .to_string(),
            ));
        }
        _ => {}
    }
    Ok(controlled)
}
