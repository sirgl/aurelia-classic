//! Minimal faith and religion AI (PLAN M6 "AI for every mechanic"), from the
//! observation only (`AIObservation::faith`). Deterministic: the pantheon by
//! the score of its effects on the empire's land and situation
//! (`ai::pantheon`); the other beliefs, with `net2`'s part `Beliefs`, by what
//! their effects are worth at the strategy plan's prices (the belief head,
//! `strategy_net::beliefs`), else by the fixed preference lists of the seats
//! before it (`ai::belief_lists`); ties by table order.
//!
//! Strategy (own, simple):
//! - found a pantheon as soon as the pool pays for one, the free belief that
//!   pays most here (`ai::pantheon::choose_pantheon`);
//! - a Great Prophet founds a religion in the city it stands in (or walks
//!   to the capital), later enhances it, else spreads it to the nearest own
//!   or seen city that does not follow it, else builds a Holy Site;
//! - without faith and a pantheon, the capital queues the cheapest faith
//!   building (a Shrine: built early, for the pantheon);
//! - add a Reformation belief when a policy allows it;
//! - buy a missionary when the pool covers it and fewer than two are out;
//!   missionaries spread to the nearest city not following the religion
//!   that they can reach over the known map.
//!
//! With the missionaries' stop priced (`PricedStops::missionaries`) a
//! missionary or prophet goes to the city it reaches first by its own way of
//! moving, rival territory included (`kernel::world::territory::unit_may_enter`),
//! and stops next to it (a foreign city's tile is not entered; the rules
//! spread from a neighbouring tile); the strength it loses in rival land is
//! the rules' price. A missionary is bought only while such a city exists.

use crate::ai::planner::DecisionBudget;
use crate::ai::planner_map::{explore, explored_path, find_path, step_points, Blockers, KnownMap, MapScratch, PathOutcome, SearchStats, Visit};
use crate::classic::effects::ClassicYield;
use crate::command::Command;
use crate::ids::{BeliefId, ProductionItem, TileId, UnitHandle, UnitKindId};
use crate::knowledge::observation::{AIObservation, OwnUnit};
use crate::rules::content::{ReligionId, UnitAbility};
use crate::rules::CompiledRuleset;
use crate::world::religion::Creed;

/// Missionaries an AI keeps out at most.
pub const MISSIONARIES_WANTED: usize = 2;

use crate::ai::belief_lists::{ENHANCERS, FOLLOWERS, FOUNDERS};

/// Where the founder, follower, enhancer, reformation and bonus beliefs come from.
#[derive(Debug, Clone, Copy, Default)]
pub enum BeliefSource<'p> {
    /// The fixed preference lists (`belief_lists`): every seat without `net2`'s part `Beliefs`. With the
    /// strategy plan and the prices, the belief head is traced beside them (`strategy_net::trace::belief`),
    /// never played.
    #[default]
    Lists,
    ListsTraced(&'p crate::ai::strategy_net::Plan, &'p crate::ai::value::Prices),
    /// The belief head at the strategy plan's prices (`strategy_net::beliefs`).
    Head(&'p crate::ai::strategy_net::Plan, &'p crate::ai::value::Prices),
}

/// The belief choices of one turn: the head's model and reach are read once, at the first choice.
struct Beliefs<'o, 'a, 'p> {
    obs: &'o AIObservation<'a>,
    rules: &'o CompiledRuleset,
    source: BeliefSource<'p>,
    read: std::cell::OnceCell<(crate::ai::strategy_net::model::Model, crate::ai::strategy_net::Base, crate::ai::strategy_net::beliefs::Reach)>,
}

impl<'o, 'a, 'p> Beliefs<'o, 'a, 'p> {
    fn new(obs: &'o AIObservation<'a>, rules: &'o CompiledRuleset, source: BeliefSource<'p>) -> Self {
        Self { obs, rules, source, read: std::cell::OnceCell::new() }
    }

