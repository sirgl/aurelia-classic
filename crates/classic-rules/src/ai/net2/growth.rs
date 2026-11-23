//! The early growth's levers of `net2` (`early-growth` only, `TurnBrief::early_growth`; work/nets/early-growth): the
//! opening's cities (`weights::OPENING_ROOM`, `OPENING`) and the price tilts they and the luxuries waiting for a worker
//! give (`weights::EXPANSION_TILT`, `WORKER_TILT`), the cities whose flat garrison waits (`weights::GARRISON`), the gold
//! the tiles leave for a growth purchase and the heads that may spend it (`weights::BUY_CHEST`, `CHEST_ITEM`), the jobs
//! the workers have ahead (`weights::WORKER_SHORT`, `JOBS_CAP`) and the site head's weight of the shared land
//! (`weights::site::SHARED`). The readings here count what the observation shows; every number that decides is a
//! weight. The executors read `plan::Growth`; nothing here orders a unit or spends a gold.

use crate::ai::planner::{role_of, AiRole, TurnBrief};
use crate::ai::strategy_net::weights::{fe, pd};
use crate::ai::strategy_net::{Intent, Milli};
use crate::ids::{EntityId, ProductionItem, UnitKindId};
use crate::knowledge::observation::{AIObservation, OwnCity};
use crate::rules::effects::{Modifier, YieldKind};
use crate::rules::CompiledRuleset;
use crate::world::player::PlayerKind;

use super::concepts::{fire, Frame};
use super::levers::{city_row, Pass};
use super::plan::{Growth, Inputs, Part, Parts, SeatInputs};
use super::weights::{self as w, c, gate, gb, gc, gi, gj, go, gw};

/// The levers of the turn.
pub fn plan(obs: &AIObservation<'_>, brief: &TurnBrief, state: &crate::ai::state::AiState, rules: &CompiledRuleset, pass: &Pass, inputs: &Inputs) -> Growth {
    let at_war = if obs.relations.iter().any(|r| r.at_war && r.kind == PlayerKind::Major) { 1000 } else { 0 };
    let camps = crate::ai::camps::known_camps(obs);
    let mean = mean_production(obs);
    let mut garrison_waits = Vec::new();
    for city in &inputs.cities {
        let Some(own) = obs.own_cities.iter().find(|o| o.id == city.city) else { continue };
        let guard = city_row(&pass.reads, city);
        let capital = inputs.capital == Some(own.tile);
        let row = garrison_row(brief, own, &guard, &camps, mean, at_war, capital);
        if fire(&w::GARRISON, &Frame { c: &row, s: &pass.seat, ..Frame::default() }) < gate::GARRISON.0 {
            garrison_waits.push(own.tile);
        }
    }
    let chest_heads = chest_heads(obs, rules);
    let waiting = waiting_price(obs, rules, &chest_heads);
    let row = buy_row(waiting.is_some(), at_war);
    let level = fire(&w::BUY_CHEST, &Frame { c: &row, s: &pass.seat, ..Frame::default() });
    let chest = waiting.map_or(0, |price| (i64::from(price) * level / 1000) as u32);
    Growth { garrison_waits, chest, chest_heads, shared: w::site::SHARED.0, jobs_ahead: jobs_ahead(obs, brief, state, rules) }
}

/// Sets the opening's inputs of the seat (`g::OPENING`, `g::OPENING_GAP`, `g::LUX_OPEN`) after the intent.
pub fn read_opening(obs: &AIObservation<'_>, rules: &CompiledRuleset, features: &[Milli; fe::N], parts: Parts, intent: &Intent, seat: &mut SeatInputs) {
    set_opening(opening_row(obs, features, parts, intent), seat);
    seat.lux_open = lux_open(obs, rules);
}

