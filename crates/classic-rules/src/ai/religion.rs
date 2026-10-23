//! Minimal faith and religion AI (PLAN M6 "AI for every mechanic"), from the
//! observation only (`AIObservation::faith`). Deterministic: the pantheon by
//! the score of its effects on the empire's land and situation
//! (`ai::pantheon`), the other beliefs by fixed preference lists, ties by
//! table order.
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

use crate::ai::planner::DecisionBudget;
use crate::ai::planner_map::{find_path, Blockers, KnownMap, MapScratch, PathOutcome, SearchStats};
use crate::classic::effects::ClassicYield;
use crate::command::Command;
use crate::ids::{BeliefId, ProductionItem, TileId, UnitHandle, UnitKindId};
use crate::knowledge::observation::{AIObservation, OwnUnit};
use crate::rules::content::{ReligionId, UnitAbility};
use crate::rules::CompiledRuleset;
use crate::world::religion::Creed;

/// Missionaries an AI keeps out at most.
pub const MISSIONARIES_WANTED: usize = 2;

const FOUNDERS: [&str; 3] = [
    "core:belief/founder/tithe",
    "core:belief/founder/church-property",
    "core:belief/founder/ceremonial-burial",
];
const FOLLOWERS: [&str; 4] = [
    "core:belief/follower/pagodas",
    "core:belief/follower/cathedrals",
    "core:belief/follower/mosques",
    "core:belief/follower/feed-the-world",
];
const ENHANCERS: [&str; 3] = [
    "core:belief/enhancer/religious-texts",
    "core:belief/enhancer/missionary-zeal",
    "core:belief/enhancer/itinerant-preachers",
];

/// The next faith decisions of the observed player: at most one per kind
/// of decision (pantheon, prophet, reformation, purchase) and one per
/// missionary. Every command is checked by the core; a refused one costs
/// nothing.
pub fn religion_commands(obs: &AIObservation<'_>, rules: &CompiledRuleset) -> Vec<Command> {
    let mut out = Vec::new();
    let faith = &obs.faith.overview;
    if faith.can_found_pantheon {
        if let Some(belief) = crate::ai::pantheon::choose_pantheon(obs, rules, &faith.free_pantheon) {
            out.push(Command::FoundPantheon {
                player: obs.player,
                belief,
            });
        }
    }
    if faith.can_add_reformation {
        if let Some(belief) = faith.free_reformation.first() {
            out.push(Command::AddReformationBelief {
                player: obs.player,
                belief: *belief,
            });
        }
    }
    let mut scratch = MapScratch::default();
    for unit in &obs.own_units {
        if let Some(cmd) = prophet_command(obs, rules, unit, &mut scratch) {
            out.push(cmd);
        } else if let Some(cmd) = missionary_command(obs, rules, unit, &mut scratch) {
            out.push(cmd);
        }
    }
    if let Some(cmd) = purchase_command(obs, rules) {
        out.push(cmd);
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
/// The round after which a lone capital queues its shrine anyway.
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
    if obs.own_cities.len() < SHRINE_AFTER_CITIES && obs.round < SHRINE_LATEST_ROUND {
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

/// The first free belief of the preference list, else the first free one.
fn pick(rules: &CompiledRuleset, free: &[BeliefId], preferred: &[&str]) -> Option<BeliefId> {
    for key in preferred {
        let found = free
            .iter()
            .find(|b| rules.module.beliefs[b.0 as usize].key == *key);
        if let Some(b) = found {
            return Some(*b);
        }
    }
    free.first().copied()
}

fn has(rules: &CompiledRuleset, kind: UnitKindId, ability: UnitAbility) -> bool {
    rules
        .units
        .get(kind.0 as usize)
        .is_some_and(|u| u.module.abilities.contains(&ability))
}

fn prophet_command(
    obs: &AIObservation<'_>,
    rules: &CompiledRuleset,
    unit: &OwnUnit,
    scratch: &mut MapScratch,
) -> Option<Command> {
    if !has(rules, unit.kind, UnitAbility::FoundReligion) {
        return None;
    }
    let faith = &obs.faith.overview;
    if faith.religion.is_none() && faith.religions_left > 0 {
        return Some(found_or_walk(obs, rules, unit));
    }
    let religion = faith.religion?;
    let enhanced = faith
        .religions
        .iter()
        .any(|r| r.religion == religion && r.enhanced);
    if !enhanced {
        let follower = pick(rules, &faith.free_follower, &FOLLOWERS)?;
        let enhancer = pick(rules, &faith.free_enhancer, &ENHANCERS)?;
        return Some(Command::EnhanceReligion {
            unit: unit.handle,
            follower,
            enhancer,
        });
    }
    spread_or_walk(obs, unit, religion, scratch).or_else(|| holy_site_or_step_out(obs, rules, unit))
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
fn found_or_walk(obs: &AIObservation<'_>, rules: &CompiledRuleset, unit: &OwnUnit) -> Command {
    let in_city = obs.own_cities.iter().any(|c| c.tile == unit.tile);
    let faith = &obs.faith.overview;
    let founder = pick(rules, &faith.free_founder, &FOUNDERS);
    let follower = pick(rules, &faith.free_follower, &FOLLOWERS);
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
                _ => bonus_belief(obs, rules, faith, &chosen),
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

/// Byzantium's bonus belief: a second founder belief of the preference
/// list, else a second follower, else an enhancer, else the pantheon belief
/// that pays most (none of the beliefs already chosen).
fn bonus_belief(
    obs: &AIObservation<'_>,
    rules: &CompiledRuleset,
    faith: &crate::derived::religion::FaithOverview,
    chosen: &[Option<BeliefId>],
) -> Option<BeliefId> {
    let options = [
        (&faith.free_founder, &FOUNDERS[..]),
        (&faith.free_follower, &FOLLOWERS[..]),
        (&faith.free_enhancer, &ENHANCERS[..]),
    ];
    let left = |free: &[BeliefId]| -> Vec<BeliefId> { free.iter().copied().filter(|b| !chosen.contains(&Some(*b))).collect() };
    for (free, preferred) in options {
        if let Some(belief) = pick(rules, &left(free), preferred) {
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

/// The player's units may enter the land of `owner` (own land, or open
/// borders granted to the player).
fn may_enter(obs: &AIObservation<'_>, owner: crate::ids::PlayerId) -> bool {
    owner == obs.player
        || obs
            .relations
            .iter()
            .any(|r| r.other == owner && r.they_grant_entry && !r.at_war)
}

/// A missionary bought in the first own city following the player's
/// religion, when fewer than MISSIONARIES_WANTED are out.
fn purchase_command(obs: &AIObservation<'_>, rules: &CompiledRuleset) -> Option<Command> {
    obs.faith.overview.religion?;
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
        let commands = religion_commands(input.observation, input.rules);
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