    /// The belief of `slot` among `free` (none of `taken`): the head's, else the list's (`list`; empty: the
    /// first free one). `founders`: the founder beliefs the religion holds or takes with it.
    fn pick(&self, slot: &str, free: &[BeliefId], taken: &[BeliefId], list: &[&str], founders: &[BeliefId]) -> Option<BeliefId> {
        let left: Vec<BeliefId> = free.iter().copied().filter(|b| !taken.contains(b)).collect();
        let listed = crate::ai::belief_lists::pick(self.rules, &left, list);
        let (plan, prices, plays) = match self.source {
            BeliefSource::Head(plan, prices) => (plan, prices, true),
            BeliefSource::ListsTraced(plan, prices) if crate::ai::strategy_net::trace::enabled() => (plan, prices, false),
            _ => return listed,
        };
        let (model, base, reach) = self.read.get_or_init(|| {
            use crate::ai::strategy_net::{beliefs::Reach, model::Model, Base};
            (Model::of_observation_for_beliefs(self.obs, self.rules), Base::of(prices), Reach::of(self.obs, self.rules))
        });
        let head = crate::ai::strategy_net::beliefs::Head::new(model, self.rules, plan, base, *reach);
        let (best, scored) = head.choose(&left, &[], founders);
        crate::ai::strategy_net::trace::belief(self.obs.player.0, self.obs.round, slot, best, listed, &scored, self.rules);
        if plays {
            best
        } else {
            listed
        }
    }

    /// The founder beliefs of the player's religion.
    fn founders(&self) -> Vec<BeliefId> {
        let faith = &self.obs.faith.overview;
        let ours = faith.religion.and_then(|r| faith.religions.iter().find(|s| s.religion == r));
        ours.map(|s| s.beliefs.iter().copied().filter(|b| self.category(*b) == Some(crate::classic::content::BeliefCategory::Founder)).collect()).unwrap_or_default()
    }

    fn category(&self, b: BeliefId) -> Option<crate::classic::content::BeliefCategory> {
        self.rules.module.beliefs.get(usize::from(b.0)).and_then(|d| d.category)
    }
}

/// The next faith decisions of the observed player: at most one per kind
/// of decision (pantheon, prophet, reformation, purchase) and one per
/// missionary. Every command is checked by the core; a refused one costs
/// nothing.
pub fn religion_commands(obs: &AIObservation<'_>, rules: &CompiledRuleset) -> Vec<Command> {
    religion_commands_with(obs, rules, false, None, BeliefSource::Lists)
}

/// `religion_commands`; with `priced` the missionaries' stop priced
/// (`PricedStops::missionaries`); with `fix` the missionaries, the
/// inquisitors and their purchase priced by the rules' formulas at those
/// prices (`ai::religion_fix`); the beliefs from `source`.
pub fn religion_commands_with(
    obs: &AIObservation<'_>,
    rules: &CompiledRuleset,
    priced: bool,
    fix: Option<&crate::ai::value::Prices>,
    source: BeliefSource<'_>,
) -> Vec<Command> {
    let mut out = Vec::new();
    let faith = &obs.faith.overview;
    let beliefs = Beliefs::new(obs, rules, source);
    if faith.can_found_pantheon {
        if let Some(belief) = crate::ai::pantheon::choose_pantheon(obs, rules, &faith.free_pantheon) {
            out.push(Command::FoundPantheon {
                player: obs.player,
                belief,
            });
        }
    }
    if faith.can_add_reformation {
        if let Some(belief) = beliefs.pick("reformation", &faith.free_reformation, &[], &[], &beliefs.founders()) {
            out.push(Command::AddReformationBelief {
                player: obs.player,
                belief,
            });
        }
    }
    let mut scratch = MapScratch::default();
    for unit in &obs.own_units {
        if let Some(cmd) = prophet_command(obs, rules, unit, priced, &mut scratch, &beliefs) {
            out.push(cmd);
        } else if fix.is_some() {
            continue;
        } else if let Some(cmd) = missionary_command(obs, rules, unit, priced, &mut scratch) {
            out.push(cmd);
        }
    }
    match fix {
        Some(prices) => out.extend(crate::ai::religion_fix::commands(obs, rules, prices)),
        None => out.extend(purchase_command(obs, rules, priced, &mut scratch)),
    }
    out.extend(shrine_command(obs, rules));
    out
}

