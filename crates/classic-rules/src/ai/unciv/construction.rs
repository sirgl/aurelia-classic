// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Derived from Unciv (https://github.com/yairm210/Unciv), tag 4.22.10, MPL-2.0:
// core/src/com/unciv/logic/automation/city/ConstructionAutomation.kt, Automation.kt
// (`chooseMilitaryUnit`, `tryTrainMilitaryUnit`, `rankStatsValue`, `allowSpendingResource`),
// NextTurnAutomation.kt (`automateCities`, `trainSettler`), CivilianUnitAutomation.isLateGame,
// models/ruleset/unit/BaseUnit.kt (`getForceEvaluation`).

//! Unciv's city production: every city with nothing to build takes the
//! choice whose value per remaining hammer is highest (buildings by the
//! yields they add to the city, workers, work boats, military units,
//! spaceship parts); a city under attack, or an empire much weaker than its
//! enemies, trains a military unit first; the best city of three citizens
//! or more trains a settler while the empire is happy and has a defender a
//! city. A building's yields are the city screen's own preview of the city
//! with it (`PlayerQueries::building_preview`), where Unciv clones the city.

use crate::ai::planner::Decision;
use crate::ai::trace::{Actor, Reason};
use crate::command::Command;
use crate::ids::{BuildingKindId, ProductionItem, TileId, UnitKindId};
use crate::knowledge::observation::{AIObservation, OwnCity};
use crate::knowledge::public::PlayerQueries;
use crate::rules::content::Domain;
use crate::rules::CompiledRuleset;
use crate::world::city::CityStatus;

use super::next_turn::draw;
use super::personality::{UncivPersonality, Value};
use super::sight::Kind;

/// Unciv's stats a construction is ranked by.
#[derive(Debug, Clone, Copy, Default)]
pub struct Stats {
    pub food: f32,
    pub production: f32,
    pub gold: f32,
    pub science: f32,
    pub culture: f32,
    pub faith: f32,
    pub happiness: f32,
}

/// `Automation.rankStatsValue`.
pub fn rank_stats_value(stats: &Stats, obs: &AIObservation<'_>) -> f32 {
    let mut rank = stats.food * 1.2;
    rank += if (obs.economy.treasury as i64) <= 0 && obs.economy.income.gold <= 0 { stats.gold } else { stats.gold / 3.0 };
    rank + stats.happiness + stats.production + stats.science + stats.culture + stats.faith
}

/// `getForceEvaluation` of a unit kind (its strengths and movement; the
/// promotions' conditional bonuses left out).
pub fn force_evaluation(rules: &CompiledRuleset, kind: UnitKindId) -> f32 {
    let Some(def) = rules.units.get(kind.0 as usize) else {
        return 0.0;
    };
    if def.strength == 0 && def.ranged_strength == 0 {
        return 0.0;
    }
    let mut power = f32::from(def.strength).powf(1.5);
    let mut ranged = f32::from(def.ranged_strength).powf(1.45);
    if def.domain == Domain::Sea {
        ranged /= 2.0;
    }
    if ranged > 0.0 {
        power = ranged;
    }
    power *= f32::from(def.movement.max(1)).powf(0.3);
    if def.combat.suicide {
        power /= 2.0;
    }
    if def.combat.nuke_level > 0 {
        power += 4000.0;
    }
    power
}

/// `isLateGame`: 55 % of the technologies known.
pub fn is_late_game(obs: &AIObservation<'_>, rules: &CompiledRuleset) -> bool {
    obs.economy.techs.len() as f32 / rules.technologies.len().max(1) as f32 >= 0.55
}

/// The facts of the empire one turn's production reads.
pub struct Empire<'o, 'a> {
    pub obs: &'o AIObservation<'a>,
    pub rules: &'a CompiledRuleset,
    pub personality: UncivPersonality,
    pub at_war: bool,
    pub military_units: usize,
    pub workers: f32,
    pub average_production: f32,
    pub afraid_of_barbarians: bool,
    /// Our cities nearest the capital of another nation we met (likely targets).
    pub front_cities: Vec<TileId>,
}

