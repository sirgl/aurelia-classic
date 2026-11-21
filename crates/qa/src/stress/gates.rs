//! The gates, written before the first run (classic/docs/ai-stress.md holds the same table; the test
//! `gates_match_the_document` keeps the two equal) and not changed after a result. A gate is a number
//! per strength of the edit: the mild level is the one whose failure matters most.
//!
//! Two kinds: OUTCOME gates read what happened to the nations the situation judges (their cities and
//! share of the score after the window, real victories); BEHAVIOUR gates read what the AI did and are
//! diagnostics. A situation passes when its outcome gates pass at every level.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use super::{Row, Situation};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Outcome,
    Behaviour,
}

/// What a gate compares with its threshold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Test {
    /// mean(edited - control) >= threshold
    DeltaAtLeast,
    /// mean(edited - control) <= threshold
    DeltaAtMost,
    /// mean(edited) >= threshold
    EditedAtLeast,
    /// mean(edited) <= threshold
    EditedAtMost,
}

impl Test {
    fn symbol(self) -> &'static str {
        match self {
            Test::DeltaAtLeast => "delta >=",
            Test::DeltaAtMost => "delta <=",
            Test::EditedAtLeast => "edited >=",
            Test::EditedAtMost => "edited <=",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Gate {
    pub situation: Situation,
    pub kind: Kind,
    pub metric: &'static str,
    pub test: Test,
    /// By level: mild, medium, strong.
    pub threshold: [f64; 3],
    /// The small net the decision belongs to (work/ai-scenarios.html).
    pub net: &'static str,
}

const fn gate(situation: Situation, kind: Kind, metric: &'static str, test: Test, threshold: [f64; 3], net: &'static str) -> Gate {
    Gate { situation, kind, metric, test, threshold, net }
}

use Kind::{Behaviour as B, Outcome as O};
use Situation::{A1, A5, B1, B4, B9, C5, D1};
use Test::{DeltaAtLeast as DGE, DeltaAtMost as DLE, EditedAtLeast as EGE, EditedAtMost as ELE};

pub const GATES: &[Gate] = &[
    gate(A5, O, "seat_cities", DGE, [0.2, 0.35, 0.5], "strategist (posture), commander"),
    gate(A5, B, "weak_captured", DGE, [0.15, 0.25, 0.3], "commander"),
    gate(A5, B, "nobody_attacked", ELE, [0.6, 0.5, 0.4], "strategist (posture)"),
    gate(B1, O, "seat_cities", DGE, [-0.3, -0.5, -0.8], "tactician, commander, danger net"),
    gate(B1, O, "seat_score_share", DGE, [-0.01, -0.02, -0.03], "tactician, commander, danger net"),
    gate(B1, B, "counter_by_3", EGE, [0.8, 0.8, 0.8], "commander, tactician"),
    gate(B1, B, "purchases", DGE, [0.5, 1.0, 1.0], "tactician, danger net"),
    gate(A1, O, "seat_score_share", DGE, [-0.01, -0.02, -0.04], "strategist (no diplomacy outputs)"),
    gate(A1, O, "leader_won", ELE, [0.05, 0.1, 0.2], "strategist"),
    gate(A1, B, "wars_on_leader", DGE, [0.25, 0.5, 0.5], "strategist"),
    gate(A1, B, "denouncements_of_leader", DGE, [0.5, 1.0, 1.0], "strategist"),
    gate(A1, B, "research_pacts_others", DGE, [0.25, 0.5, 0.5], "strategist"),
    gate(A1, B, "tech_gap", DLE, [2.0, 4.0, 8.0], "strategist"),
    gate(B4, O, "seat_cities", DGE, [-0.7, -1.4, -2.1], "commander, scripted retake"),
    gate(B4, O, "seat_score_share", DGE, [-0.04, -0.07, -0.1], "commander, scripted retake"),
    gate(B4, B, "capital_retaken", EGE, [0.3, 0.3, 0.3], "commander, scripted retake"),
    gate(B4, B, "further_losses", DLE, [0.5, 0.8, 1.0], "commander"),
    gate(B9, O, "rival_wins_science", ELE, [0.5, 0.4, 0.3], "strategist"),
    gate(B9, B, "rival_cities_lost", DGE, [0.5, 0.5, 0.5], "strategist, commander"),
    gate(D1, O, "seat_cities", DGE, [0.3, 0.3, 0.3], "none (no net for allied action)"),
    gate(D1, B, "both_struck", EGE, [0.5, 0.5, 0.5], "none"),
    gate(D1, B, "separate_peace", ELE, [0.3, 0.3, 0.3], "none"),
    gate(C5, B, "host_dead_at_end", ELE, [0.0, 0.0, 0.0], "none (the rules' table: congress.rs)"),
    gate(A5, O, "end_seat_win_weight", DGE, [0.0, 0.0, 0.0], "strategist (posture), commander"),
    gate(A5, O, "end_seat_score_share", DGE, [0.0, 0.005, 0.01], "strategist (posture), commander"),
    gate(B1, O, "end_seat_win_weight", DGE, [-0.03, -0.05, -0.08], "tactician, commander, danger net"),
    gate(B1, O, "end_seat_score_share", DGE, [-0.01, -0.02, -0.03], "tactician, commander, danger net"),
    gate(A1, O, "end_seat_win_weight", DGE, [-0.05, -0.1, -0.15], "strategist"),
    gate(A1, O, "end_seat_score_share", DGE, [-0.02, -0.04, -0.06], "strategist"),
    gate(B4, O, "end_seat_win_weight", DGE, [-0.03, -0.05, -0.08], "commander, scripted retake"),
    gate(B4, O, "end_seat_score_share", DGE, [-0.03, -0.05, -0.07], "commander, scripted retake"),
    gate(B9, O, "end_seat_win_weight", DGE, [-0.05, -0.1, -0.15], "strategist"),
    gate(B9, O, "end_seat_score_share", DGE, [-0.02, -0.04, -0.06], "strategist"),
    gate(D1, O, "end_seat_win_weight", DGE, [0.0, 0.0, 0.0], "none"),
    gate(D1, O, "end_seat_score_share", DGE, [0.0, 0.0, 0.0], "none"),
];

/// The gates of one situation.
pub fn of(situation: Situation) -> impl Iterator<Item = &'static Gate> {
    GATES.iter().filter(move |g| g.situation == situation)
}

/// The gates as the document's table.
pub fn markdown() -> String {
    let mut out = String::from("| situation | kind | metric | test | mild | medium | strong | owning small net |\n|---|---|---|---|---|---|---|---|\n");
    for g in GATES {
        let kind = if g.kind == Kind::Outcome { "outcome" } else { "behaviour" };
        let _ = writeln!(out, "| {} | {kind} | `{}` | {} | {} | {} | {} | {} |", g.situation.name().to_uppercase(), g.metric, g.test.symbol(), g.threshold[0], g.threshold[1], g.threshold[2], g.net);
    }
    out
}

/// One line of the result table.
#[derive(Debug, Clone)]
pub struct Verdict {
    pub gate: Gate,
    pub level: u8,
    pub n: usize,
    pub edited: f64,
    pub control: f64,
    pub delta: f64,
    pub delta_se: f64,
    pub pass: bool,
}

/// The mean of `edited`, of `control` and of their paired difference with its standard error.
fn means(rows: &[&Row], metric: &str) -> Option<(usize, f64, f64, f64, f64)> {
    let pairs: Vec<(f64, f64)> = rows.iter().filter_map(|r| Some((*r.edited.get(metric)?, *r.control.get(metric)?))).collect();
    if pairs.is_empty() {
        return None;
    }
    let n = pairs.len() as f64;
    let diffs: Vec<f64> = pairs.iter().map(|(e, c)| e - c).collect();
    let delta = diffs.iter().sum::<f64>() / n;
    let var = diffs.iter().map(|d| (d - delta).powi(2)).sum::<f64>() / (n - 1.0).max(1.0);
    Some((pairs.len(), pairs.iter().map(|p| p.0).sum::<f64>() / n, pairs.iter().map(|p| p.1).sum::<f64>() / n, delta, (var / n).sqrt()))
}

/// Every gate against the rows, by level.
pub fn judge(rows: &[Row]) -> Vec<Verdict> {
    let mut by: BTreeMap<(Situation, u8), Vec<&Row>> = BTreeMap::new();
    for row in rows {
        if let Ok(s) = Situation::parse(&row.situation) {
            by.entry((s, row.level)).or_default().push(row);
        }
    }
    let mut out = Vec::new();
    for ((situation, level), rows) in &by {
        for gate in of(*situation) {
            let Some((n, edited, control, delta, delta_se)) = means(rows, gate.metric) else { continue };
            let t = gate.threshold[usize::from((*level).min(2))];
            let pass = match gate.test {
                Test::DeltaAtLeast => delta >= t,
                Test::DeltaAtMost => delta <= t,
                Test::EditedAtLeast => edited >= t,
                Test::EditedAtMost => edited <= t,
            };
            out.push(Verdict { gate: *gate, level: *level, n, edited, control, delta, delta_se, pass });
        }
    }
    out
}

/// The result table in Markdown.
pub fn results_markdown(results: &[Verdict]) -> String {
    let mut out = String::from("| situation | level | kind | metric | n | edited | control | delta (se) | gate | result |\n|---|---|---|---|---|---|---|---|---|---|\n");
    for r in results {
        let kind = if r.gate.kind == Kind::Outcome { "outcome" } else { "behaviour" };
        let t = r.gate.threshold[usize::from(r.level.min(2))];
        let verdict = if r.pass { "pass" } else { "FAIL" };
        let _ = writeln!(out, "| {} | {} | {kind} | `{}` | {} | {:.3} | {:.3} | {:+.3} ({:.3}) | {} {} | {verdict} |", r.gate.situation.name().to_uppercase(), r.level, r.gate.metric, r.n, r.edited, r.control, r.delta, r.delta_se, r.gate.test.symbol(), t);
    }
    out
}
