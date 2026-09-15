//! The v1 → v2 manifest upgrade and the v2 validation it makes unnecessary.
//!
//! Split out of `manifest.rs` so the loader stays readable; see
//! [`SkillProjectToml::from_toml_str`] for where these are dispatched from.

use super::*;
use crate::core::origin_infer::{
    github_tree_path, is_github_tree_url, parse_git_url, tree_path_subdir,
};

impl SkillProjectToml {
    /// Upgrade a v1 (or legacy, already lifted to v1) manifest to v2 in memory.
    ///
    /// v1 allowed a git origin's `url` to be a GitHub browser link
    /// (`/org/repo/tree/<branch>[/<subdir>]`) — `skill add` wrote them that way until
    /// 0.9.221. Git cannot clone such a URL, so each one is split into the v2 shape: the
    /// clone URL in `url`, the subdirectory in `subdir`, the branch in `ref`.
    ///
    /// Everything else is left exactly as it was. In particular a plain URL is never
    /// touched — not even to append `.git` — so upgrading an already-correct file is a
    /// no-op, and a conflict between the URL and an explicit `ref`/`subdir` is reported
    /// rather than resolved by guessing which the author meant.
    pub(super) fn upgrade_v1_to_v2(&mut self) -> Result<(), ManifestError> {
        self.schema_version = Some(MANIFEST_SCHEMA_VERSION.to_string());
        let Some(section) = self.dependencies.as_mut() else {
            return Ok(());
        };

        for (id, spec) in section.dependencies.iter_mut() {
            let DependencySpec::Inline {
                origin: Origin::Git { url, r#ref, subdir },
                ..
            } = spec
            else {
                continue;
            };
            let Some(tree_path) = github_tree_path(url) else {
                continue;
            };
            let info = parse_git_url(url).map_err(|error| {
                ManifestError::Parse(format!(
                    "dependency '{id}' has an unusable git url '{url}': {error}"
                ))
            })?;

            // An explicitly declared branch or tag is authoritative: it is the only thing
            // that can say where the branch ends and the subdirectory begins when the
            // branch name itself contains a `/`.
            let derived_subdir = match r#ref {
                GitRef::Branch(name) | GitRef::Tag(name) => tree_path_subdir(&tree_path, name)
                    .ok_or_else(|| {
                        ManifestError::Parse(format!(
                            "dependency '{id}' declares ref '{name}' but its url '{url}' points \
                             at '/tree/{tree_path}'; the ref does not match the /tree/ path — \
                             fix one of them"
                        ))
                    })?,
                GitRef::Default | GitRef::Commit(_) => info.subdir.clone(),
            };

            // An explicit `subdir` only conflicts when the URL names a *different* one. A
            // URL that stops at the branch says nothing about the subdirectory, so the
            // declared value simply stands.
            if let (Some(declared), Some(derived)) = (subdir.as_ref(), derived_subdir.as_ref()) {
                if declared != derived {
                    return Err(ManifestError::Parse(format!(
                        "dependency '{id}' sets subdir '{}' but its url '{url}' points at \
                         '{}'; the two disagree — fix one of them",
                        declared.display(),
                        derived.display()
                    )));
                }
            }
            let derived_subdir = subdir.take().or(derived_subdir);

            if matches!(r#ref, GitRef::Default) {
                if let Some(branch) = &info.branch {
                    *r#ref = GitRef::Branch(branch.clone());
                }
            }
            *url = info.repo_url;
            *subdir = derived_subdir;

            crate::utils::warn_once(
                &format!("manifest-tree-url:{id}:{url}"),
                format!(
                    "manifest: rewrote git origin for '{id}': url={url} subdir={} ref={:?} \
                     (schema {MANIFEST_SCHEMA_V1} -> {MANIFEST_SCHEMA_VERSION}; will be saved \
                     on next write)",
                    subdir
                        .as_ref()
                        .map(|path| path.display().to_string())
                        .unwrap_or_else(|| "-".to_string()),
                    r#ref
                ),
            );
        }

        Ok(())
    }

    /// Reject what v2 promises cannot be there: a git origin whose `url` is a GitHub browser
    /// link. A file that declares v2 is taken at its word, so this is an error with the
    /// corrected TOML in it rather than a silent repair.
    pub(super) fn validate_v2(&self) -> Result<(), ManifestError> {
        let Some(section) = self.dependencies.as_ref() else {
            return Ok(());
        };
        for (id, spec) in &section.dependencies {
            let DependencySpec::Inline {
                origin: Origin::Git { url, .. },
                ..
            } = spec
            else {
                continue;
            };
            if !is_github_tree_url(url) {
                continue;
            }
            let info = parse_git_url(url).ok();
            let repo_url = info
                .as_ref()
                .map(|info| info.repo_url.clone())
                .unwrap_or_else(|| url.clone());
            let subdir = info
                .as_ref()
                .and_then(|info| info.subdir.as_ref())
                .map(|path| format!("subdir = \"{}\"\n", path.display()))
                .unwrap_or_default();
            let branch = info
                .as_ref()
                .and_then(|info| info.branch.as_ref())
                .map(|branch| format!("[dependencies.{id}.origin.ref]\nbranch = \"{branch}\"\n"))
                .unwrap_or_default();
            return Err(ManifestError::Parse(format!(
                "dependency '{id}' declares the GitHub browser url '{url}', which git cannot \
                 clone. Schema version {MANIFEST_SCHEMA_VERSION} keeps the repository, the \
                 subdirectory and the branch apart:\n\n\
                 [dependencies.{id}.origin]\ntype = \"git\"\nurl = \"{repo_url}\"\n{subdir}{branch}"
            )));
        }
        Ok(())
    }
}