impl<'o, 'a> Empire<'o, 'a> {
    pub fn new(obs: &'o AIObservation<'a>, rules: &'a CompiledRuleset, personality: UncivPersonality, afraid_of_barbarians: bool) -> Self {
        let at_war = obs.relations.iter().any(|r| r.at_war && r.kind != crate::world::player::PlayerKind::Module(crate::world::player::ClassicKind::Barbarian));
        let military_units = obs.own_units.iter().filter(|u| Kind { rules, kind: u.kind }.is_military()).count();
        let workers = obs.own_units.iter().filter(|u| rules.units.get(u.kind.0 as usize).is_some_and(|d| d.builder && d.domain == Domain::Land)).count() as f32;
        let average_production = if obs.own_cities.is_empty() { 0.0 } else { obs.own_cities.iter().map(|c| c.income.production as f32).sum::<f32>() / obs.own_cities.len() as f32 };
        let dims = obs.view.dimensions();
        let mut front_cities = Vec::new();
        for capital in crate::knowledge::public::PlayerQueries::original_capitals(&obs.view).iter().filter_map(|c| c.tile.filter(|_| c.holder != obs.player)) {
            if let Some(nearest) = obs.own_cities.iter().min_by_key(|c| (dims.distance(c.tile, capital).unwrap_or(i32::MAX), c.tile)) {
                front_cities.push(nearest.tile);
            }
        }
        Self { obs, rules, personality, at_war, military_units, workers, average_production, afraid_of_barbarians, front_cities }
    }

    fn over_average(&self, city: &OwnCity) -> bool {
        city.income.production as f32 >= self.average_production
    }
}

/// A construction and its value (`ConstructionChoice`).
#[derive(Debug, Clone, Copy)]
struct Choice {
    item: ProductionItem,
    modifier: f32,
    remaining: u32,
}

/// What every own city with an empty queue starts to build, and the
/// military units the cities in danger switch to (`automateCities`).
pub fn automate_cities(empire: &Empire<'_, '_>, enemies_force: f32, own_force: f32) -> Vec<Decision> {
    let obs = empire.obs;
    let mut out = Vec::new();
    let weaker = own_force < enemies_force * 0.66;
    for city in &obs.own_cities {
        if matches!(city.status, CityStatus::AwaitingDecision) {
            continue;
        }
        let head_is_military = matches!(city.queue.first(), Some(ProductionItem::Unit(k)) if Kind { rules: empire.rules, kind: *k }.is_military());
        if (city.damage > 0 || weaker) && !head_is_military && !is_puppet(city) {
            if let Some(unit) = choose_military_unit(empire, city) {
                out.push(Decision::act(Actor::Player, Reason::ProduceDefender, Command::ClearProduction { player: obs.player, city: city.handle }));
                out.push(Decision::act(Actor::Player, Reason::ProduceDefender, Command::EnqueueProduction { player: obs.player, city: city.handle, item: ProductionItem::Unit(unit) }));
                continue;
            }
        }
        if city.queue.is_empty() {
            out.extend(choose_next_construction(empire, city));
        }
    }
    out
}

fn is_puppet(city: &OwnCity) -> bool {
    city.status == CityStatus::Puppet
}

fn enqueue(empire: &Empire<'_, '_>, city: &OwnCity, item: ProductionItem) -> Option<Command> {
    let command = Command::EnqueueProduction { player: empire.obs.player, city: city.handle, item };
    empire.obs.view.accepts(&command).then_some(command)
}

