//! Happiness decisions from the observation: an empire short of happiness
//! queues a happiness building in its largest city that can build one.
//!
//! ```text
//! excess ≥ NEW_CITY_UNHAPPINESS nothing (room for one more city)
//! excess below it               the largest city without a happiness
//!                               building in its queue enqueues the
//!                               cheapest one it can build (its own
//!                               happiness first; ties to the lower id)
//! ```
//!
//! Expansion stops by itself while happiness is short
//! (`planner_strategy::happiness_allows_city`).

use crate::classic::effects::ClassicModifier;
use crate::command::Command;
use crate::ids::{BuildingKindId, ProductionItem};
use crate::knowledge::observation::{AIObservation, OwnCity};
use crate::rules::effects::Modifier;
use crate::rules::CompiledRuleset;
use crate::world::city::MAX_QUEUE;

/// Happiness a building gives (its City and Player effects).
pub fn building_happiness(rules: &CompiledRuleset, kind: BuildingKindId) -> i32 {
    let Some(def) = rules.buildings.get(kind.0 as usize) else {
        return 0;
    };
    let effects = def
        .effects
        .passive_city
        .iter()
        .chain(&def.effects.passive_player);
    effects
        .map(|e| match e.modifier {
            Modifier::Module(ClassicModifier::AddHappiness { amount }) => amount,
            _ => 0,
        })
        .sum()
}

/// The happiness building a city should queue (None: it has one queued,
/// or none is available).
pub fn happiness_building(
    obs: &AIObservation<'_>,
    rules: &CompiledRuleset,
    city: &OwnCity,
) -> Option<BuildingKindId> {
    cheapest_building(obs, rules, city, |kind| building_happiness(rules, kind) > 0)
}

/// The cheapest building of a kind (`wanted`) the city can build by
/// production now (None: one of the kind is queued, or none is available).
pub fn cheapest_building(
    obs: &AIObservation<'_>,
    rules: &CompiledRuleset,
    city: &OwnCity,
    wanted: impl Fn(BuildingKindId) -> bool,
) -> Option<BuildingKindId> {
    let queued = |kind: BuildingKindId| city.queue.contains(&ProductionItem::Building(kind));
    let mut best: Option<(BuildingKindId, u32)> = None;
    for (i, def) in rules.buildings.iter().enumerate() {
        let kind = BuildingKindId(i as u16);
        if !wanted(kind) {
            continue;
        }
        if queued(kind) {
            return None;
        }
        let known = def
            .required_tech
            .is_none_or(|t| obs.economy.techs.contains(&t));
        let own = def.nation.is_none_or(|c| c == obs.economy.nation);
        let required = def
            .requires_buildings
            .iter()
            .all(|b| city.buildings.contains(b));
        // Faith-only buildings and resource-consuming ones are left to
        // their own systems.
        let by_production =
            def.cost > 0 && def.module.faith_cost.is_none() && def.consumes_resources.is_empty();
        let available = known
            && own
            && required
            && by_production
            && !def.wonder
            && !def.unbuildable
            && !city.buildings.contains(&kind)
            && obs.view.own_building_site(city.handle, kind).is_ok();
        if available && best.is_none_or(|(_, cost)| def.cost < cost) {
            best = Some((kind, def.cost));
        }
    }
    best.map(|(kind, _)| kind)
}

/// EnqueueProduction of a happiness building while the empire is unhappy.
pub fn happiness_commands(obs: &AIObservation<'_>, rules: &CompiledRuleset) -> Vec<Command> {
    if obs.economy.happiness.excess() >= crate::ai::planner_strategy::NEW_CITY_UNHAPPINESS {
        return Vec::new();
    }
    // A city with an empty queue gets its order from the planner in the
    // same turn (from the same observation): leave it to the planner.
    let mut cities: Vec<&OwnCity> = obs
        .own_cities
        .iter()
        .filter(|c| !c.queue.is_empty() && c.queue.len() < MAX_QUEUE)
        .collect();
    cities.sort_by_key(|c| (std::cmp::Reverse(c.population), c.id));
    for city in cities {
        if let Some(kind) = happiness_building(obs, rules, city) {
            return vec![Command::EnqueueProduction {
                player: obs.player,
                city: city.handle,
                item: ProductionItem::Building(kind),
            }];
        }
    }
    Vec::new()
}
