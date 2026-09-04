//! The progress section (spec eval-scorecard-report R5).
//!
//! Two scorecards are comparable only when they asked the same question, so
//! the caller has already refused any set whose `benchmark.sha256` differs or
//! is absent. What is left here is drawing: one chart per metric, series keyed
//! by target, `generated_at` on the x axis, the gate drawn as a line, and every
//! point where the verdict flipped marked — because "0.71 → 0.69" and
//! "passing → failing" are different news.

use super::super::document::{MetricReport, Scorecard};
use super::{esc, pct};
use std::fmt::Write as _;

const W: f64 = 460.0;
const H: f64 = 210.0;
/// Room for the axis labels. Text drawn outside the viewBox is text nobody
/// reads, so the plot is inset by exactly what the labels need.
const PAD_L: f64 = 56.0;
const PAD_R: f64 = 16.0;
const PAD_T: f64 = 18.0;
const PAD_B: f64 = 52.0;
/// Series colours, in assignment order. Past the end, series share the last —
/// a progress chart of eight targets is a reporting mistake, not a palette one.
const SERIES_CLASSES: [&str; 4] = ["s0", "s1", "s2", "s3"];

pub fn progress(h: &mut String, cards: &[Scorecard]) {
    let names = metric_names(cards);
    h.push_str("<section><h2>Progress</h2>");
    let _ = write!(
        h,
        "<p class=\"meta\">{} scorecards over the same benchmark \
         (<span class=\"mono\">{}</span>), oldest first.</p>",
        cards.len(),
        esc(cards
            .first()
            .and_then(|c| c.benchmark.sha256.as_deref())
            .unwrap_or("—"))
    );
    let targets = target_keys(cards);
    if targets.len() > 1 {
        h.push_str("<p class=\"legend\">");
        for (n, target) in targets.iter().enumerate() {
            let _ = write!(
                h,
                "<span class=\"key {}\">{}</span>",
                SERIES_CLASSES[n.min(SERIES_CLASSES.len() - 1)],
                esc(target)
            );
        }
        h.push_str("</p>");
    }
    if names.is_empty() {
        h.push_str(
            "<p class=\"empty\">No metric appears in more than one scorecard.</p></section>\n",
        );
        return;
    }
    h.push_str("<div class=\"charts\">");
    for name in names {
        chart(h, cards, &targets, &name);
    }
    h.push_str("</div></section>\n");
}

/// A scorecard's target, as one string. A card folded over several targets has
/// no single one, and `targets[]` already says so.
fn target_of(card: &Scorecard) -> String {
    match (&card.agent, &card.model) {
        (Some(agent), Some(model)) => format!("{agent}/{model}"),
        (Some(agent), None) => agent.clone(),
        _ => format!("{} targets", card.targets.len()),
    }
}

fn target_keys(cards: &[Scorecard]) -> Vec<String> {
    let mut keys: Vec<String> = Vec::new();
    for card in cards {
        let key = target_of(card);
        if !keys.contains(&key) {
            keys.push(key);
        }
    }
    keys
}

/// Metric names that appear in at least two of the scorecards. A metric added
/// last week has no history, and a straight line through one point implies one.
fn metric_names(cards: &[Scorecard]) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for card in cards {
        for m in &card.metrics {
            if !names.contains(&m.name) {
                names.push(m.name.clone());
            }
        }
    }
    names.retain(|name| {
        cards
            .iter()
            .filter(|c| c.metrics.iter().any(|m| &m.name == name))
            .count()
            >= 2
    });
    names
}

/// The number a metric reports, whichever kind it is.
fn value(m: &MetricReport) -> Option<f64> {
    m.rate.or(m.score).or(m.p95_tool_calls.map(|v| v as f64))
}

fn format_value(m: &MetricReport, v: f64) -> String {
    if m.rate.is_some() {
        pct(v)
    } else if m.p95_tool_calls.is_some() {
        format!("{v:.0}")
    } else {
        format!("{v:.3}")
    }
}

/// The threshold as a number, parsed back out of the text the metric carries.
/// The document stores the gate as prose (`>= 80%`, `<= 12`) because that is
/// what a reader wants; the chart needs it as a coordinate.
fn gate(threshold: &str) -> Option<f64> {
    let rest = threshold
        .trim_start_matches(">=")
        .trim_start_matches("<=")
        .trim();
    match rest.strip_suffix('%') {
        Some(number) => number.trim().parse::<f64>().ok().map(|v| v / 100.0),
        None => rest.parse::<f64>().ok(),
    }
}

