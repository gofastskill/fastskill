#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! Deterministic "spec-vs-docs parity" test. No LLM, no network.
//!
//! Catches documentation drift between `webdocs/cli-reference/*.mdx` and the
//! real CLI command surface, in both directions:
//!
//! 1. `documented_commands_exist_in_cli` (hard gate): every `fastskill ...`
//!    invocation found in a fenced ` ```bash ` block in the docs must resolve
//!    to a real command in `fastskill spec --format json`.
//! 2. `cli_commands_are_documented` (separate, independently allowlistable):
//!    every real CLI command path must appear somewhere in
//!    `webdocs/cli-reference/*.mdx`.
//!
//! ## Source of CLI truth
//!
//! `fastskill-cli` has no `[lib]` target (it's bin-only), so the command tree
//! cannot be built in-process from this integration test. Instead this test
//! shells out to the compiled test binary via `CARGO_BIN_EXE_fastskill` and
//! parses `fastskill spec --format json` -- the same pattern used by
//! `mcp_stdio_protocol_test.rs` in this directory. `spec` is a built-in
//! cli-framework command that walks the live `CommandRegistry`, so this is as
//! close to "the real command surface" as we can get without a library target.
//!
//! ## Source of docs truth
//!
//! Only fenced ` ```bash ` blocks in `webdocs/cli-reference/*.mdx` are
//! parsed (not prose, not headings, not plain/`json`/`toml` example-output
//! blocks). Extraction is deliberately conservative: it is far better to miss
//! a real drift than to invent one, because false positives get this test
//! disabled. See `extract_documented_commands` for the exact tokenization
//! rules.
//!
//! ## Allowlist
//!
//! Known, investigated drift lives in `spec_docs_parity_allowlist.toml`
//! next to this file, each entry with a `reason`. Entries are skipped;
//! the test hard-fails only on drift that isn't allowlisted.

use regex::Regex;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Command path separator used by `fastskill spec`'s JSON output (`"repos/add"`).
const SPEC_PATH_SEP: char = '/';

// ---------------------------------------------------------------------------
// CLI truth: `fastskill spec --format json`
// ---------------------------------------------------------------------------

/// Run `fastskill spec --format json` against the just-built test binary and
/// return the set of command paths, space-separated (e.g. `"repos add"`,
/// `"mcp install"`, `"doctor"`).
fn cli_command_paths() -> BTreeSet<String> {
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

    doc["commands"]
        .as_array()
        .expect("`commands` array in `fastskill spec` JSON output")
        .iter()
        .map(|c| {
            c["path"]
                .as_str()
                .expect("command `path` string in `fastskill spec` JSON output")
                .replace(SPEC_PATH_SEP, " ")
        })
        .collect()
}

/// Top-level tokens that prefix at least one multi-word command path (i.e.
/// real command *groups*, like `repos` or `mcp`).
fn group_prefixes(paths: &BTreeSet<String>) -> BTreeSet<String> {
    paths
        .iter()
        .filter_map(|p| p.split_once(' ').map(|(head, _)| head.to_string()))
        .collect()
}

// ---------------------------------------------------------------------------
// Docs truth: webdocs/cli-reference/*.mdx
// ---------------------------------------------------------------------------

fn webdocs_cli_reference_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../webdocs/cli-reference")
}

/// Second token of a `fastskill <tok1> <tok2> ...` invocation, classified for
/// how confidently it names a real subcommand.
#[derive(Debug, Clone)]
enum Tok2 {
    /// No second token (`fastskill doctor`).
    None,
    /// A bare lowercase identifier -- looks like a real subcommand name
    /// (`fastskill repos add`).
    Ident(String),
    /// A usage-template placeholder (`fastskill eval <SUBCOMMAND>`,
    /// `fastskill repos <SUBCOMMAND>`) -- the doc is asserting "this is a
    /// command group", not naming a specific subcommand.
    Placeholder,
    /// Anything else (a flag, a path, a URL, a quoted arg, ...) -- not
    /// informative for command-path extraction.
    Other,
}

/// One `fastskill ...` invocation candidate extracted from a fenced ` ```bash `
/// block in a webdocs/cli-reference/*.mdx file.
#[derive(Debug, Clone)]
struct DocCommand {
    file: String,
    line: String,
    tok1: String,
    tok2: Tok2,
}