/// `weights::go` of the seat: its cities, the free sites, the cities its happiness pays for, the style and the size
/// model's new cities.
pub fn opening_row(obs: &AIObservation<'_>, features: &[Milli; fe::N], parts: Parts, intent: &Intent) -> [Milli; go::N] {
    let rates = crate::ai::value::Unhappiness::of(obs);
    let content = (i64::from(obs.economy.turn_happiness).max(0) * 100 / (rates.city + rates.citizen).max(1)).max(0);
    let sites = features[fe::ROOM] * crate::ai::strategy_net::features::ROOM_SITES as i64 / 1000;
    let tall = if intent.tall_real && parts.plays(Part::Strategy) { 1000 } else { 0 };
    let mut row = [0; go::N];
    row[go::CITIES] = obs.own_cities.len() as Milli * 250;
    row[go::SITES] = sites * 250;
    row[go::CONTENT] = content * 250;
    row[go::TALL] = tall;
    row[go::MODEL] = Milli::from(intent.more_cities) * 250;
    row
}

/// `OPENING_ROOM` and `OPENING` on the opening's row, into the seat's `opening_gap` and `opening`.
pub fn set_opening(mut row: [Milli; go::N], seat: &mut SeatInputs) {
    row[go::ROOM] = fire(&w::OPENING_ROOM, &Frame { c: &row, ..Frame::default() });
    seat.opening_gap = row[go::ROOM];
    seat.opening = fire(&w::OPENING, &Frame { c: &row, ..Frame::default() });
}

/// The opening's settlers and the luxuries' workers on the strategist's price tilts (`weights::EXPANSION_TILT`,
/// `WORKER_TILT`).
pub fn tilt_prices(seat: &SeatInputs, price_tilt: &mut [Milli; pd::N]) {
    let g = seat.row();
    price_tilt[pd::EXPANSION] += fire(&w::EXPANSION_TILT, &Frame { g: &g, ..Frame::default() });
    price_tilt[pd::WORKER] += fire(&w::WORKER_TILT, &Frame { g: &g, ..Frame::default() });
}

/// `weights::g::LUX_OPEN`: the luxury kinds the empire has no copy of lying on its own tiles (within 3 of a city)
/// without their improvement, as the player sees them, less its workers alive or queued, 500 a kind up to two.
fn lux_open(obs: &AIObservation<'_>, rules: &CompiledRuleset) -> Milli {
    use crate::rules::content::ResourceClass;
    let mut kinds: Vec<crate::ids::ResourceId> = Vec::new();
    for city in &obs.own_cities {
        for tile in crate::math::hex_disc(obs.view.dimensions(), city.tile, 3) {
            let Some(s) = obs.view.tile(tile).snapshot().copied() else { continue };
            let Some(resource) = s.resource.filter(|_| s.owner == Some(obs.player) && !s.city) else { continue };
            let Some(def) = rules.resources.get(resource.0 as usize) else { continue };
            let improved = s.improvement.is_some_and(|i| def.improvements.contains(&i));
            if def.class == Some(ResourceClass::Luxury) && !improved && obs.economy.resources.total(resource) == 0 && !kinds.contains(&resource) {
                kinds.push(resource);
            }
        }
    }
    (kinds.len().saturating_sub(land_workers(obs, rules)).min(2) * 500) as Milli
}

/// The builders of the land the empire has, alive or queued.
fn land_workers(obs: &AIObservation<'_>, rules: &CompiledRuleset) -> usize {
    let alive = obs.own_units.iter().filter(|u| land_builder(rules, u.kind)).count();
    let queued = obs.own_cities.iter().flat_map(|c| c.queue.iter()).filter(|i| matches!(i, ProductionItem::Unit(k) if land_builder(rules, *k))).count();
    alive + queued
}

fn land_builder(rules: &CompiledRuleset, kind: UnitKindId) -> bool {
    rules.units.get(kind.0 as usize).is_some_and(|d| d.builder && d.domain == crate::rules::content::Domain::Land)
}