/// `chooseNextConstruction`.
fn choose_next_construction(empire: &Empire<'_, '_>, city: &OwnCity) -> Vec<Decision> {
    let mut choices = Vec::new();
    add_building_choices(empire, city, &mut choices);
    if !is_puppet(city) {
        add_spaceship_part_choice(empire, city, &mut choices);
        add_worker_choice(empire, city, &mut choices);
        add_work_boat_choice(empire, city, &mut choices);
        add_military_unit_choice(empire, city, &mut choices);
    }
    let obs = empire.obs;
    let best = choices
        .iter()
        .map(|c| (c, (c.modifier / c.remaining.max(1) as f32).max(0.0)))
        .fold(None::<(&Choice, f32)>, |best, (c, v)| match best {
            Some((_, b)) if b >= v => best,
            _ => Some((c, v)),
        });
    match best {
        Some((choice, _)) => {
            let reason = match choice.item {
                ProductionItem::Unit(k) if Kind { rules: empire.rules, kind: k }.is_military() => Reason::ProduceArmy,
                ProductionItem::Unit(_) => Reason::ProduceWorker,
                ProductionItem::Building(_) => Reason::ProduceBuilding,
                ProductionItem::Project(_) => Reason::ProduceProject,
            };
            vec![Decision::act(Actor::Player, reason, Command::EnqueueProduction { player: obs.player, city: city.handle, item: choice.item })]
        }
        None => {
            // A process: science while there is research left, else gold.
            let all_known = obs.economy.techs.len() >= empire.rules.technologies.len();
            let process = if all_known { crate::world::city_economy::Process::Wealth } else { crate::world::city_economy::Process::Research };
            let command = Command::SetProcess { player: obs.player, city: city.handle, process: Some(process) };
            if city.process != Some(process) && obs.view.accepts(&command) {
                vec![Decision::act(Actor::Player, Reason::NothingToBuild, command)]
            } else {
                Vec::new()
            }
        }
    }
}

fn remaining_work(empire: &Empire<'_, '_>, city: &OwnCity, item: ProductionItem) -> u32 {
    let cost = empire.obs.view.own_production_cost(city.handle, item).unwrap_or(1);
    if city.queue.first() == Some(&item) {
        cost.saturating_sub(city.production)
    } else {
        cost
    }
}

fn add_choice(empire: &Empire<'_, '_>, city: &OwnCity, choices: &mut Vec<Choice>, item: ProductionItem, modifier: f32) {
    choices.push(Choice { item, modifier, remaining: remaining_work(empire, city, item) });
}

/// `addBuildingChoices`.
fn add_building_choices(empire: &Empire<'_, '_>, city: &OwnCity, choices: &mut Vec<Choice>) {
    let rules = empire.rules;
    let total_population: u32 = empire.obs.own_cities.iter().map(|c| c.population).sum();
    for (i, def) in rules.buildings.iter().enumerate() {
        let kind = BuildingKindId(i as u16);
        if city.buildings.contains(&kind) {
            continue;
        }
        let item = ProductionItem::Building(kind);
        if enqueue(empire, city, item).is_none() {
            continue;
        }
        if def.wonder && is_puppet(city) {
            continue;
        }
        if def.wonder && (!empire.over_average(city) || total_population < 12) {
            continue;
        }
        let value = building_value(empire, city, kind);
        add_choice(empire, city, choices, item, value);
    }
    // Unciv's projects are buildings: Apollo-like, Manhattan-like, the victory projects.
    for (i, def) in rules.module.projects.iter().enumerate() {
        let project = crate::rules::content::ProjectId(i as u16);
        let item = ProductionItem::Project(project);
        if def.kind != crate::classic::content::ProjectKind::Project || enqueue(empire, city, item).is_none() {
            continue;
        }
        let mut value = 0.0;
        if empire.over_average(city) {
            if def.victory.is_some() {
                value += 20.0;
            }
            let enables_parts = rules.units.iter().any(|u| u.required_project == Some(project));
            if enables_parts {
                value += 10.0 * empire.personality.modifier_focus(Value::Science, 0.3);
            }
        }
        if def.allows_nukes {
            value += 10.0 * empire.personality.modifier_focus(Value::Military, 0.3);
        }
        add_choice(empire, city, choices, item, value);
    }
}

