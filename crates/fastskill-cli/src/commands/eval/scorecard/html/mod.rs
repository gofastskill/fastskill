//! The static HTML report (spec eval-scorecard-report R5, R6).
//!
//! One file, no network, no script. The run directories a scorecard describes
//! are scratch space and get deleted; this file is opened months later, often
//! from a machine that never saw them. So everything it needs — fonts, styles,
//! and the source JSON itself — is inside it, and the only `<a href>` it emits
//! points at a run directory that still exists on the machine rendering it.
//!
//! The one `<script>` is `type="application/json"` and carries the scorecards
//! verbatim, so the report is its own data file: a reader with a text editor
//! can recover the JSON that produced every number above it.

mod cases;
mod chart;
mod style;

use super::document::Scorecard;
use super::ScorecardArgs;
use crate::error::{CliError, CliResult};
use serde_json::Value;
use std::fmt::Write as _;
use std::path::Path;

/// How much of a criterion's reasoning is shown before the fold (R6).
pub const REASONING_PREVIEW: usize = 600;

pub struct HtmlOptions {
    /// Strip reasoning from the tables *and* from the embedded JSON. A report
    /// that hides reasoning must not ship it somewhere the reader cannot see.
    pub no_reasoning: bool,
    /// Whether a run directory may become a link. False when rendering from
    /// `--from`: those paths were written on another machine, and R6 says that
    /// mode touches no run directory — including to ask whether one exists.
    pub link_run_dirs: bool,
}

impl HtmlOptions {
    fn from_args(args: &ScorecardArgs) -> Self {
        HtmlOptions {
            no_reasoning: args.no_reasoning,
            link_run_dirs: args.from.is_empty(),
        }
    }
}

/// `--format html --from a.json [--from b.json …]` (R5).
pub fn render_from_files(args: &ScorecardArgs) -> CliResult<()> {
    if !args.html {
        return Err(CliError::Config(
            "Error: --from renders an HTML report; pass --format html.".to_string(),
        ));
    }
    if args.root.is_some() || args.metrics.is_some() {
        return Err(CliError::Config(
            "Error: --from renders scorecards that are already computed, so it cannot be \
             combined with --root or --metrics."
                .to_string(),
        ));
    }

    let mut cards = Vec::with_capacity(args.from.len());
    for path in &args.from {
        let text = std::fs::read_to_string(path).map_err(|e| {
            CliError::Config(format!(
                "EVAL_SCORECARD_UNREADABLE: cannot read '{}': {}",
                path.display(),
                e
            ))
        })?;
        let card: Scorecard = serde_json::from_str(&text).map_err(|e| {
            CliError::Config(format!(
                "EVAL_SCORECARD_UNREADABLE: '{}' is not a {} document: {}",
                path.display(),
                super::document::SCORECARD_SCHEMA,
                e
            ))
        })?;
        cards.push((path.clone(), card));
    }

    comparable(&cards)?;
    if !args.allow_mixed_judges {
        one_judge_identity(&cards)?;
    }

    // Oldest first: a progress chart reads left to right in time, whatever
    // order the files were named in.
    cards.sort_by(|a, b| a.1.generated_at.cmp(&b.1.generated_at));
    let cards: Vec<Scorecard> = cards.into_iter().map(|(_, c)| c).collect();
    write_report(args, &cards)
}

/// Every input must carry the same non-null `benchmark.sha256` (R5). There is
/// no override: two benchmarks are two questions, and there is no honest way
/// to draw them on one axis.
fn comparable(cards: &[(std::path::PathBuf, Scorecard)]) -> CliResult<()> {
    let unhashed: Vec<String> = cards
        .iter()
        .filter(|(_, c)| c.benchmark.sha256.is_none())
        .map(|(p, _)| p.display().to_string())
        .collect();
    if !unhashed.is_empty() {
        return Err(CliError::Config(format!(
            "EVAL_SCORECARD_NO_BENCHMARK_HASH: {} scorecard(s) carry no benchmark.sha256, so \
             there is nothing to compare them by: {}. Declare `suites` in the metrics file and \
             regenerate them.",
            unhashed.len(),
            unhashed.join(", ")
        )));
    }
    let mut hashes: Vec<(&str, &str)> = Vec::new();
    for (path, card) in cards {
        let Some(hash) = card.benchmark.sha256.as_deref() else {
            continue;
        };
        if !hashes.iter().any(|(h, _)| *h == hash) {
            hashes.push((hash, path.to_str().unwrap_or("?")));
        }
    }
    if hashes.len() > 1 {
        let named: Vec<String> = hashes
            .iter()
            .map(|(h, p)| format!("{} ({})", &h[..h.len().min(12)], p))
            .collect();
        return Err(CliError::Config(format!(
            "EVAL_SCORECARD_BENCHMARK_MISMATCH: the {} scorecards were produced by {} different \
             benchmarks: {}. Two benchmarks are two questions; there is no override.",
            cards.len(),
            hashes.len(),
            named.join(", ")
        )));
    }
    Ok(())
}

