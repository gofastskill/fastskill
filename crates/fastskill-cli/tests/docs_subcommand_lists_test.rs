#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! Deterministic "documented subcommand list vs `fastskill spec`" parity test.
//! No LLM, no network.
//!
//! ## Why this is separate from `spec_docs_parity_test`
//!
//! That test reads ```bash fences under `webdocs/cli-reference/` and asks "does
//! every invocation shown resolve to a real command?". It structurally cannot
//! see the drift this one catches, which lives in prose and tables and takes
//! the form of an *enumeration*:
//!
//! ```text
//! | `fastskill eval <cmd>` | Skill quality evals (`validate/run/report/score`) |
//! ```
//!
//! Every one of those four names resolves. The defect is the two that are
//! absent: `eval` had gained `judge` and `scorecard`. `mcp` had likewise gained
//! `register`. Five separate documents advertised the older sets. This drift
//! recurs on every command added, and had already recurred twice by the time
//! this test was written -- hence a gate rather than another one-off fix.
//!
//! ## What counts as a subcommand list
//!
//! A line that (a) mentions `fastskill <group>` for a real command group and
//! (b) contains a separator-delimited run naming at least **two** real
//! subcommands of that group. Both halves matter: (a) alone matches ordinary
//! prose, (b) alone matches unrelated slash-separated words, and the
//! two-real-names floor is what stops `Run/install the MCP server` from being
//! read as a claim about `install`.
//!
//! Extraction is deliberately conservative -- a missed list is much cheaper
//! than an invented one, because false positives are what get parity tests
//! deleted. [`scanner_extracts_lists_and_ignores_prose`] pins both the shapes
//! it must catch and the shapes it must ignore.
//!
//! ## Sources of truth
//!
//! CLI: `fastskill spec --format json`, shelled out to via
//! `CARGO_BIN_EXE_fastskill` -- `fastskill-cli` is bin-only, so the command
//! tree cannot be built in-process. Docs: `README.md` plus every `.md`/`.mdx`
//! under `webdocs/`.
//!
//! ## Allowlist
//!
//! Intentional divergences live in `spec_docs_parity_allowlist.toml` next to
//! this file, under `[[subcommand_list_omission]]` and
//! `[[subcommand_list_unknown_token]]`, each carrying a `reason`.

use regex::Regex;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

// ---------------------------------------------------------------------------
// CLI truth
// ---------------------------------------------------------------------------

/// One command group, its real subcommands, and the compiled pattern that
/// decides whether a documentation line is talking about it.
struct Group {
    name: String,
    subs: BTreeSet<String>,
    mention: Regex,
}