/// `getValueOfBuilding`: its stats, its defence, its victory and one-time value.
fn building_value(empire: &Empire<'_, '_>, city: &OwnCity, kind: BuildingKindId) -> f32 {
    let preview = empire.obs.view.building_preview(city.handle, kind);
    let mut value = 0.0;
    if let Some(p) = &preview {
        value += apply_building_stats(empire, city, kind, p);
        value += military_building_value(empire, city, kind, p);
    }
    value += onetime_bonuses(empire, kind);
    value
}

/// `applyBuildingStats`.
fn apply_building_stats(empire: &Empire<'_, '_>, city: &OwnCity, kind: BuildingKindId, p: &crate::knowledge::public::BuildingPreview) -> f32 {
    let obs = empire.obs;
    let d = |a: i32, b: i32| (b - a) as f32;
    let mut stats = Stats {
        food: d(p.now.food_surplus, p.with.food_surplus),
        production: d(p.now.production, p.with.production),
        // Unciv's city stats leave the maintenance out (it is the empire's expense).
        gold: d(p.now.gold, p.with.gold),
        science: d(p.now.science, p.with.science),
        culture: d(p.now.culture as i32, p.with.culture as i32),
        faith: d(p.now.faith as i32, p.with.faith as i32),
        happiness: crate::ai::happiness::building_happiness(empire.rules, kind) as f32,
    };
    stats.food *= 3.0;
    stats.production *= 2.0;
    stats.gold *= 2.0;
    let happiness = obs.economy.happiness.excess();
    if happiness < 10 || happiness < obs.own_cities.len() as i32 {
        stats.happiness *= 3.0;
    }
    if city.income.culture < 2 {
        stats.culture *= 2.0;
    }
    let pers = &empire.personality;
    stats.food *= pers.scaled_focus(Value::Food);
    stats.production *= pers.scaled_focus(Value::Production);
    stats.gold *= pers.scaled_focus(Value::Gold);
    stats.science *= pers.scaled_focus(Value::Science);
    stats.culture *= pers.scaled_focus(Value::Culture);
    stats.faith *= pers.scaled_focus(Value::Faith);
    stats.happiness *= pers.scaled_focus(Value::Happiness);
    rank_stats_value(&stats, obs)
}

/// `getMilitaryBuildingValue`.
fn military_building_value(empire: &Empire<'_, '_>, city: &OwnCity, kind: BuildingKindId, p: &crate::knowledge::public::BuildingPreview) -> f32 {
    use crate::rules::effects::Modifier;
    let pers = &empire.personality;
    let mut war = if empire.at_war { 1.0 } else { 0.5 };
    if empire.front_cities.contains(&city.tile) {
        war *= 2.0;
    }
    let mut value = 0.0;
    let max_hp = f32::from(p.max_hp_now.max(1));
    value += war * f32::from(p.max_hp_with.saturating_sub(p.max_hp_now)) / max_hp * pers.inverse_modifier_focus(Value::Aggressive, 0.3);
    let strength_gain = (p.strength_with - p.strength_now) as f32 / 100.0;
    value += war * strength_gain / (p.strength_now as f32 / 100.0 + 3.0) * pers.inverse_modifier_focus(Value::Aggressive, 0.3);
    let def = &empire.rules.buildings[kind.0 as usize];
    for effect in all_effects(&def.effects) {
        if let Modifier::GrantExperience(xp) = effect.modifier {
            let mut modifier = f32::from(xp) / 5.0;
            modifier *= if empire.over_average(city) { 1.0 } else { 0.2 };
            modifier *= pers.modifier_focus(Value::Military, 0.3);
            modifier *= pers.modifier_focus(Value::Aggressive, 0.2).max(1.0);
            value += modifier;
        }
    }
    value
}