/// The jobs the workers have ahead (`Growth::jobs_ahead`) while the empire lacks land workers (`weights::WORKER_SHORT`):
/// per own city, its owned unimproved land tiles in the work radius with a job the known technologies allow
/// (`planner_workers::open_jobs`), up to the city's `weights::JOBS_CAP`.
pub fn jobs_ahead(obs: &AIObservation<'_>, brief: &TurnBrief, state: &crate::ai::state::AiState, rules: &CompiledRuleset) -> u32 {
    let mut row = [0; gw::N];
    row[gw::CITIES] = obs.own_cities.len() as Milli * 1000;
    row[gw::WORKERS] = land_workers(obs, rules) as Milli * 1000;
    if fire(&w::WORKER_SHORT, &Frame { c: &row, ..Frame::default() }) < gate::WORKER_SHORT.0 {
        return 0;
    }
    let mut total = 0;
    for city in &obs.own_cities {
        let mut row = [0; gj::N];
        row[gj::CITIZENS] = Milli::from(city.population) * 1000;
        let cap = fire(&w::JOBS_CAP, &Frame { c: &row, ..Frame::default() }).max(0) / 1000;
        total += open_tiles(obs, brief, state, rules, city, cap as u32);
    }
    total
}

/// The city's owned unimproved land tiles with a job, counted up to `cap` (each costs the worker planner's job search).
fn open_tiles(obs: &AIObservation<'_>, brief: &TurnBrief, state: &crate::ai::state::AiState, rules: &CompiledRuleset, city: &OwnCity, cap: u32) -> u32 {
    let mut open = 0;
    for tile in crate::math::hex_disc(obs.view.dimensions(), city.tile, crate::world::city::CITY_WORK_RADIUS) {
        if open >= cap {
            break;
        }
        let bare = obs.view.tile(tile).snapshot().is_some_and(|s| s.owner == Some(obs.player) && s.improvement.is_none() && !s.city);
        if bare {
            open += crate::ai::planner_workers::open_jobs(&obs.view, rules, brief, state.strategy.current, std::iter::once(tile));
        }
    }
    open.min(cap)
}

/// `weights::gc` of one city.
pub fn garrison_row(brief: &TurnBrief, city: &OwnCity, guard: &[Milli; c::N], camps: &[crate::ids::TileId], mean: i64, at_war: Milli, capital: bool) -> [Milli; gc::N] {
    let mut row = [0; gc::N];
    row[gc::LOW_PRODUCTION] = if i64::from(city.income.production) * 1000 < mean { 1000 } else { 0 };
    row[gc::CAMP_NEAR] = if camps.iter().any(|t| brief.dims.distance(*t, city.tile).is_some_and(|d| d <= crate::ai::camps::CAMP_REACH)) { 1000 } else { 0 };
    row[gc::STACK_MENACE] = guard[c::STACK_MENACE];
    row[gc::DANGER] = guard[c::DANGER];
    row[gc::DISTRUST_NEAR] = guard[c::DISTRUST_NEAR];
    row[gc::AT_WAR] = at_war;
    row[gc::CAPITAL] = if capital { 1000 } else { 0 };
    row
}

/// `weights::gb` of the seat.
pub fn buy_row(waiting: bool, at_war: Milli) -> [Milli; gb::N] {
    [if waiting { 1000 } else { 0 }, at_war]
}

/// The mean production of the empire's cities, thousandths of a hammer.
fn mean_production(obs: &AIObservation<'_>) -> i64 {
    let n = obs.own_cities.len().max(1) as i64;
    obs.own_cities.iter().map(|c| i64::from(c.income.production.max(0))).sum::<i64>() * 1000 / n
}

/// The cities whose queue head may spend the growth chest (`weights::CHEST_ITEM` at least `gate::CHEST_ITEM`).
fn chest_heads(obs: &AIObservation<'_>, rules: &CompiledRuleset) -> Vec<EntityId> {
    let mut heads = Vec::new();
    for city in &obs.own_cities {
        let Some(&item) = city.queue.first() else { continue };
        if fire(&w::CHEST_ITEM, &Frame { c: &item_row(rules, item), ..Frame::default() }) >= gate::CHEST_ITEM.0 {
            heads.push(city.id);
        }
    }
    heads
}

/// `weights::gi` of one production item.
pub fn item_row(rules: &CompiledRuleset, item: ProductionItem) -> [Milli; gi::N] {
    let mut row = [0; gi::N];
    match item {
        ProductionItem::Unit(kind) => {
            row[gi::WORKER] = if land_builder(rules, kind) { 1000 } else { 0 };
            row[gi::SETTLER] = if role_of(rules, kind) == AiRole::Settler { 1000 } else { 0 };
        }
        ProductionItem::Building(b) => {
            row[gi::SCIENCE] = if science_building(rules, b) { 1000 } else { 0 };
        }
        ProductionItem::Project(_) => {}
    }
    row
}

