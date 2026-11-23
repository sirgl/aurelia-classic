//! The hold of the cities of `net2` (`layout-defence` only, `TurnBrief::layout_defence`; work/nets/layout-defence):
//! how much of the army each own city keeps before an enemy is in sight, from the place of the city on the map. Per
//! city and rival a front (`weights::FRONT`: the rival's inputs with R1 `near` read from this city, its concepts), per
//! city a hold (`weights::HOLD`: the capital, the largest front, the seat's fall), and the gold kept for the next
//! upgrade of a held city's guard. The executors read `plan::Hold`; nothing here moves a unit, orders a build or
//! spends gold.

use crate::ai::planner::TurnBrief;
use crate::ai::state::{AiState, UnitTask};
use crate::ai::strategy_net::layers::ramp;
use crate::ai::strategy_net::Milli;
use crate::ids::{PlayerId, TileId};
use crate::knowledge::observation::{AIObservation, OwnCity};
use crate::knowledge::public::PlayerQueries;
use crate::rules::content::BuildingKind;
use crate::rules::CompiledRuleset;

use super::concepts::{fire, Frame};
use super::inputs::InputMemory;
use super::levers::{Pass, RivalPass};
use super::plan::{CityHold, Hold};
use super::weights::{self as w, c, gate, q, r};

/// The hold of the turn: every own city's level and holders beyond the standing garrison, and the chest.
pub fn plan(obs: &AIObservation<'_>, brief: &TurnBrief, state: &AiState, pass: &Pass, memory: &InputMemory, rules: &CompiledRuleset) -> Hold {
    let known = super::rival_inputs::known_cities(obs, state, memory);
    let cities: Vec<CityHold> = obs
        .own_cities
        .iter()
        .map(|city| {
            let row = row(brief, rules, city, &known, &pass.rivals);
            let level = fire(&w::HOLD, &Frame { s: &pass.seat, c: &row, ..Frame::default() });
            CityHold { city: city.id, tile: city.tile, level, extra: extra_of(level) }
        })
        .collect();
    let chest = chest(obs, brief, state, &cities);
    Hold { cities, chest, whole: whole(pass) }
}

/// The rivals at peace whose army near our cities the next watch counts whole (`gate::STACK_WHOLE` on `menace`).
fn whole(pass: &Pass) -> Vec<PlayerId> {
    pass.rivals.iter().filter(|r| r.inputs[r::AT_WAR] == 0 && r.inputs[r::PACT] == 0 && r.concepts[q::MENACE] >= gate::STACK_WHOLE.0).map(|r| r.rival).collect()
}

/// The holders beyond the standing garrison a hold level asks for (`gate::HOLD_ONE`, `gate::HOLD_TWO`).
pub fn extra_of(level: Milli) -> u8 {
    if level >= gate::HOLD_TWO.0 {
        2
    } else if level >= gate::HOLD_ONE.0 {
        1
    } else {
        0
    }
}

/// One city's hold inputs (`weights::c`; the guard's four are not read by `HOLD`): the palace and the largest front.
fn row(brief: &TurnBrief, rules: &CompiledRuleset, city: &OwnCity, known: &[(TileId, PlayerId, bool)], rivals: &[RivalPass]) -> [Milli; c::N] {
    let mut row = [0; c::N];
    row[c::CAPITAL] = if has_palace(rules, city) { 1000 } else { 0 };
    let dist = |a: TileId, b: TileId| brief.dims.distance(a, b).unwrap_or(i32::MAX);
    for rival in rivals {
        let nearest = known.iter().filter(|(_, o, _)| *o == rival.rival).map(|(t, _, _)| dist(*t, city.tile)).min();
        let Some(d) = nearest else { continue };
        row[c::FRONT] = row[c::FRONT].max(front(near_of(d), rival));
    }
    row
}

/// R1's nearness of a city `d` tiles off (`rival_inputs::read`: 1000 at 5 tiles, 0 at 15), read from one own city.
pub fn near_of(d: i32) -> Milli {
    ramp(15 - i64::from(d), 0, 10)
}

/// `weights::FRONT` of one rival on one city: its inputs with `near` from this city, its concepts.
pub fn front(near: Milli, rival: &RivalPass) -> Milli {
    let mut inputs = rival.inputs;
    inputs[r::NEAR] = near;
    fire(&w::FRONT, &Frame { r: &inputs, q: &rival.concepts, ..Frame::default() })
}