/// Every effect of a set, passive and triggered.
pub fn all_effects(set: &crate::rules::effects::EffectSet) -> impl Iterator<Item = &crate::rules::effects::CompiledEffect> {
    set.passive_player.iter().chain(&set.passive_module).chain(&set.passive_city).chain(&set.passive_unit).chain(&set.passive_tile).chain(set.triggered.iter().flatten())
}

/// `getOnetimeUniqueBonuses`: a wonder by the personality value its one-time
/// gift serves; a building that provides a resource +3.
fn onetime_bonuses(empire: &Empire<'_, '_>, kind: BuildingKindId) -> f32 {
    use crate::classic::effects::{ClassicGrant, ClassicModifier};
    use crate::rules::effects::{Grant, Modifier};
    let def = &empire.rules.buildings[kind.0 as usize];
    let pers = &empire.personality;
    if def.wonder {
        for effect in all_effects(&def.effects) {
            let value = match effect.modifier {
                Modifier::Module(ClassicModifier::FreePolicies { .. }) => pers.get(Value::Culture),
                Modifier::Grant(Grant::FreeTechnologies(_)) => pers.get(Value::Science),
                Modifier::FreeUnit { .. } | Modifier::Grant(Grant::MilitaryUnitCopies) => pers.get(Value::Production),
                Modifier::Grant(Grant::Module(ClassicGrant::FreeGreatPeople(_))) => pers.get(Value::Science),
                Modifier::Grant(Grant::Module(ClassicGrant::GoldenAge { .. })) => pers.get(Value::Expansion),
                Modifier::Grant(Grant::Population(_)) => pers.get(Value::Food),
                Modifier::ModifyCombat { percent, .. } if percent > 0 => pers.get(Value::Military),
                _ => continue,
            };
            return value;
        }
        return 0.0;
    }
    // Unciv's +3 for a building that provides a resource: the classic buildings provide none.
    0.0
}

/// `addSpaceshipPartChoice`.
fn add_spaceship_part_choice(empire: &Empire<'_, '_>, city: &OwnCity, choices: &mut Vec<Choice>) {
    if !empire.over_average(city) {
        return;
    }
    let part = empire
        .rules
        .units
        .iter()
        .enumerate()
        .map(|(i, _)| UnitKindId(i as u16))
        .filter(|k| empire.rules.units[k.0 as usize].role == Some(kernel::rules::content::UnitRole::SpaceshipPart))
        .find(|k| enqueue(empire, city, ProductionItem::Unit(*k)).is_some());
    if let Some(part) = part {
        add_choice(empire, city, choices, ProductionItem::Unit(part), 20.0);
    }
}

/// `addWorkerChoice`.
fn add_worker_choice(empire: &Empire<'_, '_>, city: &OwnCity, choices: &mut Vec<Choice>) {
    let worker = cheapest(empire, city, |d| d.builder && d.domain == Domain::Land);
    let Some(worker) = worker else {
        return;
    };
    let cities = empire.obs.own_cities.len() as f32;
    let wanted = if cities <= 1.0 {
        1.0
    } else if cities <= 5.0 {
        cities * 1.8
    } else {
        9.0 + 1.3 * (cities - 5.0)
    };
    if empire.workers < wanted {
        add_choice(empire, city, choices, ProductionItem::Unit(worker), wanted / (empire.workers + 0.17));
    }
}

/// The cheapest unit kind the city may build whose definition passes `filter`.
fn cheapest(empire: &Empire<'_, '_>, city: &OwnCity, filter: impl Fn(&kernel::rules::compiled::UnitDefinition<crate::classic::Classic>) -> bool) -> Option<UnitKindId> {
    empire
        .rules
        .units
        .iter()
        .enumerate()
        .filter(|(_, d)| filter(d))
        .map(|(i, d)| (UnitKindId(i as u16), d.cost))
        .filter(|(k, _)| enqueue(empire, city, ProductionItem::Unit(*k)).is_some())
        .min_by_key(|(k, cost)| (*cost, k.0))
        .map(|(k, _)| k)
}