/// `generated_at` as a number on the x axis. An unparseable timestamp falls
/// back to the card's position, so a hand-edited scorecard still plots.
fn instant(card: &Scorecard, index: usize) -> f64 {
    chrono::DateTime::parse_from_rfc3339(&card.generated_at)
        .map(|t| t.timestamp() as f64)
        .unwrap_or(index as f64)
}

fn day(card: &Scorecard) -> String {
    card.generated_at
        .split('T')
        .next()
        .unwrap_or(&card.generated_at)
        .to_string()
}

/// The y extent: every value, the gate — which has to be on the chart or the
/// chart cannot show a crossing — and a margin around both. A rate is then
/// clamped, because an axis labelled `107.5%` tells the reader the chart is
/// wrong even when the line through it is right.
fn y_range(values: &[f64], threshold: Option<f64>, is_rate: bool) -> (f64, f64) {
    let mut lo = values.iter().cloned().fold(f64::INFINITY, f64::min);
    let mut hi = values.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    if let Some(t) = threshold {
        lo = lo.min(t);
        hi = hi.max(t);
    }
    let pad = ((hi - lo) * 0.15).max(0.01);
    let (mut lo, mut hi) = (lo - pad, hi + pad);
    if is_rate {
        lo = lo.max(0.0);
        hi = hi.min(1.0);
    }
    (lo, hi)
}

struct Point {
    x: f64,
    y: f64,
    label: String,
    verdict: String,
    flip: bool,
}

