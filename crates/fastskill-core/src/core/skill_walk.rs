//! Shared skill-directory walking helper (spec 010).
//!
//! Both scan sites that discover skills under `skill_storage_path` —
//! [`crate::core::service`]'s filesystem auto-indexer (backs `list`/`search`)
//! and [`crate::core::reindex`]'s `find_skill_files` (backs `reindex`) — used
//! plain `walkdir::WalkDir::new(dir)` with no `.follow_links(...)`, which
//! defaults to `false`. That silently hid the develop-in-place / editable
//! workflow: `ln -s ~/dev/my-skill <storage>/my-skill`. A directory that is
//! itself a symlink is yielded as a symlink entry (`is_symlink()`, not
//! `is_dir()`), so walkdir never descends into it and the `SKILL.md` inside
//! is never found.
//!
//! The fix is deliberately **not** `.follow_links(true)` on the whole walk
//! (spec 010's Option B): that trades a silent-omission bug for a silent
//! over-inclusion bug (the walk could leave `skill_storage_path` entirely via
//! an arbitrarily deep chain of nested links) and a crash-on-cycle bug
//! (walkdir surfaces a symlink loop as an `Err` mid-iteration, which
//! `service.rs` turned into a hard `ServiceError` that failed the whole
//! index for one bad link).
//!
//! Instead (Option A): follow a symlink **only when the skill directory
//! itself is the symlink** — i.e. only the top-level entries of
//! `skill_storage_path`. This walks each top-level entry as its **own**
//! `WalkDir` root with `.follow_links(false)`. That resolves exactly one
//! intentional hop, because walkdir always stats a walk's root argument with
//! `fs::metadata` (which follows a symlink) to decide whether to descend,
//! regardless of the `follow_links` setting — `follow_links` only governs
//! entries encountered *while descending*, never the root itself. Re-rooting
//! a walk at each top-level child therefore makes a symlinked skill directory
//! get the one-hop-descend treatment as *its own* walk's root, while any
//! symlink nested any deeper (inside a skill's checkout) is never followed —
//! no scan escape, no risk from a link chain more than one hop deep.
//!
//! A self-referential (or otherwise broken/looping) top-level symlink surfaces
//! as a single `Err` at that one child walk's depth 0 (`FilesystemLoop` for a
//! cycle, `NotFound` for a dangling link) — it does not hang, and because each
//! top-level child gets its own independent `WalkDir` iterator, it cannot
//! poison the other children's walks. Callers are expected to skip such an
//! `Err` with a `tracing::warn!` rather than fail the whole scan.

use std::path::{Path, PathBuf};
use walkdir::{DirEntry, WalkDir};