/// Faith a building gives in its city (flat yield effects).
fn building_faith(rules: &CompiledRuleset, kind: crate::ids::BuildingKindId) -> i32 {
    rules.buildings[kind.0 as usize]
        .effects
        .passive_city
        .iter()
        .map(|e| match e.modifier {
            crate::rules::effects::Modifier::AddYield {
                yield_kind: crate::rules::effects::YieldKind::Module(ClassicYield::Faith),
                amount,
            } => amount,
            _ => 0,
        })
        .sum()
}

/// Cities the empire founds before it queues a shrine for a pantheon.
const SHRINE_AFTER_CITIES: usize = 2;
/// The round after which a lone capital queues its shrine anyway (a round of a Standard game: another speed reads its round as the same share of
/// its game, `SpeedValues::standard_round`).
const SHRINE_LATEST_ROUND: u32 = 60;

/// EnqueueProduction of a faith building in the first city while the
/// player makes no faith, has no pantheon and pantheons are left. A city
/// with an empty queue is left to the planner.
fn shrine_command(obs: &AIObservation<'_>, rules: &CompiledRuleset) -> Option<Command> {
    let faith = &obs.faith.overview;
    if faith.per_turn > 0 || faith.pantheon.is_some() || faith.free_pantheon.is_empty() {
        return None;
    }
    // A lone capital builds its settler first: a shrine queued behind the
    // garrison kept the second city waiting for ten rounds (the bench's
    // pace table); the pantheon comes with the second city or by round 60.
    if obs.own_cities.len() < SHRINE_AFTER_CITIES && obs.speed.standard_round(obs.round) < SHRINE_LATEST_ROUND {
        return None;
    }
    let city = obs.own_cities.iter().min_by_key(|c| c.id)?;
    if city.queue.is_empty() || city.queue.len() >= crate::world::city::MAX_QUEUE {
        return None;
    }
    let kind = crate::ai::happiness::cheapest_building(obs, rules, city, |kind| {
        building_faith(rules, kind) > 0
    })?;
    Some(Command::EnqueueProduction {
        player: obs.player,
        city: city.handle,
        item: ProductionItem::Building(kind),
    })
}

pub(crate) fn has(rules: &CompiledRuleset, kind: UnitKindId, ability: UnitAbility) -> bool {
    rules
        .units
        .get(kind.0 as usize)
        .is_some_and(|u| u.module.abilities.contains(&ability))
}

fn prophet_command(
    obs: &AIObservation<'_>,
    rules: &CompiledRuleset,
    unit: &OwnUnit,
    priced: bool,
    scratch: &mut MapScratch,
    beliefs: &Beliefs<'_, '_, '_>,
) -> Option<Command> {
    if !has(rules, unit.kind, UnitAbility::FoundReligion) {
        return None;
    }
    let faith = &obs.faith.overview;
    if faith.religion.is_none() && faith.religions_left > 0 {
        return Some(found_or_walk(obs, rules, unit, beliefs));
    }
    let religion = faith.religion?;
    let enhanced = faith
        .religions
        .iter()
        .any(|r| r.religion == religion && r.enhanced);
    if !enhanced {
        let founders = beliefs.founders();
        let follower = beliefs.pick("follower2", &faith.free_follower, &[], &FOLLOWERS, &founders)?;
        let enhancer = beliefs.pick("enhancer", &faith.free_enhancer, &[], &ENHANCERS, &founders)?;
        return Some(Command::EnhanceReligion {
            unit: unit.handle,
            follower,
            enhancer,
        });
    }
    let spread = if priced { spread_or_go(obs, rules, unit, religion, scratch) } else { spread_or_walk(obs, unit, religion, scratch) };
    spread.or_else(|| holy_site_or_step_out(obs, rules, unit))
}