fn chart(h: &mut String, cards: &[Scorecard], targets: &[String], name: &str) {
    // Every (card, metric) pair this chart draws, whatever its target.
    let hits: Vec<(usize, &Scorecard, &MetricReport)> = cards
        .iter()
        .enumerate()
        .filter_map(|(i, c)| c.metrics.iter().find(|m| m.name == name).map(|m| (i, c, m)))
        .collect();
    let values: Vec<f64> = hits.iter().filter_map(|(_, _, m)| value(m)).collect();
    if values.len() < 2 {
        return;
    }
    let sample = hits.first().map(|(_, _, m)| *m);
    let threshold = sample.and_then(|m| gate(&m.threshold));

    let (lo, hi) = y_range(&values, threshold, sample.is_some_and(|m| m.rate.is_some()));

    let plot_w = W - PAD_L - PAD_R;
    let plot_h = H - PAD_T - PAD_B;
    let times: Vec<f64> = hits.iter().map(|(i, c, _)| instant(c, *i)).collect();
    let t0 = times.iter().cloned().fold(f64::INFINITY, f64::min);
    let t1 = times.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let x_of = |t: f64| {
        if t1 > t0 {
            PAD_L + (t - t0) / (t1 - t0) * plot_w
        } else {
            PAD_L + plot_w / 2.0
        }
    };
    let y_of = |v: f64| PAD_T + plot_h - (v - lo) / (hi - lo) * plot_h;

    let _ = write!(
        h,
        "<figure class=\"chart\"><figcaption>{}</figcaption>\
         <svg viewBox=\"0 0 {W} {H}\" role=\"img\" aria-label=\"{} over {} scorecards\">",
        esc(name),
        esc(name),
        hits.len()
    );

    let base = PAD_T + plot_h;
    let _ = write!(
        h,
        "<line class=\"axis\" x1=\"{PAD_L}\" y1=\"{base}\" x2=\"{}\" y2=\"{base}\"/>\
         <line class=\"axis\" x1=\"{PAD_L}\" y1=\"{PAD_T}\" x2=\"{PAD_L}\" y2=\"{base}\"/>",
        W - PAD_R
    );
    // Both y-extremes get a label, so every tick names a value the plot reaches.
    for v in [hi, lo] {
        let _ = write!(
            h,
            "<text class=\"tick\" x=\"{}\" y=\"{:.1}\" text-anchor=\"end\">{}</text>",
            PAD_L - 6.0,
            y_of(v) + 4.0,
            esc(&sample.map(|m| format_value(m, v)).unwrap_or_default())
        );
    }
    if let Some(t) = threshold {
        let _ = write!(
            h,
            "<line class=\"gate\" x1=\"{PAD_L}\" y1=\"{y:.1}\" x2=\"{}\" y2=\"{y:.1}\"/>\
             <text class=\"tick gate-label\" x=\"{}\" y=\"{:.1}\" text-anchor=\"end\">gate {}</text>",
            W - PAD_R,
            W - PAD_R,
            y_of(t) - 5.0,
            esc(sample.map(|m| m.threshold.as_str()).unwrap_or("")),
            y = y_of(t)
        );
    }

    let mut flips = 0usize;
    for (n, target) in targets.iter().enumerate() {
        let class = SERIES_CLASSES[n.min(SERIES_CLASSES.len() - 1)];
        let mut points: Vec<Point> = Vec::new();
        let mut previous: Option<String> = None;
        for (i, card, m) in hits.iter().filter(|(_, c, _)| &target_of(c) == target) {
            let Some(v) = value(m) else { continue };
            let flip = previous.as_deref().is_some_and(|p| p != m.verdict.as_ref());
            previous = Some(m.verdict.to_string());
            points.push(Point {
                x: x_of(instant(card, *i)),
                y: y_of(v),
                label: format!("{} · {} · {}", day(card), format_value(m, v), m.verdict),
                verdict: m.verdict.to_string(),
                flip,
            });
        }
        if points.is_empty() {
            continue;
        }
        if points.len() > 1 {
            let path = points
                .iter()
                .map(|p| format!("{:.1},{:.1}", p.x, p.y))
                .collect::<Vec<_>>()
                .join(" ");
            let _ = write!(h, "<polyline class=\"series {class}\" points=\"{path}\"/>");
        }
        for p in &points {
            flips += usize::from(p.flip);
            let state = match (p.flip, p.verdict.as_str()) {
                (true, _) => "flip",
                (_, "PASS") => "pass",
                _ => "fail",
            };
            let _ = write!(
                h,
                "<circle class=\"pt {class} {state}\" cx=\"{:.1}\" cy=\"{:.1}\" r=\"{}\">\
                 <title>{}</title></circle>",
                p.x,
                p.y,
                if p.flip { 5.5 } else { 3.5 },
                esc(&p.label)
            );
        }
    }

    // Only the ends of the axis are labelled; a label per point overlaps as
    // soon as there are more than a handful.
    if let (Some(first), Some(last)) = (hits.first(), hits.last()) {
        let baseline = base + 18.0;
        let _ = write!(
            h,
            "<text class=\"tick\" x=\"{PAD_L}\" y=\"{baseline}\" text-anchor=\"start\">{}</text>\
             <text class=\"tick\" x=\"{}\" y=\"{baseline}\" text-anchor=\"end\">{}</text>",
            esc(&day(first.1)),
            W - PAD_R,
            esc(&day(last.1))
        );
    }
    if flips > 0 {
        let _ = write!(
            h,
            "<text class=\"tick flip-note\" x=\"{PAD_L}\" y=\"{:.1}\" text-anchor=\"start\">\
             {} verdict change{}</text>",
            base + 36.0,
            flips,
            if flips == 1 { "" } else { "s" }
        );
    }
    h.push_str("</svg></figure>");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rate_axis_stays_inside_the_range_a_rate_can_have() {
        let (lo, hi) = y_range(&[0.42, 1.0], Some(0.75), true);
        assert!(lo >= 0.0 && hi <= 1.0, "a rate axis ran to {lo}..{hi}");
        assert!(hi > lo, "and it still has an extent");

        // A judge score is not a rate — nothing says it stops at 1.0 — so it
        // keeps the margin that makes the top point visible.
        let (lo, hi) = y_range(&[0.42, 1.0], Some(0.75), false);
        assert!(
            lo < 0.42 && hi > 1.0,
            "a score axis was clamped to {lo}..{hi}"
        );
    }

    #[test]
    fn a_threshold_written_for_a_reader_still_parses_as_a_coordinate() {
        assert_eq!(gate(">= 80%"), Some(0.8));
        assert_eq!(gate("<= 12"), Some(12.0));
        assert_eq!(gate(">= 0.75"), Some(0.75));
        assert_eq!(gate("whatever the judge says"), None);
    }
}