/// Yield every entry under `root`, resolving one intentional hop for any
/// top-level entry that is itself a symlink to a directory, while never
/// following a symlink encountered anywhere else in the tree. See the module
/// doc for the rationale.
///
/// `skip_dir_name` decides whether a directory (at any depth, including a
/// top-level entry) should be skipped entirely — neither yielded nor
/// descended into. Pass `|_| false` to skip nothing.
///
/// Entries are `walkdir::Result`s: a per-entry `Err` (unreadable directory,
/// broken/looping symlink, permission error, …) is passed through rather than
/// short-circuiting the rest of `root`'s children — every child gets its own
/// independent walk, so one bad entry cannot hide the others.
pub(crate) fn walk_skill_storage(
    root: &Path,
    skip_dir_name: impl Fn(&str) -> bool + Copy + 'static,
) -> impl Iterator<Item = walkdir::Result<DirEntry>> {
    let children: Vec<PathBuf> = std::fs::read_dir(root)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_name()
                .to_str()
                .map(|n| !skip_dir_name(n))
                .unwrap_or(true)
        })
        .map(|e| e.path())
        .collect();

    children.into_iter().flat_map(move |child| {
        WalkDir::new(child)
            .follow_links(false)
            .into_iter()
            .filter_entry(move |e| {
                e.file_name()
                    .to_str()
                    .map(|n| !skip_dir_name(n))
                    .unwrap_or(true)
            })
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write_skill_md(dir: &Path, marker: &str) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join("SKILL.md"), marker).unwrap();
    }

    fn skill_md_paths(root: &Path, skip: impl Fn(&str) -> bool + Copy + 'static) -> Vec<PathBuf> {
        walk_skill_storage(root, skip)
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file() && e.file_name() == "SKILL.md")
            .map(|e| e.path().to_path_buf())
            .collect()
    }

    #[test]
    fn finds_skill_md_in_a_regular_nested_directory() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().join("store");
        write_skill_md(&root.join("my-skill"), "regular");

        let found = skill_md_paths(&root, |_| false);
        assert_eq!(found, vec![root.join("my-skill").join("SKILL.md")]);
    }

    #[test]
    fn finds_skill_md_through_a_top_level_symlinked_directory() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().join("store");
        std::fs::create_dir_all(&root).unwrap();
        let real_target = tmp.path().join("dev-checkout");
        write_skill_md(&real_target, "linked");

        #[cfg(unix)]
        std::os::unix::fs::symlink(&real_target, root.join("linked-skill")).unwrap();

        let found = skill_md_paths(&root, |_| false);
        assert_eq!(found, vec![root.join("linked-skill").join("SKILL.md")]);
    }

    #[test]
    fn does_not_follow_a_symlink_nested_below_the_top_level() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().join("store");
        let skill_dir = root.join("my-skill");
        write_skill_md(&skill_dir, "top");

        // A symlink *inside* the skill dir pointing at another SKILL.md
        // elsewhere must not be followed — only the top-level hop is
        // resolved.
        let elsewhere = tmp.path().join("elsewhere");
        write_skill_md(&elsewhere, "nested-target");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&elsewhere, skill_dir.join("nested-link")).unwrap();

        let found = skill_md_paths(&root, |_| false);
        assert_eq!(found, vec![skill_dir.join("SKILL.md")]);
    }

    #[test]
    fn a_self_referential_top_level_symlink_surfaces_as_a_single_err_not_a_hang() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().join("store");
        std::fs::create_dir_all(&root).unwrap();

        write_skill_md(&root.join("good-skill"), "good");

        #[cfg(unix)]
        std::os::unix::fs::symlink(root.join("looping"), root.join("looping")).unwrap();

        let entries: Vec<_> = walk_skill_storage(&root, |_| false).collect();
        let errs = entries.iter().filter(|e| e.is_err()).count();
        let oks = entries.iter().filter(|e| e.is_ok()).count();

        assert_eq!(errs, 1, "the cyclic symlink must produce exactly one Err");
        assert!(
            oks >= 1,
            "the unrelated skill's entries must still be yielded"
        );

        let found = skill_md_paths(&root, |_| false);
        assert_eq!(found, vec![root.join("good-skill").join("SKILL.md")]);
    }

    #[test]
    fn skip_dir_name_excludes_a_top_level_directory_entirely() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().join("store");
        write_skill_md(&root.join(".hidden"), "hidden");
        write_skill_md(&root.join("visible-skill"), "visible");

        let found = skill_md_paths(&root, |n| n.starts_with('.'));
        assert_eq!(found, vec![root.join("visible-skill").join("SKILL.md")]);
    }

    #[test]
    fn skip_dir_name_excludes_a_nested_directory() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().join("store");
        write_skill_md(&root.join("my-skill").join(".git"), "nested-hidden");
        write_skill_md(&root.join("my-skill"), "top");

        let found = skill_md_paths(&root, |n| n.starts_with('.'));
        assert_eq!(found, vec![root.join("my-skill").join("SKILL.md")]);
    }

    #[test]
    fn nonexistent_root_yields_no_entries() {
        let tmp = TempDir::new().unwrap();
        let missing = tmp.path().join("does-not-exist");

        let found = skill_md_paths(&missing, |_| false);
        assert!(found.is_empty());
    }
}