/// `addWorkBoatChoice`: a sea worker when a known resource in our waters
/// could be improved and none of our work boats is near.
fn add_work_boat_choice(empire: &Empire<'_, '_>, city: &OwnCity, choices: &mut Vec<Choice>) {
    let Some(boat) = cheapest(empire, city, |d| d.builder && d.domain == Domain::Sea) else {
        return;
    };
    let obs = empire.obs;
    let dims = obs.view.dimensions();
    let reach = i32::from(empire.rules.units[boat.0 as usize].movement) * 2;
    let near_boat = obs.own_units.iter().any(|u| empire.rules.units.get(u.kind.0 as usize).is_some_and(|d| d.builder && d.domain == Domain::Sea) && dims.distance(u.tile, city.tile).is_some_and(|d| d <= reach));
    if near_boat {
        return;
    }
    let kinds = obs.view.terrain_kinds();
    let worth = crate::math::hex_disc(dims, city.tile, 3).any(|t| {
        let known = obs.view.tile(t);
        let Some(s) = known.snapshot() else {
            return false;
        };
        s.owner == Some(obs.player) && kinds.is_water(s.terrain.0) && s.resource.is_some() && s.improvement.is_none()
    });
    if worth {
        add_choice(empire, city, choices, ProductionItem::Unit(boat), 6.0);
    }
}

/// `addMilitaryUnitChoice`.
fn add_military_unit_choice(empire: &Empire<'_, '_>, city: &OwnCity, choices: &mut Vec<Choice>) {
    let obs = empire.obs;
    let over = empire.over_average(city);
    if !empire.at_war && !over {
        return;
    }
    let cities = obs.own_cities.len();
    if !empire.at_war && (obs.economy.income.gold < 0 || empire.military_units > 7.max(cities * 5)) {
        return;
    }
    let Some(unit) = choose_military_unit(empire, city) else {
        return;
    };
    let ratio = cities as f32 / (empire.military_units + 1) as f32;
    let mut modifier = 1.0 + ratio.sqrt() / 2.0;
    if empire.at_war {
        modifier *= 2.0;
    }
    if empire.afraid_of_barbarians {
        modifier = 2.0;
    }
    if !over {
        modifier /= 5.0;
    }
    let settler_waiting = obs.own_units.iter().any(|u| u.tile == city.tile && Kind { rules: empire.rules, kind: u.kind }.founds_cities() && Kind { rules: empire.rules, kind: u.kind }.is_civilian());
    let dims = obs.view.dimensions();
    let guarded = obs.own_units.iter().any(|u| Kind { rules: empire.rules, kind: u.kind }.is_military() && dims.distance(u.tile, city.tile).is_some_and(|d| d <= 3));
    if settler_waiting && !guarded {
        modifier = 5.0;
    }
    modifier *= empire.personality.modifier_focus(Value::Military, 0.3);
    add_choice(empire, city, choices, ProductionItem::Unit(unit), modifier);
}