/// A Holy Site where the prophet stands if the improvement suits the tile
/// (own land outside a city, a terrain the improvement is built on); else a
/// step onto the first neighbouring tile that suits it. Nothing when none
/// does: the core would refuse the improvement every turn.
fn holy_site_or_step_out(obs: &AIObservation<'_>, rules: &CompiledRuleset, unit: &OwnUnit) -> Option<Command> {
    if holy_site_suits(obs, rules, unit, unit.tile) {
        return Some(Command::CreateGreatImprovement { unit: unit.handle });
    }
    let dims = crate::math::MapDimensions {
        width: obs.map_width,
        height: obs.map_height,
    };
    let next = dims
        .neighbors(unit.tile)
        .into_iter()
        .find(|t| holy_site_suits(obs, rules, unit, *t))?;
    Some(Command::MarchTo {
        unit: unit.handle,
        destination: next,
    })
}

/// The great person's improvement may be built on `tile`: own land outside
/// a city, on a terrain the improvement allows (`great_improvement_status`).
fn holy_site_suits(obs: &AIObservation<'_>, rules: &CompiledRuleset, unit: &OwnUnit, tile: TileId) -> bool {
    let Some(improvement) = rules
        .module
        .great_people
        .iter()
        .find(|g| g.unit == unit.kind)
        .and_then(|g| g.improvements.first())
    else {
        return false;
    };
    let Some(snapshot) = obs.view.tile(tile).snapshot().copied() else { return false };
    if snapshot.owner != Some(obs.player) || snapshot.city {
        return false;
    }
    let allowed = &rules.improvements[improvement.0 as usize].valid_terrains;
    allowed.is_empty()
        || allowed
            .iter()
            .any(|t| rules.terrains.get(t.0 as usize).and_then(|d| d.map_code) == Some(snapshot.terrain.0))
}

/// Found the religion where the prophet stands if it is an own city, else
/// walk to the first own city.
fn found_or_walk(obs: &AIObservation<'_>, rules: &CompiledRuleset, unit: &OwnUnit, beliefs: &Beliefs<'_, '_, '_>) -> Command {
    let in_city = obs.own_cities.iter().any(|c| c.tile == unit.tile);
    let faith = &obs.faith.overview;
    // The beliefs are chosen where the religion is founded (a walk chooses again in the city).
    let (founder, follower) = match in_city {
        true => {
            let founder = beliefs.pick("founder", &faith.free_founder, &[], &FOUNDERS, &[]);
            let founders: Vec<BeliefId> = founder.into_iter().collect();
            (founder, beliefs.pick("follower", &faith.free_follower, &[], &FOLLOWERS, &founders))
        }
        false => (None, None),
    };
    let religion = preferred_religion(obs, rules);
    match (in_city, founder, follower, religion) {
        (true, Some(founder), Some(follower), Some(religion)) => {
            let pantheon = match faith.pantheon {
                Some(_) => None,
                None => crate::ai::pantheon::choose_pantheon(obs, rules, &faith.free_pantheon),
            };
            let chosen = [Some(founder), Some(follower), pantheon];
            let bonus = match faith.extra_founding_beliefs {
                0 => None,
                _ => bonus_belief(obs, rules, faith, &chosen, beliefs),
            };
            Command::FoundReligion {
                unit: unit.handle,
                religion,
                founder,
                follower,
                pantheon,
                bonus,
            }
        }
        _ => {
            let target = obs.own_cities.first().map(|c| c.tile).unwrap_or(unit.tile);
            Command::MarchTo {
                unit: unit.handle,
                destination: target,
            }
        }
    }
}

