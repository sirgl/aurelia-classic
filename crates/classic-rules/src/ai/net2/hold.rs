//! The hold of the cities of `net2` (`layout-defence` only, `TurnBrief::layout_defence`; work/nets/layout-defence):
//! how much of the army each own city keeps, from the place of the city on the map and the watch's alarm. Per city and
//! rival a front (`weights::FRONT`: the rival's inputs with R1 `near` read from this city, its concepts), per city a
//! hold (`weights::HOLD`: the capital, the largest front, the alarm, the seat's fall), the gold kept for a defender of
//! a city in the watch short of its holders, and the rivals at peace whose army the next watch counts whole. The
//! executors read `plan::Hold`; nothing here moves a unit, orders a build or spends gold.

use crate::ai::planner::TurnBrief;
use crate::ai::state::{AiState, Reservation};
use crate::ai::strategy_net::layers::ramp;
use crate::ai::strategy_net::Milli;
use crate::ids::{PlayerId, TileId};
use crate::knowledge::observation::{AIObservation, OwnCity};
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
    let chest = chest(obs, brief, state, rules, &cities);
    Hold { cities, chest, whole: whole(pass) }
}

/// The rivals at peace whose army near our cities the next watch counts whole (`gate::STACK_WHOLE` on `menace`).
fn whole(pass: &Pass) -> Vec<PlayerId> {
    pass.rivals.iter().filter(|r| r.inputs[r::AT_WAR] == 0 && r.inputs[r::PACT] == 0 && r.concepts[q::MENACE] >= gate::STACK_WHOLE.0).map(|r| r.rival).collect()
}

/// The holders beyond the standing garrison a hold level asks for (`gate::HOLD_ONE`, `gate::HOLD_TWO`).
pub fn extra_of(level: Milli) -> u8 {
    if level >= gate::HOLD_THREE.0 {
        3
    } else if level >= gate::HOLD_TWO.0 {
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
    row[c::DANGER] = if brief.endangered(city.tile) { 1000 } else { 0 };
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

/// The gold kept from the other purchases while a city in the watch is short of the holders its hold asks for: the
/// rules' price of buying one defender of the best melee kind there (the most held such city), so that the treasury
/// grows to it instead of going to tiles (`planner_cities::hold_orders` buys it once it can); 0 without such a city.
fn chest(obs: &AIObservation<'_>, brief: &TurnBrief, state: &AiState, rules: &CompiledRuleset, cities: &[CityHold]) -> u32 {
    let wanted = |c: &CityHold| crate::ai::planner_units::garrison_with(brief, state.strategy.current, usize::from(c.extra));
    let short = |c: &&CityHold| c.extra > 0 && brief.endangered(c.tile) && state.holders(Reservation::Garrison(c.tile), None) < wanted(c);
    let Some(city) = cities.iter().filter(short).max_by_key(|c| (c.level, std::cmp::Reverse(c.city))) else { return 0 };
    let Some(own) = obs.own_cities.iter().find(|c| c.id == city.city) else { return 0 };
    let Some(kind) = crate::ai::operations::Arsenal::of(brief, rules).melee else { return 0 };
    let item = crate::ids::ProductionItem::Unit(kind);
    obs.view
        .own_production_cost(own.handle, item)
        .zip(crate::effects::city::hurry_cost_modifier(rules, item))
        .and_then(|(cost, modifier)| crate::world::city::hurry_price(cost, modifier))
        .unwrap_or(0)
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

    fn hold(capital: bool, front: Milli, danger: bool) -> Milli {
        let mut row = [0; c::N];
        row[c::CAPITAL] = if capital { 1000 } else { 0 };
        row[c::FRONT] = front;
        row[c::DANGER] = if danger { 1000 } else { 0 };
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

    /// The executors' counts: an interior city keeps the standing garrison, the capital on half a front one more and
    /// on a full front two; a frontier city only with the alarm: one at peace, three at war; a capital in the watch two.
    #[test]
    fn the_hold_asks_the_capital_first_and_the_alarm_most() {
        assert_eq!(extra_of(hold(false, 0, false)), 0, "interior");
        assert_eq!(extra_of(hold(true, 0, false)), 0, "a capital with no front");
        assert_eq!(extra_of(hold(true, 550, false)), 1, "the capital on half a front");
        assert_eq!(extra_of(hold(true, 1000, false)), 2, "the capital on a full front");
        assert_eq!(extra_of(hold(false, 1000, false)), 0, "a frontier city at war, no alarm");
        assert_eq!(extra_of(hold(false, 0, true)), 1, "a city in the watch at peace");
        assert_eq!(extra_of(hold(true, 0, true)), 2, "the capital in the watch at peace");
        assert_eq!(extra_of(hold(false, 1000, true)), 3, "a frontier city in the watch at war");
    }
}
