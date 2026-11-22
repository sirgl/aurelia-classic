//! The exam as a suite: every situation judged for a candidate AI variant and for `standard`, the
//! reference, in one table "situation x strength": the verdict of the outcome gates, the outcome
//! deltas (edited - control) and the behaviour gates passed (`ai-stress suite`).

use std::collections::BTreeMap;
use std::fmt::Write as _;

use super::gates::{self, Kind, Verdict};
use super::{Row, Situation};

/// The verdicts of one (situation, level).
#[derive(Debug, Clone, Default)]
struct Cell {
    n: usize,
    outcome: Vec<Verdict>,
    behaviour: Vec<Verdict>,
}

impl Cell {
    fn passes(&self) -> bool {
        self.outcome.iter().all(|v| v.pass)
    }

    fn verdict(&self) -> String {
        if self.n == 0 {
            return "-".to_owned();
        }
        let failed: Vec<&str> = self.outcome.iter().filter(|v| !v.pass).map(|v| v.gate.metric).collect();
        if failed.is_empty() {
            "pass".to_owned()
        } else {
            format!("FAIL ({})", failed.join(", "))
        }
    }

    fn deltas(&self) -> String {
        self.outcome.iter().map(|v| format!("{} {:+.3}", short(v.gate.metric), v.delta)).collect::<Vec<_>>().join("; ")
    }

    fn behaviour(&self) -> String {
        if self.behaviour.is_empty() {
            return "-".to_owned();
        }
        let passed = self.behaviour.iter().filter(|v| v.pass).count();
        let failed: Vec<String> = self.behaviour.iter().filter(|v| !v.pass).map(|v| format!("{} {:.2}", v.gate.metric, shown(v))).collect();
        if failed.is_empty() {
            format!("{passed}/{}", self.behaviour.len())
        } else {
            format!("{passed}/{}: {}", self.behaviour.len(), failed.join(", "))
        }
    }
}

/// The number a gate compares: the delta or the edited mean.
fn shown(v: &Verdict) -> f64 {
    match v.gate.test {
        gates::Test::DeltaAtLeast | gates::Test::DeltaAtMost => v.delta,
        gates::Test::EditedAtLeast | gates::Test::EditedAtMost => v.edited,
    }
}

fn short(metric: &str) -> &str {
    metric.strip_prefix("seat_").unwrap_or(metric)
}

fn cells(rows: &[Row]) -> BTreeMap<(Situation, u8), Cell> {
    let mut out: BTreeMap<(Situation, u8), Cell> = BTreeMap::new();
    for row in rows {
        if let Ok(s) = Situation::parse(&row.situation) {
            out.entry((s, row.level)).or_default().n += 1;
        }
    }
    for v in gates::judge(rows) {
        let cell = out.entry((v.gate.situation, v.level)).or_default();
        match v.gate.kind {
            Kind::Outcome => cell.outcome.push(v),
            Kind::Behaviour => cell.behaviour.push(v),
        }
    }
    out
}

/// The suite's table: one line per situation and strength, the candidate beside the reference
/// (none: the candidate is `standard` itself).
pub fn table(variant: &str, candidate: &[Row], reference: Option<&[Row]>) -> String {
    let cand = cells(candidate);
    let refr = reference.map(cells).unwrap_or_default();
    let same = reference.is_none();
    let mut out = String::new();
    let level_name = ["mild", "medium", "strong"];
    if same {
        let _ = writeln!(out, "| situation | strength | n | outcome | outcome delta | behaviour gates passed | owning net |\n|---|---|---|---|---|---|---|");
    } else {
        let _ = writeln!(out, "| situation | strength | n | {variant} | standard | {variant} outcome delta | standard outcome delta | {variant} behaviour | standard behaviour | owning net |\n|---|---|---|---|---|---|---|---|---|---|");
    }
    for situation in Situation::ALL {
        for level in 0..situation.levels() {
            let key = (situation, level);
            let c = cand.get(&key).cloned().unwrap_or_default();
            let r = refr.get(&key).cloned().unwrap_or_default();
            if c.n == 0 && r.n == 0 {
                continue;
            }
            let net = gates::of(situation).find(|g| g.kind == Kind::Outcome).map_or("", |g| g.net);
            let strength = if situation.levels() == 1 { "one" } else { level_name[usize::from(level)] };
            let name = situation.name().to_uppercase();
            if same {
                let _ = writeln!(out, "| {name} | {strength} | {} | {} | {} | {} | {net} |", c.n, c.verdict(), c.deltas(), c.behaviour());
            } else {
                let _ = writeln!(out, "| {name} | {strength} | {}/{} | {} | {} | {} | {} | {} | {} | {net} |", c.n, r.n, c.verdict(), r.verdict(), c.deltas(), r.deltas(), c.behaviour(), r.behaviour());
            }
        }
    }
    let passed = |cells: &BTreeMap<(Situation, u8), Cell>| cells.values().filter(|c| c.n > 0 && c.passes()).count();
    let total = |cells: &BTreeMap<(Situation, u8), Cell>| cells.values().filter(|c| c.n > 0).count();
    let _ = writeln!(out, "\n{variant}: {} of {} (situation, strength) pass their outcome gates.", passed(&cand), total(&cand));
    if !same {
        let _ = writeln!(out, "standard: {} of {}.", passed(&refr), total(&refr));
    }
    out
}