/// R4's mixed-judges guard, applied across scorecards rather than within one.
fn one_judge_identity(cards: &[(std::path::PathBuf, Scorecard)]) -> CliResult<()> {
    let mut hashes: Vec<String> = Vec::new();
    for (_, card) in cards {
        for judge in &card.judges {
            if !hashes.contains(&judge.judge_hash) {
                hashes.push(judge.judge_hash.clone());
            }
        }
    }
    if hashes.len() > 1 {
        return Err(CliError::Config(format!(
            "EVAL_SCORECARD_MIXED_JUDGES: the scorecards carry {} judge identities: {}. \
             Re-judge with one identity, or pass --allow-mixed-judges.",
            hashes.len(),
            hashes.join(", ")
        )));
    }
    Ok(())
}

/// Render and put the file where `-o` asked, or on stdout.
pub fn write_report(args: &ScorecardArgs, cards: &[Scorecard]) -> CliResult<()> {
    let page = render(cards, &HtmlOptions::from_args(args))?;
    match &args.output {
        Some(path) => {
            std::fs::write(path, page.as_bytes()).map_err(|e| {
                CliError::Config(format!(
                    "EVAL_SCORECARD_WRITE: cannot write '{}': {}",
                    path.display(),
                    e
                ))
            })?;
            crate::outln!("Wrote {}", path.display());
        }
        None => crate::outln!("{}", page),
    }
    Ok(())
}

/// Escape text for element content and double-quoted attribute values alike.
pub fn esc(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 8);
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

/// `Some(x)` as text, or an em dash. Absent is not zero and must not read as it.
pub fn or_dash(value: Option<String>) -> String {
    value.unwrap_or_else(|| "—".to_string())
}

pub fn pct(value: f64) -> String {
    format!("{:.1}%", value * 100.0)
}

fn verdict_pill(verdict: &str) -> String {
    let class = match verdict {
        "PASS" => "pass",
        "FAIL" => "fail",
        _ => "mute",
    };
    format!("<span class=\"pill {}\">{}</span>", class, esc(verdict))
}

/// The report's title: what was measured, not "Report".
fn title(card: &Scorecard) -> String {
    match (&card.agent, &card.model) {
        (Some(agent), Some(model)) => format!("{} · {}", agent, model),
        (Some(agent), None) => agent.clone(),
        _ => format!("{} targets", card.targets.len()),
    }
}

/// Render one or more scorecards. The last is the current one every section
/// but progress describes; progress (R5) needs at least two.
pub fn render(cards: &[Scorecard], opts: &HtmlOptions) -> CliResult<String> {
    let card = cards.last().ok_or_else(|| {
        CliError::Config("EVAL_SCORECARD_NO_INPUT: no scorecard to render".to_string())
    })?;

    let mut h = String::with_capacity(512 * 1024);
    h.push_str("<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n");
    h.push_str("<meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\n");
    let _ = writeln!(h, "<title>{} — eval scorecard</title>", esc(&title(card)));
    let _ = writeln!(h, "<style>{}</style>", style::stylesheet());
    h.push_str("</head>\n<body>\n<main>\n");

    let _ = writeln!(
        h,
        "<header class=\"report\"><h1>{}</h1>\
         <div class=\"sub\">Eval scorecard · generated {}</div></header>",
        esc(&title(card)),
        esc(&card.generated_at)
    );

    identity(&mut h, card, opts);
    gates(&mut h, card);
    notes(&mut h, card);
    judges(&mut h, card);
    cases::section(&mut h, card, opts);
    if cards.len() > 1 {
        chart::progress(&mut h, cards);
    }
    embedded_json(&mut h, cards, opts)?;
    licence(&mut h);

    h.push_str("</main>\n</body>\n</html>\n");
    Ok(h)
}

