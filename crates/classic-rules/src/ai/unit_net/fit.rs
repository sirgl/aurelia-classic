//! The numbers of the unit net that `tactician-fit` plays with (`TurnBrief::tactician_fit`,
//! classic/docs/ai-tactician-fit.md): the motive table by role and phase, the war posture's
//! prices and the operation head's thresholds, fitted on a bank of real siege positions. Every
//! other variant of the unit net (`unit-net`, `hybrid`) reads `HAND`, the hand weights of
//! `weights` themselves, so they play exactly as before.
//!
//! A development tool may replace the fitted numbers of the process (`set_process_overrides`,
//! `ai-siege-bank replay --set`) to measure a candidate; the game never does.

use std::sync::OnceLock;

use super::weights::{self, motive as mw, op as ow, MOTIVES};
use super::Milli;

/// The numbers one mind of the unit net plays with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Numbers {
    /// `weights::TABLE`'s rows, in its order.
    pub table: [[Milli; 14]; 13],
    /// The war posture's prices (`weights::prices::POSTURES[3]`): kill, city, damage, unit, risk.
    pub war: [Milli; 5],
    /// The operation head (`weights::op`).
    pub take_at: Milli,
    pub take_width: Milli,
    pub retreat_ratio: Milli,
    pub wait_cost: Milli,
    pub ready: Milli,
    pub city_defence_percent: i64,
    /// The motives' inner weights (`weights::motive`).
    pub survive_at: Milli,
    pub doom: Milli,
    pub assigned: Milli,
}

const fn rows() -> [[Milli; 14]; 13] {
    let mut out = [[0; 14]; 13];
    let mut i = 0;
    while i < 13 {
        out[i] = weights::TABLE[i].w;
        i += 1;
    }
    out
}

/// The hand weights (`weights`): what `unit-net` and `hybrid` play.
pub const HAND: Numbers = Numbers {
    table: rows(),
    war: [weights::prices::POSTURES[3].kill, weights::prices::POSTURES[3].city, weights::prices::POSTURES[3].damage, weights::prices::POSTURES[3].unit, weights::prices::POSTURES[3].risk],
    take_at: ow::TAKE_AT,
    take_width: ow::TAKE_WIDTH,
    retreat_ratio: ow::RETREAT_RATIO,
    wait_cost: ow::WAIT_COST,
    ready: ow::READY,
    city_defence_percent: ow::CITY_DEFENCE_PERCENT,
    survive_at: mw::SURVIVE_AT,
    doom: mw::DOOM,
    assigned: mw::ASSIGNED,
};

/// The fitted numbers (`tactician-fit`); see the doc for the bank, the method and what moved.
pub const FITTED: Numbers = HAND;

static OVERRIDE: OnceLock<Numbers> = OnceLock::new();

thread_local! {
    /// A development tool's candidate for the games of this thread (`enter`).
    static ACTIVE: std::cell::Cell<Option<&'static Numbers>> = const { std::cell::Cell::new(None) };
}

/// The numbers of a mind: `tactician-fit`'s (`fit`) or the hand weights.
pub fn numbers(fit: bool) -> &'static Numbers {
    if !fit {
        return &HAND;
    }
    ACTIVE.with(|a| a.get()).or_else(|| OVERRIDE.get()).unwrap_or(&FITTED)
}

/// `tactician-fit` plays `n` on this thread until the scope ends (a development tool's candidate).
pub fn enter(n: &'static Numbers) -> Scope {
    Scope { previous: ACTIVE.with(|a| a.replace(Some(n))) }
}

/// Puts the previous candidate back when dropped.
pub struct Scope {
    previous: Option<&'static Numbers>,
}

impl Drop for Scope {
    fn drop(&mut self) {
        ACTIVE.with(|a| a.set(self.previous));
    }
}

/// The row names of `weights::TABLE` as the overrides spell them (`melee_assault`).
fn row_key(i: usize) -> String {
    weights::TABLE[i].name.replace(" · ", "_").replace(' ', "_")
}