fn has_palace(rules: &CompiledRuleset, city: &OwnCity) -> bool {
    city.buildings.iter().any(|b| rules.buildings.get(b.0 as usize).is_some_and(|d| d.kind == BuildingKind::Palace))
}

/// The gold kept for the next upgrade of a held city's guard (a city with one holder more at least): the rules' price
/// of the first offer among the guards of the most held city first; 0 when none is offered.
fn chest(obs: &AIObservation<'_>, brief: &TurnBrief, state: &AiState, cities: &[CityHold]) -> u32 {
    let mut held: Vec<&CityHold> = cities.iter().filter(|c| c.extra > 0).collect();
    held.sort_by_key(|c| (std::cmp::Reverse(c.level), c.city));
    for city in held {
        let guards = brief.units.iter().filter(|u| state.task_of(u.id).map(|t| t.task) == Some(UnitTask::Defend { city: city.tile }));
        if let Some(price) = guards.filter_map(|u| obs.view.upgrade_offer(u.handle).map(|(_, price)| price)).min() {
            return price;
        }
    }
    0
}

/// `NET2_HOLD_TRACE=<file>`: one line a turn of the hold (each city's tile, level and holders beyond the standing
/// garrison, the chest). It decides nothing.
pub fn trace(player: u8, round: u32, hold: &Hold) {
    let Some(path) = std::env::var_os("NET2_HOLD_TRACE") else { return };
    let cities: Vec<String> = hold.cities.iter().map(|c| format!("[{},{},{}]", c.tile.0, c.level, c.extra)).collect();
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        use std::io::Write as _;
        let _ = writeln!(file, "{{\"p\":{player},\"r\":{round},\"chest\":{},\"cities\":[{}]}}", hold.chest, cities.join(","));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rival(near_land: bool, soldiers: Milli, at_war: bool, pact: bool) -> RivalPass {
        let mut inputs = [0; r::N];
        inputs[r::LAND] = if near_land { 1000 } else { 0 };
        inputs[r::SOLDIERS] = soldiers;
        inputs[r::AT_WAR] = if at_war { 1000 } else { 0 };
        inputs[r::PACT] = if pact { 1000 } else { 0 };
        RivalPass { rival: PlayerId(1), inputs, concepts: [0; q::N] }
    }

    fn hold(capital: bool, front: Milli) -> Milli {
        let mut row = [0; c::N];
        row[c::CAPITAL] = if capital { 1000 } else { 0 };
        row[c::FRONT] = front;
        fire(&w::HOLD, &Frame { s: &[0; w::s::N], c: &row, ..Frame::default() })
    }

    /// R1's scale read from one city: 1000 within 5 tiles, 500 at 10, 0 from 15.
    #[test]
    fn the_nearness_is_r1s_scale() {
        assert_eq!((near_of(4), near_of(5), near_of(10), near_of(15), near_of(20)), (1000, 1000, 500, 0, 0));
    }

    /// A far rival is no front; a near land neighbour at peace with a level army is half a front; at war a full one;
    /// a pact partner none.
    #[test]
    fn a_front_is_a_near_land_rival_by_its_posture() {
        assert_eq!(front(0, &rival(true, 500, false, false)), 0, "far");
        let peace = front(1000, &rival(true, 500, false, false));
        assert!((500..=600).contains(&peace), "near land, level army, peace: {peace}");
        assert_eq!(front(1000, &rival(true, 500, true, false)), 1000, "at war");
        assert_eq!(front(1000, &rival(true, 500, false, true)), 0, "a pact partner");
        assert!(front(1000, &rival(false, 500, false, false)) < peace, "no land road");
    }

    /// The executors' counts: an interior city keeps the standing garrison, the capital on half a front one more, the
    /// capital on a full front two, a frontier city at war one.
    #[test]
    fn the_hold_asks_the_capital_first() {
        assert_eq!(extra_of(hold(false, 0)), 0, "interior");
        assert_eq!(extra_of(hold(true, 0)), 0, "a capital with no front");
        assert_eq!(extra_of(hold(true, 550)), 1, "the capital on half a front");
        assert_eq!(extra_of(hold(true, 1000)), 2, "the capital on a full front");
        assert_eq!(extra_of(hold(false, 1000)), 1, "a frontier city at war");
        assert_eq!(extra_of(hold(false, 550)), 0, "a frontier city at peace");
    }
}