fn identity(h: &mut String, card: &Scorecard, opts: &HtmlOptions) {
    h.push_str("<section><h2>Identity</h2><dl class=\"identity\">");
    let targets = card
        .targets
        .iter()
        .map(|t| {
            format!(
                "{}/{} ({} run{})",
                t.agent,
                t.model.clone().unwrap_or_else(|| "—".into()),
                t.runs,
                if t.runs == 1 { "" } else { "s" }
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    row(h, "Target", &targets);
    row(
        h,
        "Skill",
        &or_dash(card.skill.path.as_ref().map(|p| p.display().to_string())),
    );
    let revision = match (&card.skill.git_sha, card.skill.dirty) {
        (Some(sha), Some(true)) => format!("{sha} (dirty)"),
        (Some(sha), _) => sha.clone(),
        // A run that predates the fields, or runs that disagree. Never a value
        // resolved from the working tree now: that is a different skill.
        (None, _) => "not recorded".to_string(),
    };
    row(h, "Skill revision", &revision);
    row(h, "Benchmark", &card.benchmark.path.display().to_string());
    row(
        h,
        "Benchmark hash",
        &card
            .benchmark
            .sha256
            .clone()
            .unwrap_or_else(|| "none — no suites declared, so this card is not comparable".into()),
    );
    row(h, "fastskill", &card.fastskill_version);
    row(h, "aikit-evals", &card.aikit_evals_version);
    h.push_str("</dl>");

    h.push_str(
        "<h3 style=\"margin-top:1.2rem\">Runs</h3><div class=\"scroll\"><table><thead><tr>\
                <th>Started</th><th>Agent</th><th>Model</th><th>Directory</th>\
                </tr></thead><tbody>",
    );
    for run in &card.runs {
        let _ = write!(
            h,
            "<tr><td class=\"mono\">{}</td><td>{}</td><td>{}</td><td class=\"mono\">{}</td></tr>",
            esc(run.started_at.as_deref().unwrap_or("—")),
            esc(&run.agent),
            esc(run.model.as_deref().unwrap_or("—")),
            run_dir_cell(&run.run_dir, opts)
        );
    }
    h.push_str("</tbody></table></div></section>\n");
}

/// A run directory is a link only when it is still there. A link to a deleted
/// scratch directory is worse than none (R6).
pub fn run_dir_cell(dir: &Path, opts: &HtmlOptions) -> String {
    let shown = esc(&dir.display().to_string());
    if opts.link_run_dirs && dir.is_dir() {
        format!("<a href=\"{}\">{}</a>", shown, shown)
    } else {
        shown
    }
}

fn row(h: &mut String, key: &str, value: &str) {
    let _ = write!(h, "<dt>{}</dt><dd>{}</dd>", esc(key), esc(value));
}

fn gates(h: &mut String, card: &Scorecard) {
    h.push_str(
        "<section><h2>Gates</h2><div class=\"scroll\"><table><thead><tr>\
         <th>Metric</th><th class=\"num\">Value</th><th class=\"num\">Observed</th>\
         <th class=\"num\">Cases</th><th>Threshold</th><th>Verdict</th><th>Flags</th>\
         </tr></thead><tbody>",
    );
    for m in &card.metrics {
        let value = match (m.rate, m.score, m.p95_tool_calls) {
            (Some(rate), _, _) => pct(rate),
            (_, Some(score), _) => format!("{score:.3}"),
            (_, _, Some(p95)) => p95.to_string(),
            _ => "—".to_string(),
        };
        let mut flags = Vec::new();
        if m.mixed_judges {
            flags.push("<span class=\"pill warn\">MIXED JUDGES</span>");
        }
        if m.mixed_targets {
            flags.push("<span class=\"pill warn\">MIXED TARGETS</span>");
        }
        let _ = write!(
            h,
            "<tr><td>{}</td><td class=\"num\">{}</td><td class=\"num\">{}</td>\
             <td class=\"num\">{}</td><td class=\"mono\">{}</td><td>{}</td><td>{}</td></tr>",
            esc(&m.name),
            esc(&value),
            m.observed,
            m.cases,
            esc(&m.threshold),
            verdict_pill(&m.verdict),
            flags.join(" ")
        );
    }
    if card.metrics.is_empty() {
        h.push_str("<tr><td colspan=\"7\" class=\"empty\">no metrics declared</td></tr>");
    }
    h.push_str("</tbody></table></div></section>\n");
}

fn notes(h: &mut String, card: &Scorecard) {
    let t = &card.totals;
    let mut items: Vec<String> = Vec::new();

    if !card.unclaimed_checks.is_empty() {
        items.push(format!(
            "<strong>{} check(s) no metric claims:</strong> <span class=\"mono\">{}</span>. \
             A check that runs on every trial and is reported nowhere is an assertion nobody reads.",
            card.unclaimed_checks.len(),
            esc(&card.unclaimed_checks.join(", "))
        ));
    }
    if !t.unmeasured_cases.is_empty() {
        items.push(format!(
            "<strong>{} case(s) carry no measurement</strong> — every trial errored: \
             <span class=\"mono\">{}</span>. They are excluded from every rate, because an \
             outage is not restraint.",
            t.unmeasured_cases.len(),
            esc(&t.unmeasured_cases.join(", "))
        ));
    }
    if t.error_trials > 0 {
        items.push(format!(
            "<strong>{} errored trial(s)</strong> excluded before anything was folded.",
            t.error_trials
        ));
    }
    if t.not_observable_checks > 0 {
        items.push(format!(
            "<strong>{} check result(s) were not observable</strong> — the backend could not \
             produce the evidence, so they are excluded rather than failed.",
            t.not_observable_checks
        ));
    }
    if t.judge_errors > 0 {
        items.push(format!(
            "<strong>{} judgment(s) recorded an error</strong> instead of scores. \
             They contribute nothing; an unreachable judge is not a zero.",
            t.judge_errors
        ));
    }
    if t.judge_excluded_trials > 0 {
        items.push(format!(
            "<strong>{} trial(s) a gated judge could not judge</strong>, excluded exactly \
             like errors.",
            t.judge_excluded_trials
        ));
    }
    if t.trials_without_cost > 0 {
        items.push(format!(
            "<strong>{} trial(s) reported no cost.</strong> Cost below is what vendors \
             reported, not an estimate over the rest.",
            t.trials_without_cost
        ));
    }
    let overrides: Vec<&str> = [
        card.metrics
            .iter()
            .any(|m| m.mixed_judges)
            .then_some("--allow-mixed-judges"),
        card.metrics
            .iter()
            .any(|m| m.mixed_targets)
            .then_some("--allow-mixed-targets"),
        (card.cases.len() > distinct_case_ids(card)).then_some("--allow-duplicate-cases"),
    ]
    .into_iter()
    .flatten()
    .collect();
    if !overrides.is_empty() {
        items.push(format!(
            "<strong>Overrides in force:</strong> <span class=\"mono\">{}</span>. \
             Each one names a comparison this card makes anyway; read the numbers with it in mind.",
            esc(&overrides.join(" "))
        ));
    }

    h.push_str("<section><h2>Notes</h2>");
    if items.is_empty() {
        h.push_str("<p class=\"empty\">Nothing excluded, nothing unclaimed, no override used.</p>");
    } else {
        h.push_str("<ul class=\"notes\">");
        for item in items {
            let _ = write!(h, "<li>{item}</li>");
        }
        h.push_str("</ul>");
    }

    let _ = writeln!(
        h,
        "<div class=\"scroll\" style=\"margin-top:1rem\"><table><thead><tr>\
         <th>Runs</th><th>Cases</th><th>Trials</th><th>Scored</th><th>Errored</th><th>Cost (USD)</th>\
         </tr></thead><tbody><tr>\
         <td class=\"num\">{}</td><td class=\"num\">{}</td><td class=\"num\">{}</td>\
         <td class=\"num\">{}</td><td class=\"num\">{}</td><td class=\"num\">{}</td>\
         </tr></tbody></table></div></section>",
        t.runs,
        t.cases,
        t.trials,
        t.scored_trials,
        t.error_trials,
        or_dash(t.cost_usd.map(|c| format!("{c:.4}")))
    );
}

fn distinct_case_ids(card: &Scorecard) -> usize {
    let mut ids: Vec<&str> = card.cases.iter().map(|c| c.case_id.as_str()).collect();
    ids.sort_unstable();
    ids.dedup();
    ids.len()
}

fn judges(h: &mut String, card: &Scorecard) {
    h.push_str("<section><h2>Judges</h2>");
    if card.judges.is_empty() {
        h.push_str("<p class=\"empty\">No judgment contributed to this scorecard.</p></section>\n");
        return;
    }
    h.push_str(
        "<div class=\"scroll\"><table><thead><tr>\
         <th>Judge</th><th>Hash</th><th>Model</th><th>Endpoint</th>\
         <th class=\"num\">Temp</th><th class=\"num\">Max tokens</th>\
         </tr></thead><tbody>",
    );
    for j in &card.judges {
        let _ = write!(
            h,
            "<tr><td>{}</td><td class=\"mono\">{}</td><td class=\"mono\">{}</td>\
             <td class=\"mono\">{}</td><td class=\"num\">{}</td><td class=\"num\">{}</td></tr>",
            esc(&j.name),
            esc(&j.judge_hash),
            esc(&j.identity.model),
            esc(&j.identity.endpoint_host),
            j.identity.temperature,
            j.identity.max_tokens
        );
    }
    let tok = &card.totals.judge_tokens;
    let _ = writeln!(
        h,
        "</tbody></table></div>\
         <p style=\"margin-top:.9rem;font-size:.82rem;color:var(--ink-soft)\">\
         Tokens across every judgment attempt, superseded retries included: \
         <span class=\"mono\">{} in · {} out · {} total</span>.</p></section>",
        tok.input, tok.output, tok.total
    );
}

/// The source JSON, verbatim, as the report's own data file (R6).
fn embedded_json(h: &mut String, cards: &[Scorecard], opts: &HtmlOptions) -> CliResult<()> {
    let mut value = serde_json::to_value(cards).map_err(|e| {
        CliError::Config(format!(
            "EVAL_SCORECARD_RENDER: cannot embed scorecard: {e}"
        ))
    })?;
    if opts.no_reasoning {
        strip_reasoning(&mut value);
    }
    let text = serde_json::to_string(&value).map_err(|e| {
        CliError::Config(format!(
            "EVAL_SCORECARD_RENDER: cannot embed scorecard: {e}"
        ))
    })?;
    // `</script` inside a data block ends the element, whatever the type is.
    let safe = text.replace('<', "\\u003c");
    let _ = writeln!(
        h,
        "<script type=\"application/json\" id=\"scorecards\">{safe}</script>"
    );
    Ok(())
}

/// Remove every `reasoning` field, wherever it sits. `--no-reasoning` is a
/// statement about the file, not about the tables in it.
fn strip_reasoning(value: &mut Value) {
    match value {
        Value::Object(map) => {
            map.remove("reasoning");
            for v in map.values_mut() {
                strip_reasoning(v);
            }
        }
        Value::Array(items) => {
            for v in items {
                strip_reasoning(v);
            }
        }
        _ => {}
    }
}

fn licence(h: &mut String) {
    let _ = writeln!(
        h,
        "<footer><p>Set in IBM Plex, embedded in this file under the SIL Open Font \
         License 1.1.</p><details><summary>Licence text</summary><pre>{}</pre></details></footer>",
        esc(style::OFL)
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts(link_run_dirs: bool) -> HtmlOptions {
        HtmlOptions {
            no_reasoning: false,
            link_run_dirs,
        }
    }

    /// R6: a link to a run directory that is no longer there is worse than no
    /// link — it tells the reader the evidence is one click away when it is
    /// gone. Run directories are scratch space, so this is the normal case for
    /// any report more than a few days old.
    #[test]
    fn a_run_directory_is_linked_only_while_it_is_still_there() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().to_path_buf();

        let linked = run_dir_cell(&path, &opts(true));
        assert!(
            linked.starts_with("<a href=\""),
            "an existing directory is reachable: {linked}"
        );

        // The same report, rendered from `--from`: those paths were written on
        // another machine, and R6 says that mode touches no run directory.
        assert!(
            !run_dir_cell(&path, &opts(false)).contains("<a "),
            "a --from render links nothing"
        );

        drop(dir);
        let gone = run_dir_cell(&path, &opts(true));
        assert!(
            !gone.contains("<a "),
            "a deleted directory is text, not a link: {gone}"
        );
        assert!(
            gone.contains(path.file_name().and_then(|n| n.to_str()).unwrap_or("?")),
            "the path is still shown, so the reader knows what was deleted: {gone}"
        );
    }
}