/// The scalar names and their places.
fn scalar<'n>(n: &'n mut Numbers, name: &str) -> Option<&'n mut i64> {
    let p = match name {
        "war.kill" => &mut n.war[0],
        "war.city" => &mut n.war[1],
        "war.damage" => &mut n.war[2],
        "war.unit" => &mut n.war[3],
        "war.risk" => &mut n.war[4],
        "op.take_at" => &mut n.take_at,
        "op.take_width" => &mut n.take_width,
        "op.retreat_ratio" => &mut n.retreat_ratio,
        "op.wait_cost" => &mut n.wait_cost,
        "op.ready" => &mut n.ready,
        "op.city_defence_percent" => &mut n.city_defence_percent,
        "motive.survive_at" => &mut n.survive_at,
        "motive.doom" => &mut n.doom,
        "motive.assigned" => &mut n.assigned,
        _ => return None,
    };
    Some(p)
}

/// `FITTED` with `pairs` ("name=value,…") applied: `table.<row>.<motive>` (`table.melee_assault.take`),
/// `war.<price>`, `op.<threshold>`, `motive.<weight>`. An unknown name is an error.
pub fn parse(pairs: &str) -> Result<Numbers, String> {
    let mut n = FITTED;
    for pair in pairs.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        let (name, value) = pair.split_once('=').ok_or_else(|| format!("{pair}: name=value"))?;
        let value: i64 = value.trim().parse().map_err(|e| format!("{pair}: {e}"))?;
        let name = name.trim();
        if let Some(rest) = name.strip_prefix("table.") {
            let (row, motive) = rest.rsplit_once('.').ok_or_else(|| format!("{name}: table.<row>.<motive>"))?;
            let r = (0..13).find(|i| row_key(*i) == row).ok_or_else(|| format!("unknown row {row}"))?;
            let m = MOTIVES.iter().position(|x| *x == motive).ok_or_else(|| format!("unknown motive {motive}"))?;
            n.table[r][m] = value;
            continue;
        }
        *scalar(&mut n, name).ok_or_else(|| format!("unknown number {name}"))? = value;
    }
    Ok(n)
}

/// The process plays `tactician-fit` with `FITTED` changed by `pairs` (a development tool's
/// candidate; once per process).
pub fn set_process_overrides(pairs: &str) -> Result<(), String> {
    let n = parse(pairs)?;
    OVERRIDE.set(n).map_err(|_| "the overrides are set once per process".to_owned())
}

/// Every name `parse` knows, with its value in `n` (the tools print a table from it).
pub fn names(n: &Numbers) -> Vec<(String, i64)> {
    let mut out = Vec::new();
    for r in 0..13 {
        for (m, motive) in MOTIVES.iter().enumerate() {
            out.push((format!("table.{}.{motive}", row_key(r)), n.table[r][m]));
        }
    }
    let mut copy = *n;
    for name in ["war.kill", "war.city", "war.damage", "war.unit", "war.risk", "op.take_at", "op.take_width", "op.retreat_ratio", "op.wait_cost", "op.ready", "op.city_defence_percent", "motive.survive_at", "motive.doom", "motive.assigned"] {
        if let Some(p) = scalar(&mut copy, name) {
            out.push((name.to_owned(), *p));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The hand numbers are the weights themselves: `unit-net` and `hybrid` play as before.
    #[test]
    fn the_hand_numbers_are_the_weights() {
        for (i, row) in weights::TABLE.iter().enumerate() {
            assert_eq!(HAND.table[i], row.w);
        }
        assert_eq!(numbers(false), &HAND);
        let n = parse("table.melee_assault.take=4000,op.take_at=900").unwrap();
        assert_eq!(n.table[weights::row::MELEE_ASSAULT][0], 4000);
        assert_eq!(n.take_at, 900);
        assert!(parse("op.nothing=1").is_err());
        assert_eq!(names(&HAND).len(), 13 * 14 + 14);
    }
}