/// A building, not a wonder, with an effect that yields science.
fn science_building(rules: &CompiledRuleset, building: crate::ids::BuildingKindId) -> bool {
    let Some(d) = rules.buildings.get(usize::from(building.0)) else { return false };
    !d.wonder && crate::ai::strategy_net::cell::effects_of(&d.effects).any(|e| yields_science(&e.modifier))
}

fn yields_science(modifier: &Modifier) -> bool {
    matches!(
        modifier,
        Modifier::AddYield { yield_kind: YieldKind::Science, .. }
            | Modifier::AddYieldPerPopulation { yield_kind: YieldKind::Science, .. }
            | Modifier::AddYieldPercent { yield_kind: YieldKind::Science, .. }
    )
}

/// The hurry price of the cheapest chest head (`chest_heads`) with `gate::CHEST_TURNS` turns or more left. None: no
/// such head.
fn waiting_price(obs: &AIObservation<'_>, rules: &CompiledRuleset, heads: &[EntityId]) -> Option<u32> {
    let mut best: Option<u32> = None;
    for city in obs.own_cities.iter().filter(|c| heads.contains(&c.id)) {
        let Some(&item) = city.queue.first() else { continue };
        if crate::ai::planner_cities::turns(obs, rules, city, item) < obs.speed.pace_rounds(gate::CHEST_TURNS.0).max(1) {
            continue;
        }
        if let Some(price) = obs.view.own_hurry_price(city.handle) {
            best = Some(best.map_or(price, |b| b.min(price)));
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::super::concepts::{fire, Frame};
    use super::super::levers::cities_of;
    use super::super::plan::SeatInputs;
    use super::super::weights::{self as w, gate, gb, gc, gj, go, gw, s};
    use crate::ai::strategy_net::weights::{fe, kc, pd};
    use crate::ai::strategy_net::Milli;

    fn garrison(row: [Milli; gc::N]) -> Milli {
        fire(&w::GARRISON, &Frame { c: &row, s: &[0; s::N], ..Frame::default() })
    }

    fn city(low: Milli, camp: Milli, danger: Milli, distrust: Milli, war: Milli, capital: Milli) -> [Milli; gc::N] {
        let mut row = [0; gc::N];
        (row[gc::LOW_PRODUCTION], row[gc::CAMP_NEAR], row[gc::DANGER]) = (low, camp, danger);
        (row[gc::DISTRUST_NEAR], row[gc::AT_WAR], row[gc::CAPITAL]) = (distrust, war, capital);
        row
    }

    #[test]
    fn a_quiet_low_city_waits_for_its_garrison_but_the_capital_does_not() {
        let gate = gate::GARRISON.0;
        assert!(garrison(city(0, 0, 0, 0, 0, 0)) >= gate, "a city at the mean production keeps today's garrison");
        assert!(garrison(city(1000, 0, 0, 0, 0, 0)) < gate, "below the mean and quiet: infrastructure first");
        assert!(garrison(city(1000, 0, 0, 0, 0, 1000)) >= gate, "the capital keeps its garrison");
        assert!(garrison(city(1000, 1000, 0, 0, 0, 0)) >= gate, "a camp in reach");
        assert!(garrison(city(1000, 0, 1000, 0, 0, 0)) >= gate, "the watch's threat");
        assert!(garrison(city(1000, 0, 0, 0, 1000, 0)) >= gate, "at war");
        assert!(garrison(city(1000, 0, 0, 700, 0, 0)) < gate && garrison(city(1000, 0, 0, 800, 0, 0)) >= gate, "a distrusted neighbour above 715");
    }

    #[test]
    fn the_chest_waits_for_a_growth_item_at_peace() {
        let seat = [0; s::N];
        let chest = |waiting: i64, war: i64| {
            let mut row = [0; gb::N];
            row[gb::WAITING] = waiting;
            row[gb::AT_WAR] = war;
            fire(&w::BUY_CHEST, &Frame { c: &row, s: &seat, ..Frame::default() })
        };
        assert_eq!((chest(0, 0), chest(1000, 0), chest(1000, 1000)), (0, 1000, 0));
    }

    /// The opening's row of a real tall seat: `cities` cities, `sites` free sites, happiness for `content` more, the
    /// size model's `model` new ones.
    fn opening(cities: Milli, sites: Milli, content: Milli, tall: bool, model: Milli) -> SeatInputs {
        let mut row = [0; go::N];
        (row[go::CITIES], row[go::SITES], row[go::CONTENT], row[go::MODEL]) = (cities * 250, sites * 250, content * 250, model * 250);
        row[go::TALL] = if tall { 1000 } else { 0 };
        let mut seat = SeatInputs::default();
        super::set_opening(row, &mut seat);
        seat
    }

    /// The new cities `CITY_CAP` gives a content seat with nothing else in its frame.
    fn more_cities(seat: &SeatInputs) -> u32 {
        let (mut f, k, a, y) = ([0; fe::N], [0; kc::N], [0; w::a::N], [0; w::y::N]);
        f[fe::HAPPY] = 600; // content: `CITY_CAP`'s headroom term reads nothing
        cities_of(fire(&w::CITY_CAP, &Frame { f: &f, k: &k, g: &seat.row(), a: &a, y: &y, ..Frame::default() }))
    }

    /// A real tall seat at 2 cities with 3 free sites and the happiness for 3 more founds the opening's 2 more (up to
    /// four) beyond the model's none; the sites, the happiness and the model's own cities each hold it back; a seat
    /// off the real tall style founds none.
    #[test]
    fn the_opening_founds_up_to_four_cities_the_land_and_the_happiness_allow() {
        assert_eq!(more_cities(&opening(2, 3, 3, true, 0)), 2, "two more up to four");
        assert_eq!(more_cities(&opening(2, 1, 3, true, 0)), 1, "one free site");
        assert_eq!(more_cities(&opening(2, 3, 0, true, 0)), 0, "no happiness for a city");
        assert_eq!(more_cities(&opening(4, 3, 3, true, 0)), 0, "four cities: the opening is done");
        assert_eq!(more_cities(&opening(2, 3, 3, false, 0)), 0, "not the real tall style");
        let model = opening(2, 3, 3, true, 1);
        assert_eq!((model.opening, model.opening_gap), (250, 500), "the model's one city is not the opening's again");
    }

    /// The opening's cities to found lift the settler's price, a luxury waiting lifts the worker's.
    #[test]
    fn the_opening_and_the_luxuries_tilt_the_settler_and_the_worker() {
        let mut tilt = [0; pd::N];
        super::tilt_prices(&SeatInputs::default(), &mut tilt);
        assert_eq!((tilt[pd::EXPANSION], tilt[pd::WORKER]), (0, 0), "nothing waiting, no tilt");
        let seat = SeatInputs { lux_open: 500, ..opening(2, 3, 3, true, 0) };
        super::tilt_prices(&seat, &mut tilt);
        assert_eq!(tilt[pd::EXPANSION], 3000, "two opening cities: +1.5 a city");
        assert_eq!(tilt[pd::WORKER], 2000, "one luxury kind waiting: +2");
    }

    #[test]
    fn the_jobs_ahead_are_counted_while_a_city_lacks_its_worker_up_to_two_citizens_more() {
        let short = |cities: Milli, workers: Milli| {
            let mut row = [0; gw::N];
            (row[gw::CITIES], row[gw::WORKERS]) = (cities * 1000, workers * 1000);
            fire(&w::WORKER_SHORT, &Frame { c: &row, ..Frame::default() }) >= gate::WORKER_SHORT.0
        };
        assert!(short(2, 1) && short(1, 0), "a city without its worker");
        assert!(!short(2, 2) && !short(1, 3), "a worker a city");
        let cap = |citizens: Milli| {
            let mut row = [0; gj::N];
            row[gj::CITIZENS] = citizens * 1000;
            fire(&w::JOBS_CAP, &Frame { c: &row, ..Frame::default() }) / 1000
        };
        assert_eq!((cap(1), cap(4)), (3, 6), "the worked tiles and two more");
    }
}
