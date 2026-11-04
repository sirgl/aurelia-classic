//! Great person decisions from the observation: what an own great person
//! does now. The action rules are the core's own (`derived::great_people`);
//! the improvement site is checked over the player's knowledge.
//!
//! ```text
//! golden age unit (artist)      StartGoldenAge
//! scientist                     DiscoverTechnology while research is queued,
//!                               else its improvement (academy) on the spot
//! engineer                      HurryProduction in an own city with a
//!                               building at the head of the queue, else its
//!                               improvement (manufactory) on the spot
//! merchant on a city-state's     TradeMission (gold and influence, doubled
//! land                          gold with Entrepreneurship)
//! merchant and the others       their improvement on the spot (customs
//!                               house, citadel), else nothing
//! ```
//!
//! A scientist, engineer or merchant with nothing to do where it stands
//! (in a city, say) walks to the nearest own tile next to a city where its
//! improvement may stand: without this they stood idle in the capital for
//! the rest of the game, where they also kept spaceship parts out (one
//! civilian per tile; found by the full-game harness).
//!
//! Writers and musicians make great works (track B), generals and admirals
//! lead armies (track C): without a site they keep waiting.

use crate::knowledge::public::PlayerQueries;
use crate::command::Command;
use crate::derived::great_people::{great_person_of, GreatPersonAction};
use crate::derived::tile_yields::TechSet;
use crate::ids::{ImprovementId, TileId};
use crate::knowledge::observation::{AIObservation, OwnUnit};
use crate::rules::CompiledRuleset;
use crate::world::builds::{site, Builder};

/// What a great person should do now (None: keep it).
pub fn great_person_action(
    obs: &AIObservation<'_>,
    rules: &CompiledRuleset,
    unit: &OwnUnit,
) -> Option<GreatPersonAction> {
    let gp = great_person_of(rules, unit.kind)?;
    let abilities = rules.units.get(unit.kind.0 as usize)?.module.economy;
    if abilities.golden_age_turns > 0 {
        return Some(GreatPersonAction::StartGoldenAge);
    }
    if abilities.beakers_turns > 0 && !obs.economy.research_queue.is_empty() {
        return Some(GreatPersonAction::DiscoverTechnology);
    }
    if abilities.base_hurry > 0 && building_in_city(obs, unit) {
        return Some(GreatPersonAction::HurryProduction);
    }
    if abilities.base_gold > 0 {
        let mission = Command::GreatPersonAction {
            unit: unit.handle,
            action: GreatPersonAction::TradeMission,
        };
        if obs.view.accepts(&mission) {
            return Some(GreatPersonAction::TradeMission);
        }
    }
    let builder = Builder {
        player: obs.player,
        nation: obs.economy.nation,
        techs: TechSet::of(&obs.economy.techs),
    };
    gp.improvements
        .iter()
        .copied()
        .find(|i| site(&obs.view, rules, &builder, unit.tile, *i).is_ok())
        .map(GreatPersonAction::Construct)
}

/// The unit stands in an own city whose head a great engineer may hurry
/// (a building, wonders included; a spaceship part with Spaceflight
/// Pioneers).
fn building_in_city(obs: &AIObservation<'_>, unit: &OwnUnit) -> bool {
    match obs.own_cities.iter().find(|c| c.tile == unit.tile) {
        Some(city) => obs.view.engineer_may_hurry(city.handle),
        None => false,
    }
}

/// Orders for every own great person with something to do: its action,
/// else the walk to a site for its improvement.
pub fn great_person_commands(obs: &AIObservation<'_>, rules: &CompiledRuleset) -> Vec<Command> {
    let mut taken: Vec<TileId> = Vec::new();
    obs.own_units.iter().filter_map(|unit| unit_order(obs, rules, unit, &mut taken)).collect()
}

/// The order of one own unit by `great_person_commands`: its action, else
/// the walk to a site for its improvement that no other great person took
/// this turn (`taken` gets the site).
pub(crate) fn unit_order(obs: &AIObservation<'_>, rules: &CompiledRuleset, unit: &OwnUnit, taken: &mut Vec<TileId>) -> Option<Command> {
    if let Some(action) = great_person_action(obs, rules, unit) {
        return Some(Command::GreatPersonAction {
            unit: unit.handle,
            action,
        });
    }
    let site = walk_to_site(obs, rules, unit, taken)?;
    taken.push(site);
    let walk = Command::MarchTo {
        unit: unit.handle,
        destination: site,
    };
    obs.view.accepts(&walk).then_some(walk)
}

/// Own tiles this close to an own city are building sites.
const SITE_RADIUS: i32 = 2;

/// The nearest own tile next to an own city where a scientist, engineer
/// or merchant may build its improvement, free of other own units and not
/// chosen by another great person this turn.
fn walk_to_site(
    obs: &AIObservation<'_>,
    rules: &CompiledRuleset,
    unit: &OwnUnit,
    taken: &[TileId],
) -> Option<TileId> {
    if unit.moves_left == 0 {
        return None;
    }
    let abilities = rules.units.get(unit.kind.0 as usize)?.module.economy;
    let builds = abilities.beakers_turns > 0 || abilities.base_hurry > 0 || abilities.base_gold > 0;
    if !builds {
        return None;
    }
    let improvement = *great_person_of(rules, unit.kind)?.improvements.first()?;
    let dims = obs.view.dimensions();
    let mut best: Option<(i32, TileId)> = None;
    for city in &obs.own_cities {
        for tile in crate::math::hex_disc(dims, city.tile, SITE_RADIUS) {
            if !free_site(obs, rules, unit, improvement, tile) || taken.contains(&tile) {
                continue;
            }
            let distance = dims.distance(unit.tile, tile).unwrap_or(i32::MAX);
            if best.is_none_or(|(d, t)| (distance, tile.0) < (d, t.0)) {
                best = Some((distance, tile));
            }
        }
    }
    best.map(|(_, tile)| tile)
}

fn free_site(obs: &AIObservation<'_>, rules: &CompiledRuleset, unit: &OwnUnit, improvement: ImprovementId, tile: TileId) -> bool {
    let own = obs.view.tile(tile).snapshot().is_some_and(|s| s.owner == Some(obs.player));
    let occupied = obs.own_units.iter().any(|u| u.tile == tile && u.handle != unit.handle);
    let builder = Builder {
        player: obs.player,
        nation: obs.economy.nation,
        techs: TechSet::of(&obs.economy.techs),
    };
    own && !occupied && site(&obs.view, rules, &builder, tile, improvement).is_ok()
}
