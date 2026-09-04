//! The per-case tables (spec eval-scorecard-report R6).
//!
//! "One table per suite" is what the spec asks for, and a case row does not
//! carry a suite. A run is one suite executed against one target, though, and
//! every case row carries its run directory — so the grouping below is by run
//! directory, which is the same partition by a name the document actually has.

use super::super::document::{CaseRow, CheckRow, JudgmentRow, Scorecard};
use super::{esc, or_dash, run_dir_cell, HtmlOptions, REASONING_PREVIEW};
use std::fmt::Write as _;
use std::path::PathBuf;

pub fn section(h: &mut String, card: &Scorecard, opts: &HtmlOptions) {
    h.push_str("<section><h2>Cases</h2>");
    if card.cases.is_empty() {
        h.push_str("<p class=\"empty\">No case survived selection.</p></section>\n");
        return;
    }
    for dir in run_dirs(card) {
        let rows: Vec<&CaseRow> = card.cases.iter().filter(|c| c.run_dir == dir).collect();
        let _ = write!(h, "<h3 class=\"run\">{}</h3>", run_dir_cell(&dir, opts));
        table(h, &rows, opts);
    }
    h.push_str("</section>\n");
}

/// Run directories in the order the cases first mention them, so the tables
/// follow the document rather than an alphabetical order nobody chose.
fn run_dirs(card: &Scorecard) -> Vec<PathBuf> {
    let mut seen: Vec<PathBuf> = Vec::new();
    for case in &card.cases {
        if !seen.contains(&case.run_dir) {
            seen.push(case.run_dir.clone());
        }
    }
    seen
}

fn table(h: &mut String, rows: &[&CaseRow], opts: &HtmlOptions) {
    h.push_str(
        "<div class=\"scroll\"><table><thead><tr>\
         <th>Case</th><th>Status</th><th class=\"num\">Trials</th>\
         <th class=\"num\">Scored</th><th class=\"num\">Errored</th>\
         <th>Checks</th><th>Judgments</th></tr></thead><tbody>",
    );
    for case in rows {
        let status = format!("{:?}", case.status).to_uppercase();
        let class = match status.as_str() {
            "PASSED" => "pass",
            "FAILED" => "fail",
            _ => "mute",
        };
        let _ = write!(
            h,
            "<tr><td class=\"mono\">{}</td>\
             <td><span class=\"pill {}\">{}</span></td>\
             <td class=\"num\">{}</td><td class=\"num\">{}</td><td class=\"num\">{}</td>\
             <td>{}</td><td>{}</td></tr>",
            esc(&case.case_id),
            class,
            esc(&status),
            case.trials,
            case.scored_trials,
            case.error_count,
            checks_cell(&case.checks),
            judgments_cell(&case.judgments, opts)
        );
    }
    h.push_str("</tbody></table></div>");
}

/// Checks folded to `name n/m`, with the ones the backend could not observe
/// called out rather than silently counted as failures.
fn checks_cell(checks: &[CheckRow]) -> String {
    let mut names: Vec<&str> = checks.iter().map(|c| c.name.as_str()).collect();
    names.sort_unstable();
    names.dedup();
    if names.is_empty() {
        return "—".to_string();
    }
    let mut out = String::new();
    for name in names {
        let hits: Vec<&CheckRow> = checks.iter().filter(|c| c.name == name).collect();
        let observed = hits.iter().filter(|c| c.observed).count();
        let passed = hits.iter().filter(|c| c.observed && c.passed).count();
        let unobserved = hits.len() - observed;
        let class = if observed == 0 {
            "mute"
        } else if passed == observed {
            "pass"
        } else {
            "fail"
        };
        let suffix = if unobserved > 0 {
            format!(" +{unobserved} not observable")
        } else {
            String::new()
        };
        let _ = write!(
            out,
            "<span class=\"pill {}\">{} {}/{}{}</span> ",
            class,
            esc(name),
            passed,
            observed,
            esc(&suffix)
        );
    }
    out
}

fn judgments_cell(judgments: &[JudgmentRow], opts: &HtmlOptions) -> String {
    if judgments.is_empty() {
        return "—".to_string();
    }
    let mut out = String::new();
    for j in judgments {
        let _ = write!(
            out,
            "<details><summary>{} · trial {} · {}</summary>{}</details>",
            esc(&j.judge),
            j.trial_id,
            esc(&or_dash(j.overall.map(|o| format!("{o:.3}")))),
            judgment_body(j, opts)
        );
    }
    out
}

