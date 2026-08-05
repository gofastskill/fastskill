//! Read-shorthand routing: deciding when `fastskill <token>` means
//! `fastskill read <token>`.
//!
//! `fastskill <skill-id>` is a shorthand for `fastskill read <skill-id>`, which
//! means any unrecognised first positional would otherwise be silently treated
//! as a skill name. That turned every command typo into a misleading error --
//! `fastskill insatll` reported `Skill 'insatll' not found` rather than
//! suggesting `install`.
//!
//! The rule implemented here: route to `read` *unless* the token is a near miss
//! of a known command and no installed skill actually claims that name. When we
//! decline to route, the caller reports an unrecognised subcommand along with
//! the nearest command computed by [`nearest_command`].
//!
//! An existing skill always wins. `fastskill repo` reads a skill named `repo`
//! even though the `repos` command is one edit away -- we only intercept names
//! that resolve to nothing.

use std::collections::HashSet;

/// Maximum edit distance at which a token is treated as a typo of a command.
///
/// Deliberately 1, using Damerau-Levenshtein so that a single transposition
/// counts as one edit. That covers the overwhelming majority of real typos
/// (`insatll`, `serach`, `doctr`, `lst`) while keeping the blast radius small:
/// only names exactly one edit from a command can ever be intercepted, and even
/// then only when no such skill exists.
const MAX_TYPO_DISTANCE: usize = 1;

/// Damerau-Levenshtein distance (optimal string alignment), capped at `max`.
///
/// Returns `max + 1` for any pair further apart than `max`, which is all the
/// caller needs and lets us bail out early. Unlike plain Levenshtein this counts
/// a transposition of two adjacent characters as a single edit, so `insatll` is
/// distance 1 from `install` rather than 2.
fn distance_within(a: &str, b: &str, max: usize) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let over = max + 1;

    // Length difference alone already exceeds the budget.
    if a.len().abs_diff(b.len()) > max {
        return over;
    }

    let mut prev_prev: Vec<usize> = vec![0; b.len() + 1];
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut curr: Vec<usize> = vec![0; b.len() + 1];

    for i in 1..=a.len() {
        curr[0] = i;
        let mut row_best = curr[0];

        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            let mut best = (curr[j - 1] + 1) // insertion
                .min(prev[j] + 1) // deletion
                .min(prev[j - 1] + cost); // substitution

            // Transposition of two adjacent characters.
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                best = best.min(prev_prev[j - 2] + 1);
            }

            curr[j] = best;
            row_best = row_best.min(best);
        }

        // Every alignment through this row already costs more than the budget.
        if row_best > max {
            return over;
        }

        std::mem::swap(&mut prev_prev, &mut prev);
        std::mem::swap(&mut prev, &mut curr);
    }

    prev[b.len()].min(over)
}

/// The known command closest to `token`, if any lies within the typo budget.
///
/// Ties are broken by shortest-then-alphabetical so the result is deterministic
/// regardless of `HashSet` iteration order. An exact match is not a typo --
/// those tokens never reach this code path because they dispatch as commands.
fn nearest_command<'a>(token: &str, known: &HashSet<&'a str>) -> Option<&'a str> {
    known
        .iter()
        .filter(|cmd| **cmd != token)
        .filter_map(|cmd| {
            let d = distance_within(token, cmd, MAX_TYPO_DISTANCE);
            (d <= MAX_TYPO_DISTANCE).then_some((d, cmd.len(), *cmd))
        })
        .min()
        .map(|(_, _, cmd)| cmd)
}

/// How a bare first positional should be handled.
#[derive(Debug, PartialEq, Eq)]
pub enum Routing<'a> {
    /// Rewrite to `read <token>`.
    Read,
    /// Report a command typo, suggesting this command.
    DidYouMean(&'a str),
}

