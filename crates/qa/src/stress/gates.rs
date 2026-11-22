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
use Situation::{A1, A2, A3, A4, A5, A6, A7, B1, B2, B4, B5, B6, B7, B9, C2, C5, D1, D2, D3, P1, P2, P3, P4, P5, P7, P8};
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
    // Wave 2 (written before its first run, 22 Nov 2026).
    gate(P3, O, "seat_cities", DGE, [-0.3, -0.5, -0.8], NET_DEFENCE),
    gate(P3, O, "seat_score_share", DGE, [-0.01, -0.02, -0.03], NET_DEFENCE),
    gate(P3, O, "end_seat_win_weight", DGE, [-0.03, -0.05, -0.08], NET_DEFENCE),
    gate(P3, B, "counter_by_3", EGE, [0.8, 0.8, 0.8], NET_DEFENCE),
    gate(P3, B, "military_added_before_war", DGE, [1.0, 1.0, 2.0], NET_DEFENCE),
    gate(P2, O, "seat_alive", DGE, [-0.02, -0.05, -0.1], NET_DEFENCE),
    gate(P2, O, "seat_cities", DGE, [-0.2, -0.4, -0.6], NET_DEFENCE),
    gate(P2, O, "end_seat_win_weight", DGE, [-0.03, -0.05, -0.08], NET_DEFENCE),
    gate(P2, B, "capital_lost", ELE, [0.1, 0.2, 0.3], NET_DEFENCE),
    gate(P2, B, "counter_by_3", EGE, [0.8, 0.8, 0.8], NET_DEFENCE),
    gate(P7, O, "seat_score_share", DGE, [-0.03, -0.03, -0.03], NET_WONDER),
    gate(P7, O, "end_seat_win_weight", DGE, [-0.03, -0.03, -0.03], NET_WONDER),
    gate(P7, B, "hammers_burned", ELE, [0.0, 0.0, 0.0], NET_WONDER),
    gate(P7, B, "lost_head_rounds", ELE, [1.0, 1.0, 1.0], NET_WONDER),
    gate(P5, O, "seat_score_share", DGE, [-0.01, -0.02, -0.03], NET_BORDER),
    gate(P5, O, "seat_cities", DGE, [-0.1, -0.2, -0.4], NET_BORDER),
    gate(P5, O, "end_seat_win_weight", DGE, [-0.02, -0.03, -0.05], NET_BORDER),
    gate(P5, B, "forward_city_held", ELE, [0.8, 0.7, 0.6], NET_BORDER),
    gate(P5, B, "war_by_seat", EGE, [0.1, 0.2, 0.0], NET_BORDER),
    gate(P1, O, "seat_score_share", DGE, [-0.005, -0.01, -0.02], NET_RAID),
    gate(P1, O, "seat_cities", DGE, [-0.1, -0.2, -0.3], NET_RAID),
    gate(P1, O, "end_seat_win_weight", DGE, [-0.02, -0.03, -0.05], NET_RAID),
    gate(P1, B, "raiders_dead", EGE, [0.5, 0.5, 0.5], NET_RAID),
    gate(P1, B, "pillaged_end", DLE, [1.0, 2.0, 3.0], NET_RAID),
    gate(P1, B, "civilians_end", DGE, [-0.5, -1.0, -1.5], NET_RAID),
    gate(P4, O, "seat_score_share", DGE, [-0.01, -0.015, -0.02], NET_DEALS),
    gate(P4, O, "seat_treasury", DGE, [-50.0, -100.0, -150.0], NET_DEALS),
    gate(P4, O, "end_seat_win_weight", DGE, [-0.02, -0.03, -0.05], NET_DEALS),
    gate(P4, B, "script_sold_then_war", ELE, [0.3, 0.2, 0.1], NET_DEALS),
    gate(P8, O, "seat_score_share", DGE, [-0.01, -0.02, -0.03], NET_SWEEP),
    gate(P8, O, "leader_won", ELE, [0.05, 0.1, 0.15], NET_SWEEP),
    gate(P8, O, "end_seat_win_weight", DGE, [-0.05, -0.1, -0.15], NET_SWEEP),
    gate(P8, B, "gifts_others", DGE, [1.0, 2.0, 3.0], NET_SWEEP),
    gate(P8, B, "leader_allies", DLE, [2.0, 3.0, 4.0], NET_SWEEP),
    gate(A2, O, "seat_cities", DGE, [-0.1, -0.2, -0.3], "strategist"),
    gate(A2, O, "seat_score_share", DGE, [-0.01, -0.02, -0.03], "strategist"),
    gate(A2, O, "end_seat_win_weight", DGE, [-0.05, -0.1, -0.15], "strategist"),
    gate(A2, B, "seat_military", DGE, [0.5, 1.0, 2.0], "strategist"),
    gate(A2, B, "wars_on_leader", DLE, [0.0, 0.0, 0.0], "strategist"),
    gate(A3, O, "seat_score_share", DGE, [-0.01, -0.02, -0.04], "strategist"),
    gate(A3, O, "leader_won", ELE, [0.05, 0.1, 0.2], "strategist"),
    gate(A3, O, "end_seat_win_weight", DGE, [-0.05, -0.1, -0.15], "strategist"),
    gate(A3, B, "wars_on_leader", DGE, [0.1, 0.2, 0.3], "strategist"),
    gate(A4, O, "seat_score_share", DGE, [-0.01, -0.02, -0.04], NET_SWEEP),
    gate(A4, O, "leader_won", ELE, [0.05, 0.1, 0.2], NET_SWEEP),
    gate(A4, O, "end_seat_win_weight", DGE, [-0.05, -0.1, -0.15], NET_SWEEP),
    gate(A4, B, "gifts_others", DGE, [0.5, 1.0, 1.0], NET_SWEEP),
    gate(A4, B, "leader_allies", DLE, [1.0, 2.0, 3.0], NET_SWEEP),
    gate(A6, O, "seat_cities", DGE, [0.1, 0.2, 0.3], NET_SEA),
    gate(A6, O, "end_seat_win_weight", DGE, [0.0, 0.0, 0.0], NET_SEA),
    gate(A6, B, "weak_captured", DGE, [0.1, 0.15, 0.2], NET_SEA),
    gate(A6, B, "nobody_attacked", ELE, [0.7, 0.6, 0.5], NET_SEA),
    gate(C2, O, "seat_cities", DGE, [0.2, 0.3, 0.4], NET_SEA),
    gate(C2, O, "end_seat_win_weight", DGE, [0.0, 0.0, 0.0], NET_SEA),
    gate(C2, B, "weak_captured", DGE, [0.15, 0.25, 0.3], NET_SEA),
    gate(A7, O, "seat_cities", DGE, [-0.3, -0.5, -0.8], "strategist"),
    gate(A7, O, "seat_score_share", DGE, [-0.01, -0.02, -0.03], "strategist"),
    gate(A7, O, "end_seat_win_weight", DGE, [-0.03, -0.05, -0.08], "strategist"),
    gate(A7, B, "wars_declared", DLE, [0.0, 0.0, 0.0], "strategist"),
    gate(A7, B, "seat_military", DGE, [-2.0, -3.0, -4.0], "strategist"),
    gate(B2, O, "seat_cities", DGE, [-0.3, -0.5, -0.8], NET_DEFENCE),
    gate(B2, O, "seat_score_share", DGE, [-0.01, -0.02, -0.03], NET_DEFENCE),
    gate(B2, O, "end_seat_win_weight", DGE, [-0.03, -0.05, -0.08], NET_DEFENCE),
    gate(B2, B, "counter_by_3", EGE, [0.8, 0.8, 0.8], NET_DEFENCE),
    gate(B2, B, "purchases", DGE, [0.5, 1.0, 1.0], NET_DEFENCE),
    gate(B5, O, "seat_cities", DGE, [-0.3, -0.5, -0.8], NET_DEFENCE),
    gate(B5, O, "seat_score_share", DGE, [-0.01, -0.02, -0.03], NET_DEFENCE),
    gate(B5, O, "end_seat_win_weight", DGE, [-0.03, -0.05, -0.08], NET_DEFENCE),
    gate(B5, B, "counter_by_3", EGE, [0.8, 0.8, 0.8], NET_DEFENCE),
    gate(B5, B, "purchases", DGE, [0.5, 1.0, 1.0], NET_DEFENCE),
    gate(B6, O, "seat_score_share", DGE, [-0.01, -0.015, -0.02], "strategist; supply none"),
    gate(B6, O, "seat_cities", DGE, [-0.2, -0.3, -0.4], "strategist; supply none"),
    gate(B6, O, "end_seat_win_weight", DGE, [-0.02, -0.03, -0.05], "strategist; supply none"),
    gate(B6, B, "seat_military", DGE, [-1.0, -2.0, -3.0], "strategist; supply none"),
    gate(B7, O, "seat_score_share", DGE, [-0.01, -0.02, -0.03], "strategist"),
    gate(B7, O, "seat_cities", DGE, [-0.2, -0.3, -0.4], "strategist"),
    gate(B7, O, "end_seat_win_weight", DGE, [-0.02, -0.03, -0.05], "strategist"),
    gate(B7, B, "unhappy_rounds", DLE, [5.0, 8.0, 10.0], "strategist"),
    gate(B7, B, "broke_rounds", DLE, [3.0, 5.0, 5.0], "strategist"),
    gate(D2, O, "seat_cities", DGE, [-0.3, -0.3, -0.3], "none (no net for allied action)"),
    gate(D2, O, "end_seat_win_weight", DGE, [-0.03, -0.03, -0.03], "none (no net for allied action)"),
    gate(D2, B, "ally_at_war", EGE, [0.9, 0.9, 0.9], "none (the rules' pact)"),
    gate(D2, B, "ally_struck", EGE, [0.5, 0.5, 0.5], "none (no net for allied action)"),
    gate(D2, B, "ally_left", ELE, [0.3, 0.3, 0.3], "none (no net for allied action)"),
    gate(D3, O, "seat_alive", DGE, [-0.05, -0.1, -0.15], NET_PEACE),
    gate(D3, O, "seat_cities", DGE, [-1.3, -2.5, -3.6], NET_PEACE),
    gate(D3, O, "end_seat_win_weight", DGE, [-0.05, -0.08, -0.1], NET_PEACE),
    gate(D3, B, "peace", EGE, [0.4, 0.5, 0.6], NET_PEACE),
    gate(D3, B, "peace_round", ELE, [15.0, 12.0, 10.0], NET_PEACE),
];

const NET_DEFENCE: &str = "danger net, tactician, commander (off in standard); scripted defence.rs";
const NET_WONDER: &str = "none: scripted wonder_race.rs; strategist production head";
const NET_BORDER: &str = "site net, danger net (off); scripted defence.rs";
const NET_RAID: &str = "none: scripted (worker flight, settler escort); tactician, commander (off)";
const NET_DEALS: &str = "none: scripted (peace.rs Negotiation::value, diplomacy.rs)";
const NET_SWEEP: &str = "none: scripted (city_states.rs, victory_watch.rs); strategist has no diplomacy outputs";
const NET_SEA: &str = "commander; fleet none (fleet.rs)";
const NET_PEACE: &str = "none: scripted peace.rs";

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