/// `Automation.chooseMilitaryUnit`: a ranged unit for an ungarrisoned city in
/// peace, else a naval melee unit when our coastal cities lack them, else the
/// strongest kind of a random unit type (ignoring those far weaker than the best).
pub fn choose_military_unit(empire: &Empire<'_, '_>, city: &OwnCity) -> Option<UnitKindId> {
    let obs = empire.obs;
    let rules = empire.rules;
    if let Some(ProductionItem::Unit(k)) = city.queue.first() {
        if (Kind { rules, kind: *k }).is_military() {
            return Some(*k);
        }
    }
    let kinds = obs.view.terrain_kinds();
    let dims = obs.view.dimensions();
    let coastal = dims.neighbors(city.tile).into_iter().any(|t| obs.view.tile(t).snapshot().is_some_and(|s| kinds.is_water(s.terrain.0)));
    let (remove_ships, missing_naval) = if coastal { water_body(obs, rules, city.tile) } else { (true, false) };
    let units: Vec<UnitKindId> = (0..rules.units.len())
        .map(|i| UnitKindId(i as u16))
        .filter(|k| Kind { rules, kind: *k }.is_military())
        .filter(|k| !(remove_ships && rules.units[k.0 as usize].domain == Domain::Sea))
        .filter(|k| rules.units[k.0 as usize].combat.carries.is_none())
        .filter(|k| allow_spending_resource(obs, rules, *k))
        .filter(|k| enqueue(empire, city, ProductionItem::Unit(*k)).is_some())
        .collect();
    if units.is_empty() {
        return None;
    }
    let ungarrisoned = obs.own_cities.iter().any(|c| !obs.own_units.iter().any(|u| u.tile == c.tile && Kind { rules, kind: u.kind }.is_military()));
    let ranged: Vec<UnitKindId> = units.iter().copied().filter(|k| Kind { rules, kind: *k }.is_ranged()).collect();
    if !empire.at_war && ungarrisoned && !ranged.is_empty() {
        return ranged.into_iter().max_by_key(|k| (rules.units[k.0 as usize].cost, std::cmp::Reverse(k.0)));
    }
    let naval_melee: Vec<UnitKindId> = units.iter().copied().filter(|k| Kind { rules, kind: *k }.is_melee() && rules.units[k.0 as usize].domain == Domain::Sea).collect();
    if missing_naval && !naval_melee.is_empty() {
        return naval_melee.into_iter().max_by_key(|k| (rules.units[k.0 as usize].cost, std::cmp::Reverse(k.0)));
    }
    // The strongest of each unit type (role).
    let mut best: Vec<(Option<kernel::rules::content::UnitRole>, UnitKindId)> = Vec::new();
    for k in &units {
        let role = rules.units[k.0 as usize].role;
        match best.iter_mut().find(|(r, _)| *r == role) {
            Some(entry) => {
                if force_evaluation(rules, entry.1) < force_evaluation(rules, *k) {
                    entry.1 = *k;
                }
            }
            None => best.push((role, *k)),
        }
    }
    let top = best.iter().map(|(_, k)| force_evaluation(rules, *k)).fold(0.0f32, f32::max);
    let shortlist: Vec<UnitKindId> = best.iter().map(|(_, k)| *k).filter(|k| rules.units[k.0 as usize].nation.is_some() || force_evaluation(rules, *k) > top / 3.0).collect();
    if shortlist.is_empty() {
        return None;
    }
    Some(shortlist[draw(obs.round, u64::from(city.handle.index), 0x6d75, shortlist.len())])
}

/// The known water connected to the city: whether no foreign city or unit is
/// on it (no use for ships), and whether our coastal cities on it outnumber
/// our naval melee units.
fn water_body(obs: &AIObservation<'_>, rules: &CompiledRuleset, start: TileId) -> (bool, bool) {
    let kinds = obs.view.terrain_kinds();
    let dims = obs.view.dimensions();
    let mut seen = vec![start];
    let mut frontier = vec![start];
    let mut foreign = false;
    let mut own_cities = 0;
    let mut naval_melee = 0;
    while let Some(t) = frontier.pop() {
        if seen.len() > 3000 {
            break;
        }
        for n in dims.neighbors(t) {
            if seen.contains(&n) {
                continue;
            }
            let known = obs.view.tile(n);
            let Some(s) = known.snapshot() else {
                continue;
            };
            if s.city {
                seen.push(n);
                if s.owner == Some(obs.player) {
                    own_cities += 1;
                } else {
                    foreign = true;
                }
                continue;
            }
            if !kinds.is_water(s.terrain.0) {
                continue;
            }
            seen.push(n);
            frontier.push(n);
            if obs.visible_foreign_units.iter().any(|u| u.tile == n && Kind { rules, kind: u.kind }.is_military()) {
                foreign = true;
            }
            naval_melee += obs.own_units.iter().filter(|u| u.tile == n && Kind { rules, kind: u.kind }.is_melee() && rules.units[u.kind.0 as usize].domain == Domain::Sea).count();
        }
    }
    (!foreign, own_cities > naval_melee)
}