impl DocCommand {
    /// The command path this line is claiming exists, for reporting and for
    /// matching against the allowlist. Uses the two-token form when the
    /// second token looks like a real subcommand name, otherwise just the
    /// first token.
    fn claimed_path(&self) -> String {
        match &self.tok2 {
            Tok2::Ident(tok2) => format!("{} {tok2}", self.tok1),
            Tok2::None | Tok2::Placeholder | Tok2::Other => self.tok1.clone(),
        }
    }

    /// Whether this invocation resolves to a real CLI command path.
    fn is_documented(&self, leaf_paths: &BTreeSet<String>, groups: &BTreeSet<String>) -> bool {
        if leaf_paths.contains(&self.tok1) {
            return true;
        }
        match &self.tok2 {
            Tok2::Ident(tok2) => leaf_paths.contains(&format!("{} {tok2}", self.tok1)),
            // "fastskill eval <SUBCOMMAND>" / "fastskill repos <SUBCOMMAND>":
            // the doc names a group, not a specific subcommand, so it's
            // enough for `tok1` to be a real group.
            Tok2::Placeholder => groups.contains(&self.tok1),
            Tok2::None | Tok2::Other => false,
        }
    }
}

/// A bare identifier: lowercase ASCII letters/digits/hyphens only, starting
/// with a letter. Rejects flags (`--json`), placeholders (`<ID>`), paths
/// (`./skill`), URLs, quoted strings, and env/shell syntax (`$VAR`) -- i.e.
/// everything that isn't confidently a subcommand name.
fn is_ident(tok: &str) -> bool {
    let mut chars = tok.chars();
    match chars.next() {
        Some(c) if c.is_ascii_lowercase() => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

fn is_placeholder(tok: &str) -> bool {
    tok.starts_with('<') || tok.starts_with('[')
}

/// Yield the contents of every ` ```bash ` ... ` ``` ` fenced block in `text`.
/// Other fence languages (`toml`, `json`, plain example-output blocks) are
/// deliberately excluded: they routinely contain the literal word
/// `fastskill` in prose/output that is not an invocation (e.g.
/// `Installation: fastskill add acme/web-scraper` inside a plain output
/// block), and including them would make extraction noisy.
fn bash_fences(text: &str) -> Vec<&str> {
    const OPEN: &str = "```bash";
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find(OPEN) {
        let after_tag = &rest[start + OPEN.len()..];
        // The tag must end the line, so ```bashful is not a bash fence. Accept a
        // CRLF as well as a bare LF: Git for Windows checks out with
        // core.autocrlf=true by default, so on a Windows runner every one of
        // these files arrives with \r\n and an LF-only match finds nothing --
        // which silently yielded zero commands rather than a real failure.
        let after = match after_tag
            .strip_prefix("\r\n")
            .or_else(|| after_tag.strip_prefix('\n'))
        {
            Some(after) => after,
            None => {
                rest = after_tag;
                continue;
            }
        };
        let Some(end) = after.find("```") else {
            break;
        };
        out.push(&after[..end]);
        rest = &after[end + 3..];
    }
    out
}

/// Extract every `fastskill ...` invocation candidate from every
/// ` ```bash ` fenced block in `webdocs/cli-reference/*.mdx`.
///
/// A line is only considered if, after stripping an optional shell-prompt
/// `$ ` marker, it begins with the literal token `fastskill` (word-bounded,
/// so `fastskill.io` doesn't match). Only the first one or two
/// whitespace-separated tokens after `fastskill` are used to form a command
/// path candidate; see [`Tok2`] for how the second token is classified.
fn extract_documented_commands() -> Vec<DocCommand> {
    let dir = webdocs_cli_reference_dir();
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("read_dir {}: {e}", dir.display()))
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "mdx"))
        .collect();
    paths.sort();
    assert!(
        !paths.is_empty(),
        "no .mdx files found under {} -- did webdocs/cli-reference move?",
        dir.display()
    );

    let mut out = Vec::new();
    for path in paths {
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        let file = path
            .file_name()
            .expect("mdx file has a file name")
            .to_string_lossy()
            .into_owned();

        for block in bash_fences(&text) {
            for raw_line in block.lines() {
                let line = raw_line.trim();
                let line = line.strip_prefix("$ ").unwrap_or(line).trim_start();

                let rest = match line.strip_prefix("fastskill") {
                    Some(r) if r.is_empty() || r.starts_with(char::is_whitespace) => r.trim(),
                    _ => continue,
                };

                let toks: Vec<&str> = rest.split_whitespace().collect();
                let Some(tok1) = toks.first() else {
                    // Bare "fastskill" with no arguments: not a specific
                    // command-path claim, nothing to check.
                    continue;
                };
                if !is_ident(tok1) {
                    continue;
                }

                let tok2 = match toks.get(1) {
                    None => Tok2::None,
                    Some(t) if is_ident(t) => Tok2::Ident((*t).to_string()),
                    Some(t) if is_placeholder(t) => Tok2::Placeholder,
                    Some(_) => Tok2::Other,
                };

                out.push(DocCommand {
                    file: file.clone(),
                    line: raw_line.trim().to_string(),
                    tok1: (*tok1).to_string(),
                    tok2,
                });
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Allowlist
// ---------------------------------------------------------------------------

const ALLOWLIST_TOML: &str = include_str!("spec_docs_parity_allowlist.toml");

#[derive(Debug, Default, serde::Deserialize)]
struct Allowlist {
    #[serde(default)]
    documented_but_missing: Vec<AllowedEntry>,
    #[serde(default)]
    implemented_but_undocumented: Vec<AllowedEntry>,
}

#[derive(Debug, serde::Deserialize)]
struct AllowedEntry {
    command: String,
    #[serde(default)]
    #[allow(dead_code)] // documentation-only field, read by humans not code
    reason: String,
}

fn load_allowlist() -> Allowlist {
    toml::from_str(ALLOWLIST_TOML)
        .unwrap_or_else(|e| panic!("spec_docs_parity_allowlist.toml is not valid TOML: {e}"))
}

fn allowed_commands(entries: &[AllowedEntry]) -> BTreeSet<&str> {
    entries.iter().map(|e| e.command.as_str()).collect()
}

// ---------------------------------------------------------------------------
// Assertion 1 (hard gate): every documented command exists in the CLI.
// ---------------------------------------------------------------------------

#[test]
fn documented_commands_exist_in_cli() {
    let cli_paths = cli_command_paths();
    let groups = group_prefixes(&cli_paths);
    let doc_commands = extract_documented_commands();
    assert!(
        !doc_commands.is_empty(),
        "extracted zero `fastskill ...` invocations from webdocs/cli-reference -- \
         extraction logic is likely broken (regressed the ```bash fence parser?)"
    );

    let allowlist = load_allowlist();
    let allowed = allowed_commands(&allowlist.documented_but_missing);

    // Group offending occurrences by claimed command path so a command
    // documented (wrongly) in five places produces one message, not five.
    let mut offenders: std::collections::BTreeMap<String, Vec<&DocCommand>> = Default::default();
    for doc_cmd in &doc_commands {
        if doc_cmd.is_documented(&cli_paths, &groups) {
            continue;
        }
        let claimed = doc_cmd.claimed_path();
        if allowed.contains(claimed.as_str()) {
            continue;
        }
        offenders.entry(claimed).or_default().push(doc_cmd);
    }

    if offenders.is_empty() {
        return;
    }

    let mut msg = String::new();
    msg.push_str(&format!(
        "\n{} command(s) documented in webdocs/cli-reference/*.mdx do not exist \
         in the CLI surface (`fastskill spec --format json`):\n\n",
        offenders.len()
    ));
    for (command, occurrences) in &offenders {
        msg.push_str(&format!("  `fastskill {command}`\n"));
        for occ in occurrences {
            msg.push_str(&format!("    - {}: `{}`\n", occ.file, occ.line));
        }
    }
    msg.push_str(
        "\nEither fix the doc (the command/flag doesn't exist -- update or remove the \
         example), or, if this is intentional/known drift, add an entry to \
         crates/fastskill-cli/tests/spec_docs_parity_allowlist.toml under \
         [[documented_but_missing]] with a `command` and a `reason`.\n",
    );
    panic!("{msg}");
}

// ---------------------------------------------------------------------------
// Assertion 2 (separately allowlistable): every CLI command is documented.
//
// Kept as its own test function per the task brief: this direction is
// noisier (a command can legitimately be documented outside
// webdocs/cli-reference, e.g. `mcp *` and `optimize *` live under
// webdocs/integration/*.mdx and webdocs/optimize/*.mdx respectively) so it
// should be independently allowlistable/ignorable without weakening
// `documented_commands_exist_in_cli`.
// ---------------------------------------------------------------------------

#[test]
fn cli_commands_are_documented() {
    let cli_paths = cli_command_paths();

    let dir = webdocs_cli_reference_dir();
    let mut mdx_paths: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("read_dir {}: {e}", dir.display()))
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "mdx"))
        .collect();
    mdx_paths.sort();
    assert!(
        !mdx_paths.is_empty(),
        "no .mdx files found under {}",
        dir.display()
    );

    let corpus: String = mdx_paths
        .iter()
        .map(|p| std::fs::read_to_string(p).unwrap_or_else(|e| panic!("read {}: {e}", p.display())))
        .collect::<Vec<_>>()
        .join("\n");

    let allowlist = load_allowlist();
    let allowed = allowed_commands(&allowlist.implemented_but_undocumented);

    let mut undocumented: Vec<&str> = Vec::new();
    for path in &cli_paths {
        if allowed.contains(path.as_str()) {
            continue;
        }
        // Word-bounded, whitespace-tolerant, case-insensitive search for the
        // command path as a phrase (e.g. "mcp install", "doctor") anywhere
        // in the docs -- prose, headings, or code fences all count. This is
        // intentionally lenient (see module docs): a false "documented" is
        // the safe direction of error for a noisy, secondary check.
        let words: Vec<String> = path.split(' ').map(regex::escape).collect();
        let pattern = format!(r"(?i)\b{}\b", words.join(r"\s+"));
        let re = Regex::new(&pattern).unwrap_or_else(|e| panic!("build regex for {path:?}: {e}"));
        if !re.is_match(&corpus) {
            undocumented.push(path);
        }
    }

    if undocumented.is_empty() {
        return;
    }

    let mut msg = String::new();
    msg.push_str(&format!(
        "\n{} CLI command(s) are not mentioned anywhere in webdocs/cli-reference/*.mdx:\n\n",
        undocumented.len()
    ));
    for command in &undocumented {
        msg.push_str(&format!("  `fastskill {command}`\n"));
    }
    msg.push_str(
        "\nEither document the command under webdocs/cli-reference/, or, if it's \
         intentionally documented elsewhere (or intentionally undocumented), add an \
         entry to crates/fastskill-cli/tests/spec_docs_parity_allowlist.toml under \
         [[implemented_but_undocumented]] with a `command` and a `reason`.\n",
    );
    panic!("{msg}");
}

// ---------------------------------------------------------------------------
// Regression: the fence parser must be line-ending agnostic.
// ---------------------------------------------------------------------------

/// `bash_fences` originally matched the literal "```bash\n". Git for Windows
/// checks out with `core.autocrlf=true` by default, so on a Windows runner
/// every `.mdx` file arrives CRLF-terminated and that match found nothing --
/// the parity test then extracted zero commands and failed with "extraction
/// logic is likely broken" rather than reporting a real docs drift.
#[test]
fn bash_fences_is_line_ending_agnostic() {
    let lf = "intro\n```bash\nfastskill list\n```\ntail\n";
    let crlf = "intro\r\n```bash\r\nfastskill list\r\n```\r\ntail\r\n";

    assert_eq!(bash_fences(lf), vec!["fastskill list\n"]);
    assert_eq!(bash_fences(crlf), vec!["fastskill list\r\n"]);

    // The tag must still end the line: ```bashful is not a bash fence.
    assert!(bash_fences("```bashful\nnope\n```\n").is_empty());

    // And an unterminated fence must not panic or loop forever.
    assert!(bash_fences("```bash\nfastskill list\n").is_empty());
}
