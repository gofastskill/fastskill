//! Record which `aikit-evals` this binary was built against.
//!
//! A scorecard has to say what produced it (spec eval-scorecard-report R2), and
//! the engine that scored the runs is part of that. `CARGO_PKG_VERSION` only
//! ever names the crate being compiled, so the dependency's identity is read
//! here, from the lockfile that resolved it.

use std::path::PathBuf;

/// `<version> (git <short rev>)` for the locked `aikit-evals`, or `unknown`
/// when the lockfile is absent or does not name it. Never a guess: a wrong
/// version in an artifact is worse than an honest "unknown".
fn locked_aikit_evals(lock: &str) -> Option<String> {
    let mut version = None;
    let mut source = None;
    let mut in_package = false;
    for line in lock.lines() {
        let line = line.trim();
        if line == "[[package]]" {
            if version.is_some() {
                break;
            }
            in_package = false;
            source = None;
            continue;
        }
        if line == r#"name = "aikit-evals""# {
            in_package = true;
            continue;
        }
        if !in_package {
            continue;
        }
        if let Some(rest) = line.strip_prefix("version = ") {
            version = Some(rest.trim_matches('"').to_string());
        } else if let Some(rest) = line.strip_prefix("source = ") {
            source = Some(rest.trim_matches('"').to_string());
        }
    }
    let version = version?;
    let rev = source
        .as_deref()
        .and_then(|s| s.split_once("rev="))
        .map(|(_, rest)| rest.split('#').next().unwrap_or(rest))
        .map(|rev| rev.chars().take(8).collect::<String>());
    Some(match rev {
        Some(rev) => format!("{version} (git {rev})"),
        None => version,
    })
}

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default());
    let lock_path = manifest.join("../../Cargo.lock");
    println!("cargo:rerun-if-changed={}", lock_path.display());
    let version = std::fs::read_to_string(&lock_path)
        .ok()
        .and_then(|lock| locked_aikit_evals(&lock))
        .unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=FASTSKILL_AIKIT_EVALS_VERSION={version}");
}