/// Byzantium's bonus belief (none of the beliefs already chosen): with the
/// belief head the one of any kind worth most; else a second founder belief
/// of the preference list, else a second follower, else an enhancer, else
/// the pantheon belief that pays most.
fn bonus_belief(
    obs: &AIObservation<'_>,
    rules: &CompiledRuleset,
    faith: &crate::derived::religion::FaithOverview,
    chosen: &[Option<BeliefId>],
    beliefs: &Beliefs<'_, '_, '_>,
) -> Option<BeliefId> {
    if let BeliefSource::Head(..) = beliefs.source {
        let taken: Vec<BeliefId> = chosen.iter().flatten().copied().collect();
        let founders: Vec<BeliefId> = taken.iter().copied().filter(|b| beliefs.category(*b) == Some(crate::classic::content::BeliefCategory::Founder)).collect();
        let mut free: Vec<BeliefId> = [&faith.free_founder, &faith.free_follower, &faith.free_enhancer, &faith.free_pantheon].into_iter().flatten().copied().collect();
        free.sort_unstable_by_key(|b| b.0);
        free.dedup();
        return beliefs.pick("bonus", &free, &taken, &[], &founders);
    }
    let options = [
        (&faith.free_founder, &FOUNDERS[..]),
        (&faith.free_follower, &FOLLOWERS[..]),
        (&faith.free_enhancer, &ENHANCERS[..]),
    ];
    let left = |free: &[BeliefId]| -> Vec<BeliefId> { free.iter().copied().filter(|b| !chosen.contains(&Some(*b))).collect() };
    for (free, preferred) in options {
        if let Some(belief) = crate::ai::belief_lists::pick(rules, &left(free), preferred) {
            return Some(belief);
        }
    }
    crate::ai::pantheon::choose_pantheon(obs, rules, &left(&faith.free_pantheon))
}

/// The first religion of the table nobody founded (names are cosmetic).
fn preferred_religion(obs: &AIObservation<'_>, _rules: &CompiledRuleset) -> Option<ReligionId> {
    obs.faith.overview.free_religions.first().copied()
}

fn missionary_command(
    obs: &AIObservation<'_>,
    rules: &CompiledRuleset,
    unit: &OwnUnit,
    priced: bool,
    scratch: &mut MapScratch,
) -> Option<Command> {
    if !has(rules, unit.kind, UnitAbility::SpreadReligion) {
        return None;
    }
    let religion = obs
        .faith
        .units
        .iter()
        .find(|(h, _, _)| *h == unit.handle)
        .map(|(_, r, _)| *r)?;
    if priced {
        return spread_or_go(obs, rules, unit, religion, scratch);
    }
    spread_or_walk(obs, unit, religion, scratch)
}

/// Spread where the unit stands if that city does not follow the religion,
/// else march to the nearest known city that does not and that the unit
/// can reach (the core refuses a march without a way, every turn again).
fn spread_or_walk(
    obs: &AIObservation<'_>,
    unit: &OwnUnit,
    religion: ReligionId,
    scratch: &mut MapScratch,
) -> Option<Command> {
    let dims = crate::math::MapDimensions {
        width: obs.map_width,
        height: obs.map_height,
    };
    let creed = Some(Creed::Religion(religion));
    let mut targets = Vec::new();
    for city in &obs.faith.cities {
        if city.majority == creed || !may_enter(obs, city.owner) {
            continue;
        }
        let distance = dims.distance(unit.tile, city.tile).unwrap_or(i32::MAX);
        if distance <= 1 {
            return Some(Command::SpreadReligion { unit: unit.handle });
        }
        targets.push((distance, city.tile));
    }
    targets.sort_by_key(|(distance, _)| *distance);
    let (_, tile) = targets.into_iter().find(|(_, tile)| reachable(obs, unit, *tile, scratch))?;
    Some(Command::MarchTo {
        unit: unit.handle,
        destination: tile,
    })
}

/// The unit has a way to `to` over the player's known map, as it moves now.
fn reachable(obs: &AIObservation<'_>, unit: &OwnUnit, to: TileId, scratch: &mut MapScratch) -> bool {
    let map = KnownMap::of_unit(&obs.view, unit.handle);
    let limit = DecisionBudget::default().search();
    let outcome = find_path(&map, unit.tile, to, &Blockers::default(), scratch, limit, &mut SearchStats::default());
    matches!(outcome, PathOutcome::Found { .. })
}

/// Spread where the unit stands if a city at or next to it does not follow
/// the religion, else march to the first tile at or next to such a city
/// the unit reaches over the known map by its own way of moving
/// (`first_station`).
fn spread_or_go(obs: &AIObservation<'_>, rules: &CompiledRuleset, unit: &OwnUnit, religion: ReligionId, scratch: &mut MapScratch) -> Option<Command> {
    let map = KnownMap::of_unit(&obs.view, unit.handle);
    let station = first_station(obs, rules, &map, unit.tile, Some(unit.handle), unit.kind, religion, scratch)?;
    if station == unit.tile {
        return Some(Command::SpreadReligion { unit: unit.handle });
    }
    march_toward(obs, rules, &map, unit, station, scratch)
}