/// Decide how to handle a bare first positional that matched no command.
///
/// `skill_exists` is consulted only when the token looks like a typo, so the
/// (cheap, but non-zero) filesystem probe stays off the hot path for ordinary
/// skill reads.
pub fn route<'a>(
    token: &str,
    known: &HashSet<&'a str>,
    skill_exists: impl Fn(&str) -> bool,
) -> Routing<'a> {
    match nearest_command(token, known) {
        // A real skill outranks a command typo.
        Some(cmd) if !skill_exists(token) => Routing::DidYouMean(cmd),
        _ => Routing::Read,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn known() -> HashSet<&'static str> {
        [
            "init",
            "install",
            "update",
            "list",
            "read",
            "add",
            "remove",
            "search",
            "reindex",
            "serve",
            "doctor",
            "repos",
            "marketplace",
            "eval",
            "optimize",
            "analyze",
            "spec",
            "mcp",
            "help",
        ]
        .into_iter()
        .collect()
    }

    fn no_skills(_: &str) -> bool {
        false
    }

    fn routes_to_read(token: &str, known: &HashSet<&str>, exists: impl Fn(&str) -> bool) -> bool {
        route(token, known, exists) == Routing::Read
    }

    // ── distance ────────────────────────────────────────────────────────────

    #[test]
    fn identical_strings_are_distance_zero() {
        assert_eq!(distance_within("install", "install", 1), 0);
    }

    #[test]
    fn single_transposition_is_one_edit() {
        // Plain Levenshtein would score these 2; Damerau scores 1.
        assert_eq!(distance_within("insatll", "install", 1), 1);
        assert_eq!(distance_within("serach", "search", 1), 1);
    }

    #[test]
    fn single_deletion_insertion_substitution_are_one_edit() {
        assert_eq!(distance_within("doctr", "doctor", 1), 1); // deletion
        assert_eq!(distance_within("lst", "list", 1), 1); // deletion
        assert_eq!(distance_within("installs", "install", 1), 1); // insertion
        assert_eq!(distance_within("lost", "list", 1), 1); // substitution
    }

    #[test]
    fn two_edits_exceed_the_cap() {
        // `installer` is 2 edits from `install` and must not be treated as a typo.
        assert!(distance_within("installer", "install", 1) > 1);
        assert!(distance_within("xyz", "install", 1) > 1);
    }

    #[test]
    fn length_gap_short_circuits() {
        assert!(distance_within("a", "abcdefgh", 1) > 1);
    }

    #[test]
    fn distance_is_symmetric() {
        assert_eq!(
            distance_within("insatll", "install", 2),
            distance_within("install", "insatll", 2)
        );
    }

    #[test]
    fn empty_token_is_not_within_one_of_a_command() {
        assert!(distance_within("", "install", 1) > 1);
    }

    // ── routing ─────────────────────────────────────────────────────────────

    #[test]
    fn command_typos_do_not_route_to_read() {
        for typo in ["insatll", "serach", "doctr", "lst", "ini", "serv"] {
            assert!(
                !routes_to_read(typo, &known(), no_skills),
                "{typo} should surface as an unrecognized subcommand"
            );
        }
    }

    #[test]
    fn ordinary_skill_names_route_to_read() {
        for name in [
            "pdf-processor",
            "installer",
            "my-skill",
            "data-viz",
            "fastskill",
        ] {
            assert!(
                routes_to_read(name, &known(), no_skills),
                "{name} should be read as a skill id"
            );
        }
    }

    #[test]
    fn an_installed_skill_outranks_a_command_typo() {
        // `repo` is one edit from the `repos` command, but a skill named `repo`
        // exists -- reading it must still win.
        assert!(routes_to_read("repo", &known(), |t| t == "repo"));
        assert!(routes_to_read("specs", &known(), |t| t == "specs"));
        // ...and with no such skill installed, it is treated as a typo.
        assert!(!routes_to_read("repo", &known(), no_skills));
    }

    #[test]
    fn skill_existence_is_only_probed_for_typo_candidates() {
        use std::cell::Cell;
        let probed = Cell::new(false);
        let probe = |_: &str| {
            probed.set(true);
            false
        };
        // Nowhere near a command name -> no probe.
        assert!(routes_to_read("pdf-processor", &known(), probe));
        assert!(!probed.get());
        // One edit away -> probe.
        assert!(!routes_to_read("lst", &known(), probe));
        assert!(probed.get());
    }

    #[test]
    fn exact_command_names_are_not_their_own_typos() {
        // Defensive: these dispatch as commands before reaching this code, but
        // the predicate must not classify them as typos of themselves.
        let mut h = HashSet::new();
        h.insert("install");
        assert_eq!(nearest_command("install", &h), None);
    }

    // ── suggestion quality ──────────────────────────────────────────────────

    #[test]
    fn suggests_the_actually_nearest_command() {
        // Regression guard: clap's own suggester answers `init` for `insatll`
        // and `serve` for `serach`. Running the suggested `init` would scaffold
        // a project the user never asked for, so we compute the suggestion
        // ourselves and must keep beating it.
        for (typo, expected) in [
            ("insatll", "install"),
            ("serach", "search"),
            ("doctr", "doctor"),
            ("lst", "list"),
            ("updte", "update"),
        ] {
            assert_eq!(
                route(typo, &known(), no_skills),
                Routing::DidYouMean(expected),
                "{typo} should suggest {expected}"
            );
        }
    }

    #[test]
    fn suggestion_is_deterministic_across_iteration_orders() {
        // `HashSet` iteration order varies per process; the tie-break must not.
        let first = route("rea", &known(), no_skills);
        for _ in 0..50 {
            assert_eq!(route("rea", &known(), no_skills), first);
        }
    }
}