/// `allowSpendingResource`: a unit's strategic resources are spent only while
/// the empire has the copies free.
pub fn allow_spending_resource(obs: &AIObservation<'_>, rules: &CompiledRuleset, kind: UnitKindId) -> bool {
    rules.units[kind.0 as usize].required_resources.iter().all(|(r, n)| obs.economy.resources.available(*r) >= i32::from(*n))
}

/// `trainSettler`: the most productive city of three citizens or more (not
/// a puppet, two buildings or more) switches to the cheapest settler while
/// the empire is happy, has a military unit a city and fewer than two
/// settlers in flight; late in the game only while land is left near our cities.
pub fn train_settler(empire: &Empire<'_, '_>) -> Vec<Decision> {
    let obs = empire.obs;
    let rules = empire.rules;
    if obs.own_cities.is_empty() || obs.view.own_kind() != crate::world::player::PlayerKind::Major {
        return Vec::new();
    }
    let happiness = obs.economy.happiness.excess();
    if happiness <= obs.own_cities.len() as i32 {
        return Vec::new();
    }
    let is_settler = |item: &ProductionItem| matches!(item, ProductionItem::Unit(k) if rules.units[k.0 as usize].founder);
    if is_late_game(obs, rules) {
        let dims = obs.view.dimensions();
        let mut free = 0;
        let mut counted: Vec<TileId> = Vec::new();
        for city in &obs.own_cities {
            for t in crate::math::hex_disc(dims, city.tile, 10) {
                if counted.contains(&t) {
                    continue;
                }
                counted.push(t);
                let unowned = |t: TileId| obs.view.tile(t).snapshot().is_some_and(|s| s.owner.is_none());
                if unowned(t) && dims.neighbors(t).into_iter().all(unowned) {
                    free += 1;
                }
            }
        }
        if free < 6 {
            return Vec::new();
        }
    }
    let in_flight = obs.own_units.iter().filter(|u| rules.units[u.kind.0 as usize].founder).count() + obs.own_cities.iter().filter(|c| c.queue.first().is_some_and(is_settler)).count();
    if in_flight >= 2 {
        return Vec::new();
    }
    let majors = obs.relations.iter().filter(|r| r.kind == crate::world::player::PlayerKind::Major).count() + 1;
    let slack = !empire.at_war && happiness >= 8 && obs.round < obs.speed.game_round(150) && majors >= 3;
    if empire.military_units < obs.own_cities.len() && !slack {
        return Vec::new();
    }
    let best = obs
        .own_cities
        .iter()
        .filter(|c| !is_puppet(c) && c.population >= 3 && !c.queue.first().is_some_and(is_settler))
        .max_by_key(|c| (c.income.production, std::cmp::Reverse(c.id)));
    let Some(city) = best else {
        return Vec::new();
    };
    if city.buildings.len() <= 1 {
        return Vec::new();
    }
    let Some(settler) = cheapest(empire, city, |d| d.founder) else {
        return Vec::new();
    };
    vec![
        Decision::act(Actor::Player, Reason::ProduceSettler, Command::ClearProduction { player: obs.player, city: city.handle }),
        Decision::act(Actor::Player, Reason::ProduceSettler, Command::EnqueueProduction { player: obs.player, city: city.handle, item: ProductionItem::Unit(settler) }),
    ]
}