/// A march toward `station` over the way the last exploration from the
/// unit's tile found (`scratch`).
pub(crate) fn march_toward(
    obs: &AIObservation<'_>,
    rules: &CompiledRuleset,
    map: &KnownMap<'_, '_>,
    unit: &OwnUnit,
    station: TileId,
    scratch: &MapScratch,
) -> Option<Command> {
    // The march follows the way found around the units in the way, a turn's
    // moves at a time over tiles seen now, and ends where the unit may stop:
    // the core's own way to a far station may run into a unit and stop
    // there, every turn again.
    let way = explored_path(scratch, unit.tile, station)?;
    let crowded = crowded_tiles(obs, rules, Some(unit.handle), unit.kind);
    let mut spent = 0u32;
    let mut destination = None;
    for step in way.windows(2) {
        spent = spent.saturating_add(step_points(map, step[0], step[1])?);
        if destination.is_some() && (spent > u32::from(unit.moves_max) || !obs.view.is_visible(step[1])) {
            break;
        }
        if crowded.binary_search_by_key(&step[1].0, |t| t.0).is_err() {
            destination = Some(step[1]);
        }
    }
    let destination = destination?;
    Some(Command::MarchTo {
        unit: unit.handle,
        destination,
    })
}

/// The tile nearest to `from` by the price of `map`'s moves that is at or
/// next to a city seen now (its religion is known) not following
/// `religion`, any owner's (where the rules let a missionary spread it:
/// `derived::religion::spread_status`); None when no such city is reached.
/// A city remembered but not seen is no target: out of sight again it would
/// call back the unit that went to look at it. A foreign unit's tile is not
/// entered and an own unit's of the class of `kind` (but `mover`) is no
/// station (a march there stops short, every turn again).
#[allow(clippy::too_many_arguments)]
fn first_station(
    obs: &AIObservation<'_>,
    rules: &CompiledRuleset,
    map: &KnownMap<'_, '_>,
    from: TileId,
    mover: Option<UnitHandle>,
    kind: UnitKindId,
    religion: ReligionId,
    scratch: &mut MapScratch,
) -> Option<TileId> {
    let dims = crate::math::MapDimensions {
        width: obs.map_width,
        height: obs.map_height,
    };
    let creed = Some(Creed::Religion(religion));
    let mut stations: Vec<TileId> = Vec::new();
    for city in obs.faith.cities.iter().filter(|c| c.majority != creed) {
        stations.push(city.tile);
        stations.extend(dims.neighbors(city.tile));
    }
    if stations.is_empty() {
        return None;
    }
    stations.sort_unstable_by_key(|t| t.0);
    let limit = DecisionBudget::default().search();
    let crowded = crowded_tiles(obs, rules, mover, kind);
    let blockers = Blockers::new(obs.visible_foreign_units.iter().map(|u| u.tile).collect());
    let mut found = None;
    explore(map, from, &blockers, scratch, limit, &mut SearchStats::default(), &mut |tile, _| {
        let station = stations.binary_search_by_key(&tile.0, |t| t.0).is_ok();
        let free = tile == from || crowded.binary_search_by_key(&tile.0, |t| t.0).is_err();
        if station && free {
            found = Some(tile);
            Visit::Stop
        } else {
            Visit::Continue
        }
    });
    found
}

/// The tiles, sorted, where an own unit of the class of `kind` (but `mover`)
/// stands: a unit of that kind may pass but not stop there
/// (`world::stacking`).
pub(crate) fn crowded_tiles(obs: &AIObservation<'_>, rules: &CompiledRuleset, mover: Option<UnitHandle>, kind: UnitKindId) -> Vec<TileId> {
    let class = crate::world::stacking::stack_class(rules, kind);
    let mut tiles: Vec<TileId> = obs
        .own_units
        .iter()
        .filter(|u| Some(u.handle) != mover && crate::world::stacking::stack_class(rules, u.kind) == class)
        .map(|u| u.tile)
        .collect();
    tiles.sort_unstable_by_key(|t| t.0);
    tiles
}