/// `group -> {subcommand}` for every group in `fastskill spec --format json`.
fn cli_groups() -> BTreeMap<String, BTreeSet<String>> {
    let output = Command::new(env!("CARGO_BIN_EXE_fastskill"))
        .args(["spec", "--format", "json"])
        .output()
        .expect("spawn `fastskill spec --format json`");

    assert!(
        output.status.success(),
        "`fastskill spec --format json` exited with {}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );

    let doc: serde_json::Value = serde_json::from_slice(&output.stdout)
        .expect("parse `fastskill spec --format json` stdout as JSON");

    let mut groups: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for command in doc["commands"]
        .as_array()
        .expect("`commands` array in `fastskill spec` JSON output")
    {
        let path = command["path"]
            .as_str()
            .expect("command `path` string in `fastskill spec` JSON output");
        // Only nested paths ("eval/judge") name a group and a subcommand. A
        // deeper path, should one ever appear, keys on its immediate parent.
        if let Some((group, sub)) = path.rsplit_once('/') {
            groups
                .entry(group.replace('/', " "))
                .or_default()
                .insert(sub.to_string());
        }
    }
    assert!(
        !groups.is_empty(),
        "`fastskill spec` reported no command groups at all -- did the JSON shape change?"
    );
    groups
}

fn compile_groups(groups: &BTreeMap<String, BTreeSet<String>>) -> Vec<Group> {
    groups
        .iter()
        .map(|(name, subs)| Group {
            name: name.clone(),
            subs: subs.clone(),
            mention: Regex::new(&format!(r"\bfastskill\s+{}\b", regex::escape(name)))
                .expect("group-mention regex compiles"),
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Extraction
// ---------------------------------------------------------------------------

/// One enumeration of subcommands claimed by one documentation line.
#[derive(Debug, Clone)]
struct ClaimedList {
    file: String,
    line_no: usize,
    line: String,
    group: String,
    /// Every identifier in the qualifying runs on that line, real or not.
    claimed: BTreeSet<String>,
}

/// Matches a run of identifiers joined by list separators: `validate/run/score`,
/// `list, add, and remove`, `validate | run | report`.
///
/// The `regex` crate has no lookaround, so the leading word boundary is
/// *consumed* and the run itself is capture group 1.
///
/// The separator alternation requires punctuation or whitespace before
/// `and`/`or`. Without that guard the engine backtracks inside ordinary words:
/// `for validate / run` tokenises as `f` + separator `or ` + `validate`, which
/// reports a phantom `f` subcommand.
fn run_regex() -> Regex {
    Regex::new(&format!(
        r"(?:^|[^A-Za-z0-9_`/-])({TOKEN}(?:{SEP}{TOKEN})+)"
    ))
    .expect("subcommand-run regex compiles")
}

/// A single identifier, optionally wrapped in the backticks markdown uses for
/// inline code.
const TOKEN: &str = r"`?[a-z][a-z0-9-]*`?";

/// A list separator: `,` `/` `|` (each optionally trailed by "and"/"or"), or a
/// bare " and " / " or ".
const SEP: &str = r"(?:\s*[/,|]\s*(?:(?:and|or)\s+)?|\s+(?:and|or)\s+)";

fn sep_regex() -> Regex {
    Regex::new(SEP).expect("separator regex compiles")
}

/// Split a matched run back into its identifiers.
///
/// It has to split on the *separator*, not on "any non-identifier character":
/// the conjunction in `score, and scorecard` is part of the separator, and
/// character-splitting leaves `and` behind as a phantom subcommand.
fn split_run(run: &str, sep_re: &Regex) -> Vec<String> {
    sep_re
        .split(run)
        .map(|piece| piece.trim().trim_matches('`'))
        .filter(|piece| {
            let mut chars = piece.chars();
            chars.next().is_some_and(|c| c.is_ascii_lowercase())
                && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        })
        .map(str::to_string)
        .collect()
}

/// Every subcommand list claimed on one line, at most one per command group.
///
/// A run qualifies for group `g` only if it names at least two real
/// subcommands of `g`; identifiers from every qualifying run on the line are
/// then merged into a single claim, so a list split across two parentheticals
/// ("...operations (add, remove) and browsing (show, versions)") reads as the
/// one list it is.
fn claimed_lists_in_line(
    file: &str,
    line_no: usize,
    line: &str,
    groups: &[Group],
    run_re: &Regex,
    sep_re: &Regex,
) -> Vec<ClaimedList> {
    let mut out = Vec::new();
    for group in groups {
        if !group.mention.is_match(line) {
            continue;
        }
        let mut claimed: BTreeSet<String> = BTreeSet::new();
        for caps in run_re.captures_iter(line) {
            let tokens = split_run(&caps[1], sep_re);
            if tokens.iter().filter(|t| group.subs.contains(*t)).count() >= 2 {
                claimed.extend(tokens);
            }
        }
        if claimed.is_empty() {
            continue;
        }
        out.push(ClaimedList {
            file: file.to_string(),
            line_no,
            line: line.trim().to_string(),
            group: group.name.clone(),
            claimed,
        });
    }
    out
}

fn scan_text(
    file: &str,
    text: &str,
    groups: &[Group],
    run_re: &Regex,
    sep_re: &Regex,
) -> Vec<ClaimedList> {
    text.lines()
        .enumerate()
        // Git for Windows checks out with core.autocrlf=true, so on a Windows
        // runner every line arrives with a trailing \r; left in place it would
        // end a run one character early.
        .flat_map(|(i, raw)| {
            claimed_lists_in_line(
                file,
                i + 1,
                raw.trim_end_matches('\r'),
                groups,
                run_re,
                sep_re,
            )
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Docs corpus
// ---------------------------------------------------------------------------

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// `README.md` plus every `.md`/`.mdx` under `webdocs/`, as (label, contents).
fn docs_corpus() -> Vec<(String, String)> {
    let root = repo_root();
    let webdocs = root.join("webdocs");
    let mut nested: Vec<PathBuf> = walkdir::WalkDir::new(&webdocs)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
        .map(walkdir::DirEntry::into_path)
        .filter(|p| p.extension().is_some_and(|e| e == "md" || e == "mdx"))
        .collect();
    assert!(
        !nested.is_empty(),
        "no .md/.mdx files found under {} -- did webdocs move?",
        webdocs.display()
    );
    nested.sort();

    let mut files: Vec<PathBuf> = vec![root.join("README.md")];
    files.extend(nested);

    files
        .into_iter()
        .map(|p| {
            let text =
                std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()));
            let label = p
                .strip_prefix(&root)
                .unwrap_or(&p)
                .to_string_lossy()
                .replace('\\', "/");
            (label, text)
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Allowlist
// ---------------------------------------------------------------------------

const ALLOWLIST_TOML: &str = include_str!("spec_docs_parity_allowlist.toml");

#[derive(Debug, Default, serde::Deserialize)]
struct Allowlist {
    #[serde(default)]
    subcommand_list_omission: Vec<AllowedEntry>,
    #[serde(default)]
    subcommand_list_unknown_token: Vec<AllowedEntry>,
}

#[derive(Debug, serde::Deserialize)]
struct AllowedEntry {
    /// "<group> <subcommand>", e.g. `mcp register`.
    command: String,
    /// Documentation-only: read by humans reviewing the allowlist.
    #[serde(default)]
    #[allow(dead_code)]
    reason: String,
}

fn load_allowlist() -> Allowlist {
    toml::from_str(ALLOWLIST_TOML)
        .unwrap_or_else(|e| panic!("spec_docs_parity_allowlist.toml is not valid TOML: {e}"))
}

fn allowed(entries: &[AllowedEntry]) -> BTreeSet<&str> {
    entries.iter().map(|e| e.command.as_str()).collect()
}

// ---------------------------------------------------------------------------
// The gate
// ---------------------------------------------------------------------------

/// One documented list that disagrees with `fastskill spec`.
struct Finding {
    list: ClaimedList,
    missing: Vec<String>,
    unknown: Vec<String>,
}

/// Returns (every list examined, drift findings).
fn scan_corpus(
    corpus: &[(String, String)],
    groups: &[Group],
    allowlist: &Allowlist,
) -> (Vec<ClaimedList>, Vec<Finding>) {
    let run_re = run_regex();
    let sep_re = sep_regex();
    let allowed_missing = allowed(&allowlist.subcommand_list_omission);
    let allowed_unknown = allowed(&allowlist.subcommand_list_unknown_token);

    let mut examined: Vec<ClaimedList> = Vec::new();
    let mut out = Vec::new();
    for (label, text) in corpus {
        for list in scan_text(label, text, groups, &run_re, &sep_re) {
            examined.push(list.clone());
            let subs = &groups
                .iter()
                .find(|g| g.name == list.group)
                .expect("claimed list carries a known group")
                .subs;
            let missing: Vec<String> = subs
                .iter()
                .filter(|s| !list.claimed.contains(*s))
                .map(|s| format!("{} {s}", list.group))
                .filter(|qualified| !allowed_missing.contains(qualified.as_str()))
                .collect();
            let unknown: Vec<String> = list
                .claimed
                .iter()
                .filter(|t| !subs.contains(*t))
                .map(|t| format!("{} {t}", list.group))
                .filter(|qualified| !allowed_unknown.contains(qualified.as_str()))
                .collect();
            if !missing.is_empty() || !unknown.is_empty() {
                out.push(Finding {
                    list,
                    missing,
                    unknown,
                });
            }
        }
    }
    (examined, out)
}

fn report(findings: &[Finding]) -> String {
    let mut msg = format!(
        "\n{} documented subcommand list(s) disagree with `fastskill spec --format json`:\n\n",
        findings.len()
    );
    for f in findings {
        msg.push_str(&format!(
            "  {}:{}  (`fastskill {}`)\n    {}\n",
            f.list.file, f.list.line_no, f.list.group, f.list.line
        ));
        if !f.missing.is_empty() {
            msg.push_str(&format!(
                "    real, but missing from the list: {}\n",
                f.missing.join(", ")
            ));
        }
        if !f.unknown.is_empty() {
            msg.push_str(&format!(
                "    listed, but not a real subcommand: {}\n",
                f.unknown.join(", ")
            ));
        }
        msg.push('\n');
    }
    msg.push_str(
        "Update the documented list to match the CLI. If a divergence is deliberate, add \
         an entry to crates/fastskill-cli/tests/spec_docs_parity_allowlist.toml under \
         [[subcommand_list_omission]] (or [[subcommand_list_unknown_token]]) giving \
         `command = \"<group> <subcommand>\"` and a `reason`.\n",
    );
    msg
}

#[test]
fn documented_subcommand_lists_match_the_cli() {
    let groups = compile_groups(&cli_groups());
    let corpus = docs_corpus();
    let allowlist = load_allowlist();

    let (examined, findings) = scan_corpus(&corpus, &groups, &allowlist);

    // A checker that finds nothing to check is not checking anything, and a
    // silently-empty scan is the failure mode this whole file exists to
    // prevent. Pin the floor at the README command-reference table, which is
    // where the drift that motivated this test actually lived: if a rewrite
    // stops those rows being recognised as lists, that is a change to make
    // deliberately, by editing the set below.
    let readme_groups: BTreeSet<&str> = examined
        .iter()
        .filter(|l| l.file == "README.md")
        .map(|l| l.group.as_str())
        .collect();
    for group in ["analyze", "eval", "mcp", "optimize", "repos"] {
        assert!(
            readme_groups.contains(group),
            "no `fastskill {group}` subcommand list was found in README.md; extraction is \
             broken, or the command-reference table was restructured. Groups seen: \
             {readme_groups:?}"
        );
    }

    assert!(findings.is_empty(), "{}", report(&findings));
}

// ---------------------------------------------------------------------------
// Negative check: the scanner must fire on the drift it was written for.
// ---------------------------------------------------------------------------

/// The four command-table rows this test was written against, copied verbatim
/// from `README.md` at 2f5df14 -- the tip of the branch before the docs fix.
/// `eval` had gained `judge` and `scorecard` and `mcp` had gained `register`,
/// and none of the three appeared in the documented lists.
///
/// Held as a fixture rather than shelled out to `git show`: the assertion is
/// about the scanner, and it must hold in a source tarball with no git history.
const PRE_FIX_README_ROWS: &str = concat!(
    "| `fastskill repos <cmd>` | Manage repositories & browse catalogs ",
    "(`list/add/remove/info/update/test/refresh/skills/show/versions`) |\n",
    "| `fastskill eval <cmd>` | Skill quality evals (`validate/run/report/score`) |\n",
    "| `fastskill optimize <cmd>` | Text-gradient skill optimization ",
    "(`run/resume/status/inspect/export`) |\n",
    "| `fastskill mcp <cmd>` | Run/install the MCP server (`serve/install/list`) for agents |\n",
);

#[test]
fn checker_fires_on_the_pre_fix_readme() {
    let groups = compile_groups(&cli_groups());
    let corpus = vec![(
        "README.md@2f5df14".to_string(),
        PRE_FIX_README_ROWS.to_string(),
    )];

    // Deliberately no allowlist: this asserts the *detector* fires, separately
    // from the policy layer that afterwards forgives `mcp register`.
    let (examined, findings) = scan_corpus(&corpus, &groups, &Allowlist::default());

    assert_eq!(
        examined.len(),
        4,
        "expected exactly one claimed list per fixture row, found {}",
        examined.len()
    );

    let missing: BTreeSet<String> = findings
        .iter()
        .flat_map(|f| f.missing.iter().cloned())
        .collect();
    let expected: BTreeSet<String> = ["eval judge", "eval scorecard", "mcp register"]
        .into_iter()
        .map(str::to_string)
        .collect();
    assert_eq!(
        missing, expected,
        "the pre-fix README must be reported as missing exactly judge, scorecard and register"
    );

    // ...and no collateral noise: the repos and optimize rows were already
    // complete, and nothing in the fixture names a non-existent subcommand.
    let unknown: Vec<&String> = findings.iter().flat_map(|f| f.unknown.iter()).collect();
    assert!(unknown.is_empty(), "unexpected unknown tokens: {unknown:?}");
    let flagged: BTreeSet<&str> = findings.iter().map(|f| f.list.group.as_str()).collect();
    assert_eq!(
        flagged,
        BTreeSet::from(["eval", "mcp"]),
        "only the eval and mcp rows were stale in the pre-fix README"
    );
}

// ---------------------------------------------------------------------------
// Extraction unit tests: what it must catch, and what it must ignore.
// ---------------------------------------------------------------------------

#[test]
fn scanner_extracts_lists_and_ignores_prose() {
    let run_re = run_regex();
    let sep_re = sep_regex();
    let groups = compile_groups(&BTreeMap::from([
        (
            "eval".to_string(),
            ["validate", "run", "judge", "report", "score", "scorecard"]
                .into_iter()
                .map(str::to_string)
                .collect::<BTreeSet<_>>(),
        ),
        (
            "mcp".to_string(),
            ["serve", "install", "list"]
                .into_iter()
                .map(str::to_string)
                .collect::<BTreeSet<_>>(),
        ),
    ]));
    let claimed = |line: &str| -> Option<Vec<String>> {
        claimed_lists_in_line("f", 1, line, &groups, &run_re, &sep_re)
            .into_iter()
            .next()
            .map(|l| l.claimed.into_iter().collect())
    };
    let all_six: Vec<String> = ["judge", "report", "run", "score", "scorecard", "validate"]
        .into_iter()
        .map(str::to_string)
        .collect();

    // Slash-, pipe-, comma- and "and"-separated forms all read the same.
    for line in [
        "| `fastskill eval <cmd>` | Skill quality evals (`validate/run/judge/report/score/scorecard`) |",
        "`fastskill eval validate | run | judge | report | score | scorecard` covers a suite",
        "`fastskill eval` runs validate, run, judge, report, score, and scorecard",
        "Use `fastskill eval` for validate / run / judge / report / score / scorecard.",
    ] {
        assert_eq!(claimed(line).as_ref(), Some(&all_six), "failed on: {line}");
    }

    // "for validate / run": the separator must not split the ordinary word
    // `for` into `f` + `or `, which used to yield a phantom `f` subcommand.
    assert!(
        !claimed("Use `fastskill eval` for validate / run")
            .expect("two real names is a list")
            .contains(&"f".to_string()),
        "tokenisation split an ordinary word around `or`"
    );

    // A run naming only one real subcommand is not a list.
    assert_eq!(
        claimed("| `fastskill mcp <cmd>` | Run/install the MCP server for agents |"),
        None
    );
    // Group named, but nothing enumerated.
    assert_eq!(
        claimed("Start it with `fastskill mcp serve --transport stdio`."),
        None
    );
    // An enumeration, but no group named on the line.
    assert_eq!(
        claimed("The tools are validate/run/judge/report/score."),
        None
    );
    // Markdown table pipes must not join words across cells into a claim.
    assert_eq!(
        claimed("| Re-score a prior run | `fastskill eval score --run-dir <dir>` |"),
        None
    );

    // Names that are not real subcommands are surfaced, not silently dropped.
    assert!(
        claimed("`fastskill mcp` exposes serve/install/list/publish")
            .expect("three real names is a list")
            .contains(&"publish".to_string())
    );
}