fn judgment_body(j: &JudgmentRow, opts: &HtmlOptions) -> String {
    if let Some(error) = &j.error {
        return format!(
            "<p class=\"pill fail\">judge error</p><p class=\"reasoning\">{}</p>",
            esc(error)
        );
    }
    let mut out = String::new();
    let _ = write!(
        out,
        "<p class=\"meta\">{} · judged {}</p>",
        esc(&j.judge_hash),
        esc(&j.judged_at)
    );
    out.push_str(
        "<div class=\"scroll\"><table><thead><tr><th>Criterion</th>\
         <th class=\"num\">Score</th><th>Answer</th><th>Reasoning</th>\
         </tr></thead><tbody>",
    );
    for c in &j.criteria {
        let answer = c
            .answer
            .as_ref()
            .map(|a| match a.as_str() {
                Some(text) => text.to_string(),
                None => a.to_string(),
            })
            .unwrap_or_else(|| "—".to_string());
        let _ = write!(
            out,
            "<tr><td>{}</td><td class=\"num\">{:.3}</td><td class=\"mono\">{}</td><td>{}</td></tr>",
            esc(&c.name),
            c.score,
            esc(&answer),
            reasoning_cell(c.reasoning.as_deref(), opts)
        );
    }
    out.push_str("</tbody></table></div>");
    out
}

/// Reasoning is the reason the case rows exist, so it is shown, not linked to.
/// Long reasoning is folded rather than truncated: the preview is what the eye
/// scans, and the rest is one click away in the same file.
fn reasoning_cell(reasoning: Option<&str>, opts: &HtmlOptions) -> String {
    if opts.no_reasoning {
        return "<span class=\"mute-text\">withheld</span>".to_string();
    }
    let Some(text) = reasoning else {
        return "—".to_string();
    };
    if text.chars().count() <= REASONING_PREVIEW {
        return format!("<span class=\"reasoning\">{}</span>", esc(text));
    }
    let head: String = text.chars().take(REASONING_PREVIEW).collect();
    format!(
        "<span class=\"reasoning\">{}…</span>\
         <details><summary>full reasoning</summary><p class=\"reasoning\">{}</p></details>",
        esc(&head),
        esc(text)
    )
}

#[cfg(test)]
mod tests {
    use super::super::super::document::CriterionRow;
    use super::*;

    fn judgment(reasoning: Option<&str>) -> JudgmentRow {
        JudgmentRow {
            trial_id: 1,
            judge: "quality".to_string(),
            judge_hash: "hash".to_string(),
            overall: Some(0.75),
            criteria: vec![CriterionRow {
                name: "clarity".to_string(),
                score: 0.75,
                answer: None,
                reasoning: reasoning.map(str::to_string),
            }],
            error: None,
            judged_at: "2026-09-04T12:00:00Z".to_string(),
        }
    }

    #[test]
    fn short_reasoning_is_shown_whole_and_long_reasoning_is_folded_not_cut() {
        let short = judgment(Some("names every flag"));
        let shown = judgments_cell(
            &[short],
            &HtmlOptions {
                no_reasoning: false,
                link_run_dirs: false,
            },
        );
        assert!(shown.contains("names every flag"));
        assert!(
            !shown.contains("full reasoning"),
            "short text needs no fold"
        );

        let long_text = "x".repeat(REASONING_PREVIEW + 40);
        let long = judgment(Some(&long_text));
        let shown = judgments_cell(
            &[long],
            &HtmlOptions {
                no_reasoning: false,
                link_run_dirs: false,
            },
        );
        assert!(shown.contains("full reasoning"), "long text folds");
        assert!(
            shown.contains(&long_text),
            "the fold holds the whole text, so nothing is lost"
        );
    }

    #[test]
    fn no_reasoning_leaves_no_reasoning_in_the_table() {
        let j = judgment(Some("a judge said something quotable"));
        let shown = judgments_cell(
            &[j],
            &HtmlOptions {
                no_reasoning: true,
                link_run_dirs: false,
            },
        );
        assert!(!shown.contains("quotable"));
        assert!(shown.contains("withheld"));
    }

    #[test]
    fn a_judge_error_is_reported_rather_than_shown_as_a_zero() {
        let mut j = judgment(None);
        j.overall = None;
        j.error = Some("endpoint refused the connection".to_string());
        let shown = judgments_cell(
            &[j],
            &HtmlOptions {
                no_reasoning: false,
                link_run_dirs: false,
            },
        );
        assert!(shown.contains("judge error"));
        assert!(shown.contains("endpoint refused"));
        assert!(!shown.contains("0.000"), "an error is not a score");
    }
}
