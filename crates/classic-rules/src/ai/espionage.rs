//! Minimal espionage decisions over `AIObservation` (M4 track C): spend
//! steal rights (the first technology of the list), stage likely coups,
//! place idle spies — in the biggest visible city of a civilization, else
//! in a city-state allied to someone else, else at home for
//! counter-intelligence in the city thieves would rob fastest. A thief
//! whose theft would take too long (police, a Great Firewall) moves on.
//! The city planner values counter-espionage buildings by the science
//! they protect (`counter_espionage_value`).

use crate::knowledge::public::PlayerQueries;
use crate::command::Command;
use crate::ids::{BuildingKindId, CityHandle, PlayerId};
use crate::knowledge::observation::{AIObservation, OwnCity, SpyInfo};
use crate::rules::content::BuildingAbility;
use crate::rules::effects::{Modifier, Selector};
use crate::rules::stats::{EspionageStat, Stat};
use crate::rules::CompiledRuleset;
use crate::world::espionage::SpyState;

/// A theft slower than this is not worth the spy: it looks for another city.
pub const SLOW_THEFT_TURNS: u32 = 50;

/// Coups the AI tries (percent chance of success).
pub const COUP_MIN_CHANCE: i64 = 60;

/// One `StealTechnology` per victim with rights.
pub fn steal_orders(obs: &AIObservation<'_>) -> Vec<Command> {
    obs.steal
        .iter()
        .filter_map(|(victim, techs)| {
            techs.first().map(|tech| Command::StealTechnology {
                player: obs.player,
                from: *victim,
                tech: *tech,
            })
        })
        .collect()
}

/// Coups with a good chance.
pub fn coup_orders(obs: &AIObservation<'_>) -> Vec<Command> {
    obs.spies
        .iter()
        .filter(|s| s.coup_chance.is_some_and(|c| c >= COUP_MIN_CHANCE))
        .map(|s| Command::StageCoup {
            player: obs.player,
            spy: s.index,
        })
        .collect()
}

/// Percent of enemy steal rates in one of our cities: its own
/// counter-espionage buildings and our empire-wide ones (the rules'
/// `EnemyStealPercent` effects of buildings we see in our cities).
pub fn own_counter_percent(obs: &AIObservation<'_>, rules: &CompiledRuleset, city: &OwnCity) -> i32 {
    let mut total = 0;
    for building in &city.buildings {
        total += steal_percent_of(rules, *building, Selector::SourceCity);
    }
    for other in &obs.own_cities {
        for building in &other.buildings {
            total += steal_percent_of(rules, *building, Selector::OwnerCities);
        }
    }
    total
}

fn steal_percent_of(rules: &CompiledRuleset, building: BuildingKindId, selector: Selector) -> i32 {
    let Some(def) = rules.buildings.get(building.0 as usize) else {
        return 0;
    };
    let mut total = 0;
    for effect in def.effects.passive_city.iter().filter(|e| e.selector == selector) {
        if let Modifier::AddStat { stat: Stat::Module(crate::rules::stats::ClassicStat::Espionage(EspionageStat::EnemyStealPercent)), amount } = effect.modifier {
            total += amount;
        }
    }
    total
}

/// How fast thieves would rob one of our cities: its science × the
/// percent of enemy steal rates (§7.4).
fn exposure(obs: &AIObservation<'_>, rules: &CompiledRuleset, city: &OwnCity) -> i32 {
    let percent = (100 + own_counter_percent(obs, rules, city)).max(0);
    city.income.science.max(0) * percent
}

/// Our city a counter-spy guards: the one thieves would rob fastest.
fn guarded_city(obs: &AIObservation<'_>, rules: &CompiledRuleset) -> Option<CityHandle> {
    obs.own_cities
        .iter()
        .max_by_key(|c| (exposure(obs, rules, c), std::cmp::Reverse(c.handle.index)))
        .map(|c| c.handle)
}

/// What a counter-espionage building is worth in `city` besides its
/// effects (points, 10 ≈ one yield a turn): the science it protects once
/// spies are about (we have one ourselves: the Renaissance has come).
pub fn counter_espionage_value(obs: &AIObservation<'_>, rules: &CompiledRuleset, city: &OwnCity, kind: BuildingKindId) -> i32 {
    let counter = rules
        .buildings
        .get(kind.0 as usize)
        .is_some_and(|d| d.module.abilities.contains(&BuildingAbility::CounterEspionage));
    if !counter || obs.spies.is_empty() {
        return 0;
    }
    city.income.science.max(0) * 10
}

/// A spy free for a new post: idle, or facing a hopeless theft.
fn movable(spy: &SpyInfo) -> bool {
    spy.state == SpyState::Unassigned || spy.theft_turns.is_some_and(|t| t > SLOW_THEFT_TURNS)
}

/// Where an idle spy goes, best first.
fn targets(obs: &AIObservation<'_>, rules: &CompiledRuleset) -> Vec<CityHandle> {
    let civilization = |owner: PlayerId| {
        obs.relations
            .iter()
            .any(|r| r.other == owner && r.kind == crate::world::player::PlayerKind::Major)
    };
    // The biggest visible cities of civilizations: the most science to
    // steal from (their technologies are not visible).
    let mut majors: Vec<(u32, CityHandle)> = obs
        .visible_foreign_cities
        .iter()
        .filter(|c| civilization(c.owner))
        .map(|c| (c.population, c.handle))
        .collect();
    majors.sort_by_key(|(p, h)| (std::cmp::Reverse(*p), h.index));
    let minors = obs
        .city_states
        .iter()
        .filter(|cs| cs.ally.is_some_and(|a| a != obs.player))
        .filter_map(|cs| {
            obs.visible_foreign_cities
                .iter()
                .find(|c| Some(c.tile) == cs.capital)
                .map(|c| c.handle)
        });
    let home = guarded_city(obs, rules);
    majors
        .into_iter()
        .map(|(_, h)| h)
        .chain(minors)
        .chain(home)
        .collect()
}

/// Idle spies (and thieves facing a hopeless theft) go to the first free
/// target the rules accept.
pub fn placement_orders(obs: &AIObservation<'_>, rules: &CompiledRuleset) -> Vec<Command> {
    let mut used: Vec<CityHandle> = obs.spies.iter().filter_map(|s| s.city).collect();
    let mut out = Vec::new();
    for spy in obs.spies.iter().filter(|s| movable(s)) {
        let order = targets(obs, rules)
            .into_iter()
            .filter(|c| !used.contains(c))
            .map(|city| Command::MoveSpy {
                player: obs.player,
                spy: spy.index,
                city: Some(city),
                diplomat: false,
            })
            .find(|c| obs.view.accepts(c));
        if let Some(Command::MoveSpy {
            city: Some(city), ..
        }) = &order
        {
            used.push(*city);
        }
        out.extend(order);
    }
    out
}

/// The espionage system's advisor (`ai::system`).
#[derive(Debug, Clone, Copy, Default)]
pub struct EspionageAdvisor;

impl crate::ai::system::SystemAdvisor for EspionageAdvisor {
    fn start_of_turn(
        &self,
        input: &crate::ai::system::AdvisorInput<'_, '_>,
    ) -> Vec<crate::ai::system::Proposal> {
        let obs = input.observation;
        let mut out = crate::ai::combat::proposals(9, steal_orders(obs));
        out.extend(crate::ai::combat::proposals(7, coup_orders(obs)));
        out.extend(crate::ai::combat::proposals(5, placement_orders(obs, input.rules)));
        out
    }
}