/// The player's units may enter the land of `owner` (own land, or open
/// borders granted to the player).
pub(crate) fn may_enter(obs: &AIObservation<'_>, owner: crate::ids::PlayerId) -> bool {
    owner == obs.player
        || obs
            .relations
            .iter()
            .any(|r| r.other == owner && r.they_grant_entry && !r.at_war)
}

/// A missionary bought in the first own city following the player's
/// religion, when fewer than MISSIONARIES_WANTED are out; with `priced`
/// only while it would reach a city to spread to from there
/// (`first_station`).
fn purchase_command(obs: &AIObservation<'_>, rules: &CompiledRuleset, priced: bool, scratch: &mut MapScratch) -> Option<Command> {
    let religion = obs.faith.overview.religion?;
    let missionaries = obs
        .own_units
        .iter()
        .filter(|u| {
            has(rules, u.kind, UnitAbility::SpreadReligion)
                && !has(rules, u.kind, UnitAbility::FoundReligion)
        })
        .count();
    if missionaries >= MISSIONARIES_WANTED {
        return None;
    }
    let kind = crate::derived::religion::missionary_kind(rules)?;
    let (city, _) = obs.faith.overview.missionary_purchase?;
    if priced {
        let tile = obs.own_cities.iter().find(|c| c.handle == city)?.tile;
        first_station(obs, rules, &KnownMap::of_kind(&obs.view, kind), tile, None, kind, religion, scratch)?;
    }
    Some(Command::PurchaseWithFaith {
        player: obs.player,
        city,
        item: ProductionItem::Unit(kind),
    })
}

/// Units the AI uses for faith (for drivers that skip them in other plans).
pub fn is_religious_unit(rules: &CompiledRuleset, unit: &OwnUnit) -> bool {
    has(rules, unit.kind, UnitAbility::SpreadReligion)
        || has(rules, unit.kind, UnitAbility::RemoveHeresy)
}

/// Handles of the units `religion_commands` gives orders to.
pub fn religious_units(obs: &AIObservation<'_>, rules: &CompiledRuleset) -> Vec<UnitHandle> {
    obs.own_units
        .iter()
        .filter(|u| is_religious_unit(rules, u) || has(rules, u.kind, UnitAbility::FoundReligion))
        .map(|u| u.handle)
        .collect()
}

/// Where a seat's beliefs come from: the belief head with `net2`'s part `Beliefs` (and the strategy plan
/// and the prices it reads), else the lists — traced beside the head when the seat has a plan.
pub fn belief_source(brief: &crate::ai::planner::TurnBrief) -> BeliefSource<'_> {
    let plays = brief.net2.as_ref().is_some_and(|n| n.parts.plays(crate::ai::net2::Part::Beliefs));
    match (brief.strategy.as_deref(), brief.prices.as_ref()) {
        (Some(plan), Some(prices)) if plays => BeliefSource::Head(plan, prices),
        (Some(plan), Some(prices)) => BeliefSource::ListsTraced(plan, prices),
        _ => BeliefSource::Lists,
    }
}

/// The religion advisor of the AI driver (`ai::system`): pantheon,
/// founding, beliefs, religious units and faith purchases, in the order of
/// `religion_commands` (the first proposal has the highest priority).
#[derive(Debug, Clone, Copy, Default)]
pub struct ReligionAdvisor;

impl crate::ai::system::SystemAdvisor for ReligionAdvisor {
    fn end_of_turn(
        &self,
        input: &crate::ai::system::AdvisorInput<'_, '_>,
    ) -> Vec<crate::ai::system::Proposal> {
        let fix = input.brief.prices.as_ref().filter(|_| input.brief.religion_fix);
        let commands = religion_commands_with(input.observation, input.rules, input.brief.priced_stops.missionaries, fix, belief_source(input.brief));
        let count = commands.len() as i32;
        commands
            .into_iter()
            .enumerate()
            .map(|(i, command)| crate::ai::system::Proposal {
                priority: 10 + count - i as i32,
                command,
            })
            .collect()
    }
}
