//! The classic part of the kernel's systems (`kernel::systems::ModuleSystems`):
//! the module's turn steps, what it does at the kernel's events and the
//! numbers of its formulas.

use kernel::systems::{CityGrowth, KernelEvent, ModuleSystems};

use crate::classic::effects::ClassicTrigger;
use crate::classic::turn::{CityStepInput, EndTurnInput};
use crate::classic::{Classic, ClassicCityStep, ClassicStep};
use crate::effects::city::CityModifiers;
use crate::ids::{CityHandle, PlayerId, ProductionItem, TileId, UnitHandle};
use crate::rules::effects::{CostTarget, Trigger};
use crate::rules::CompiledRuleset;
use crate::systems::effects::TriggerEvent;
use crate::systems::growth;
use crate::transaction::transaction::{KernelTransaction, WorldTransaction};
use crate::world::city_economy::Process;
use crate::world::state::AuthWorld;
use crate::world::city::CityCore;
use crate::world::unit::UnitCore;
use crate::command::validate::{needs_resources, trade_rule, very_unhappy};
use crate::command::CommandError;
use crate::world::player::ClassicPlayers;

/// The kernel's snapshot of a city during its resolution.
pub type CityTurn = kernel::systems::turn::CityTurn<Classic>;

/// Treasury balance at or below which a deficit may disband a unit (own value,
/// not compared with a public source (unverified)).
pub const DEFICIT_DISBAND_THRESHOLD: i64 = -5;

/// The classic systems at the kernel's points.
#[derive(Debug, Clone, Copy)]
pub struct ClassicSystems;

impl ModuleSystems<Classic> for ClassicSystems {
    fn ai_prices_by_profile() -> bool {
        true
    }

    fn run_step(tx: &mut KernelTransaction<'_>, step: ClassicStep, input: &EndTurnInput<'_>) {
        crate::classic::turn::run_step(WorldTransaction::of(tx), step, input)
    }

    fn run_city_step(tx: &mut KernelTransaction<'_>, step: ClassicCityStep, input: &CityStepInput<'_>) {
        crate::classic::turn::run_city_step(WorldTransaction::of(tx), step, input)
    }

    /// Religious pressure between cities, once per round.
    fn end_round(tx: &mut KernelTransaction<'_>, rules: &CompiledRuleset) {
        WorldTransaction::of(tx).religious_pressure_round(rules);
    }

    /// The aircraft and missiles based on a carrier go down with it.
    fn unit_removing(tx: &mut KernelTransaction<'_>, unit: crate::ids::UnitHandle) {
        WorldTransaction::of(tx).destroy_cargo(unit);
    }

    fn on_event(tx: &mut KernelTransaction<'_>, rules: &CompiledRuleset, event: KernelEvent) {
        let tx = WorldTransaction::of(tx);
        match event {
            KernelEvent::UnitSpawned { player, unit } => great_person_born(tx, rules, player, unit),
            KernelEvent::UnitEntered { player, unit, tile } => {
                tx.clear_camp_at(player, tile, rules);
                tx.carry_cargo(unit, tile, rules);
                tx.touch_natural_wonders(unit, tile, rules);
                tx.enter_ruins(player, unit, tile, rules);
            }
            KernelEvent::UnitTrained { city, unit, kind } => {
                tx.fire(rules, TriggerEvent::UnitTrained { city, unit, kind });
                tx.fire(rules, TriggerEvent::UnitTrainedHere { city, unit });
            }
            KernelEvent::UnitPurchased { unit, kind, .. } => tx.bought_unit_waits(unit, kind, rules),
            KernelEvent::BuildingCompleted { city, building } => {
                tx.place_free_great_work(city, building, rules);
                tx.fire(rules, TriggerEvent::BuildingBuilt { city, building });
                tx.capital_science_boost(city, building, rules);
                tx.espionage_after_building(city, building, rules);
            }
            KernelEvent::ProjectCompleted { player, project, .. } => tx.complete_project(player, project),
            KernelEvent::CityFounded { player, city } => {
                tx.claim_extra_founding_tiles(player, city, rules);
                tx.place_unique_luxury(player, city, rules);
                tx.settle_free_buildings(player, city);
                tx.fire(rules, TriggerEvent::CityFounded { player, city });
            }
            KernelEvent::TechnologyLearned { player, tech, new_era } => {
                tx.fire(rules, TriggerEvent::TechnologyLearned { player, tech });
                if new_era {
                    tx.fire(rules, TriggerEvent::EraEntered { player });
                }
                tx.drop_unfit_free_technologies(rules, player);
            }
            KernelEvent::TurnStarted { player } => {
                // OnTurnStart, then OnLongCountCycle when the game year
                // crossed the end of a Long Count cycle since the last round.
                tx.lend_movement_to_generals(player, rules);
                tx.religious_strength_loss(player, rules);
                tx.fire(rules, TriggerEvent::TurnStart { player });
                if crate::classic::calendar::long_count_cycle_ends(tx.world.turn.round) {
                    tx.fire(rules, TriggerEvent::LongCountCycle { player });
                }
            }
            KernelEvent::ScienceCollected { player, science } => tx.record_science(player, science),
        }
    }

    fn heal_amount(auth: &AuthWorld, rules: &CompiledRuleset, unit: &UnitCore) -> i32 {
        crate::systems::combat::heal::heal_amount(auth, rules, unit)
    }

    /// City growth from the snapshot: the empire's happiness changes the
    /// surplus, an AI's threshold follows its difficulty level.
    fn city_growth(auth: &AuthWorld, rules: &CompiledRuleset, city: &CityTurn) -> CityGrowth {
        let excess = crate::derived::happiness::excess_snapshot(auth, city.owner);
        CityGrowth {
            difference_times100: growth::food_difference_times100(
                &city.yields,
                city.population,
                city.producing_food,
                crate::derived::happiness::growth_percent(excess),
            ),
            kept_percent: growth::food_kept_percent(&city.yields),
            threshold: growth::player_growth_threshold(auth, rules, city.owner, city.population),
        }
    }

    fn food_production(city: &CityTurn) -> i32 {
        growth::food_production(&city.yields, city.population)
    }

    fn border_threshold(level: u32, modifiers: &CityModifiers) -> u32 {
        let exponent = modifiers.cost_percent[CostTarget::BorderGrowthExponent.index()];
        let cost = modifiers.cost_percent[CostTarget::BorderGrowth.index()];
        crate::derived::culture::border_cost(level, exponent, cost)
    }

    /// Wealth and Research convert the production; a World Congress
    /// project takes it whole.
    fn run_process(
        tx: &mut KernelTransaction<'_>,
        owner: PlayerId,
        _city: CityHandle,
        process: Process,
        production: u32,
    ) -> (i32, i32) {
        if let Process::Contribute(project) = process {
            WorldTransaction::of(tx).contribute_to_project(owner, project, production);
            return (0, 0);
        }
        crate::systems::processes::process_yield(Some(process), production as i32)
    }

    /// An order the empire lacks the resources for is dropped from the queue.
    fn queue_item_blocked(auth: &AuthWorld, rules: &CompiledRuleset, owner: PlayerId, item: ProductionItem) -> bool {
        crate::derived::resources::production_resources(auth, rules, owner, item).is_err()
    }

    /// The treasury (rules.md): the cities' gold through the player's
    /// modifiers, plus city connections, minus the maintenance of units,
    /// buildings and routes.
    fn gold_balance(auth: &AuthWorld, rules: &CompiledRuleset, player: PlayerId, cities_gold: i32) -> i64 {
        let gross = crate::derived::income::player_gold(auth, rules, player, cities_gold);
        let upkeep = crate::derived::maintenance::upkeep(auth, rules, player);
        i64::from(gross) - i64::from(upkeep.net_cost())
    }

    /// At −5 or worse an empire with more military units than cities
    /// disbands one with probability 1/2.
    fn treasury_deficit(tx: &mut KernelTransaction<'_>, rules: &CompiledRuleset, player: PlayerId, balance: i64) {
        if balance <= DEFICIT_DISBAND_THRESHOLD {
            WorldTransaction::of(tx).disband_for_deficit(player, rules);
        }
    }

    /// The cities' science through the player's modifiers; the World
    /// Congress's scholars add a fifth.
    fn science(auth: &AuthWorld, rules: &CompiledRuleset, player: PlayerId, cities_science: i32) -> i32 {
        let science = crate::derived::income::player_science(auth, rules, player, cities_science);
        if crate::systems::congress::scholars_bonus(auth, player) {
            return science * (100 + crate::systems::congress::SCHOLARS_PERCENT) / 100;
        }
        science
    }

    /// Research cost (rules.md, `derived::research`).
    fn research_cost(auth: &AuthWorld, rules: &CompiledRuleset, player: PlayerId, tech: crate::ids::TechId) -> u32 {
        crate::derived::research::technology_cost(auth, rules, player, tech)
    }

    /// City-states found no cities; a very unhappy empire founds none.
    fn founding_problem(auth: &AuthWorld, _rules: &CompiledRuleset, owner: PlayerId, _tile: TileId) -> Option<&'static str> {
        if auth.players().is_city_state(owner) {
            return Some("city-states found no cities");
        }
        if very_unhappy(auth, owner) {
            return Some("empire very unhappy");
        }
        None
    }

    /// A Conquistador founds cities though it is no settler kind.
    fn founds_cities(rules: &CompiledRuleset, kind: crate::ids::UnitKindId) -> bool {
        crate::systems::founding::founds_abroad(rules, kind)
    }

    /// A unit that founds only abroad stays off its capital's landmass.
    fn unit_founding_problem(
        auth: &AuthWorld,
        rules: &CompiledRuleset,
        owner: PlayerId,
        kind: crate::ids::UnitKindId,
        tile: TileId,
    ) -> Option<&'static str> {
        crate::systems::founding::abroad_problem(auth, rules, owner, kind, tile)
    }

    fn attacks_left(auth: &AuthWorld, rules: &CompiledRuleset, unit: &UnitCore) -> u8 {
        crate::combat::exchange::attacks_left(auth, rules, unit)
    }

    fn city_max_hit_points(auth: &AuthWorld, rules: &CompiledRuleset, city: &CityCore) -> u16 {
        crate::combat::city::max_hit_points(auth, rules, city)
    }

    fn fortify_problem(auth: &AuthWorld, rules: &CompiledRuleset, unit: &UnitCore) -> Option<&'static str> {
        crate::combat::exchange::fortify_problem(auth, rules, unit)
    }

    /// Settlers need a city of two and a content empire (city-states train
    /// none); obsolete units, nuclear weapons under the non-proliferation
    /// treaty or before the Manhattan Project, units of a project not built,
    /// projects and buildings of the classic rules, strategic resources and
    /// trade routes left.
    fn production_problem(
        auth: &AuthWorld,
        rules: &CompiledRuleset,
        player: PlayerId,
        city: &CityCore,
        item: ProductionItem,
    ) -> Result<(), CommandError> {
        let violation = |reason: &str| CommandError::RuleViolation(reason.into());
        match item {
            ProductionItem::Unit(kind) => unit_problem(auth, rules, player, city, kind)?,
            ProductionItem::Project(project) => {
                crate::systems::victory::project_problem(auth, rules, player, project, &city.queue)
                    .map_err(violation)?
            }
            ProductionItem::Building(kind) => {
                let handle = auth.indexes().city_on_tile(city.tile).ok_or(CommandError::UnknownCity)?;
                crate::derived::culture::building_culture_status(auth, rules, handle, kind).map_err(violation)?
            }
        }
        // After the technology: a locked unit says so first.
        needs_resources(auth, rules, player, item)?;
        let trade_unit = matches!(item, ProductionItem::Unit(k) if crate::derived::trade::is_trade_unit(rules, k));
        if trade_unit && !crate::derived::trade::route_left(auth, rules, player) {
            return Err(trade_rule(crate::derived::trade::TradeBlock::NoRouteLeft));
        }
        Ok(())
    }

    /// Spaceship parts are for sale only with `PlayerRule::SpaceshipPartPurchase`
    /// (Space Procurements); every other unit is.
    fn unit_for_sale(auth: &AuthWorld, rules: &CompiledRuleset, owner: PlayerId, unit: crate::ids::UnitKindId) -> bool {
        let part = rules
            .units
            .get(unit.0 as usize)
            .is_some_and(|d| d.role == Some(crate::rules::content::UnitRole::SpaceshipPart));
        if !part {
            return true;
        }
        let stats = &crate::effects::player::player_modifiers(auth, rules, owner).stats;
        crate::rules::stats::has_rule(&stats, crate::rules::stats::PlayerRule::SpaceshipPartPurchase)
    }

    /// A puppet buys nothing; a very unhappy empire buys no settler; the
    /// head needs its strategic resources.
    fn hurry_problem(auth: &AuthWorld, rules: &CompiledRuleset, player: PlayerId, city: &CityCore) -> Result<(), CommandError> {
        if city.status == crate::world::city::CityStatus::Puppet {
            return Err(CommandError::RuleViolation("a puppet buys nothing".into()));
        }
        let head = city.queue[0];
        let founder = match head {
            ProductionItem::Unit(kind) => rules.units.get(kind.0 as usize).is_some_and(|u| u.founder),
            ProductionItem::Building(_) | ProductionItem::Project(_) => false,
        };
        if founder && very_unhappy(auth, player) {
            return Err(CommandError::RuleViolation("empire very unhappy".into()));
        }
        needs_resources(auth, rules, player, head)
    }

    /// The barbarian player lives while barbarians are on (its camps
    /// appear in its own turns).
    fn stays_alive(auth: &AuthWorld, player: PlayerId) -> bool {
        auth.players().is_barbarian(player)
            && auth.module().barbarians.mode != crate::world::barbarians::BarbarianMode::None
    }

    /// The high-water mark of the player's cities.
    fn had_city(auth: &AuthWorld, player: PlayerId) -> bool {
        auth.module().culture.of(player).max_cities > 0
    }
}

/// Unit-creation effects fire whichever way the unit came.
fn great_person_born(tx: &mut WorldTransaction<'_>, rules: &CompiledRuleset, player: PlayerId, unit: UnitHandle) {
    let watched = !rules.effect_index.sources(Trigger::Module(ClassicTrigger::OnGreatPersonBorn)).is_empty();
    let kind = tx.world.units.get(unit).map(|u| u.core.kind);
    let great = kind.is_some_and(|k| crate::derived::great_people::great_person_of(rules, k).is_some());
    if watched && great {
        tx.fire(rules, TriggerEvent::GreatPersonBorn { player, unit });
    }
}

/// The classic rules of training a unit.
fn unit_problem(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    player: PlayerId,
    city: &CityCore,
    kind: crate::ids::UnitKindId,
) -> Result<(), CommandError> {
    let violation = |reason: &str| Err(CommandError::RuleViolation(reason.into()));
    let Some(def) = rules.units.get(kind.0 as usize) else {
        return violation("unknown unit kind");
    };
    if def.founder && city.population < growth::SETTLER_MIN_CITY_SIZE {
        return violation("city too small");
    }
    // Ships are built in coastal cities only (a lake is not enough).
    if def.domain == crate::rules::content::Domain::Sea && !auth.map().coastal_land(city.tile) {
        return violation("not a coastal city");
    }
    if def.founder && very_unhappy(auth, player) {
        return violation("empire very unhappy");
    }
    if crate::systems::combat::upgrade::is_obsolete(auth, rules, player, kind) {
        return violation("unit obsolete");
    }
    if def.founder && auth.players().is_city_state(player) {
        return violation("city-states train no settlers");
    }
    let nuke_ban = auth
        .module()
        .congress()
        .is_active(crate::world::congress::ResolutionKind::NuclearNonProliferation);
    if def.combat.nuke_level > 0 && nuke_ban {
        return violation("nuclear non-proliferation");
    }
    if def.combat.nuke_level > 0 && !crate::systems::victory::nukes_allowed(auth, rules, player) {
        return violation("nuclear weapons need the Manhattan Project");
    }
    if let Some(missing) = crate::world::building_rules::first_missing(rules, city, &def.requires_buildings) {
        let name = rules.buildings.get(missing.0 as usize).map_or("a building", |b| b.name.as_str());
        return violation(&format!("needs {name} in the city"));
    }
    if def.required_project.is_some_and(|p| auth.module().victory().project_count(player, p) == 0) {
        return violation("needs a project first");
    }
    // A unit of a policy (Landsknecht: Mercenary Army).
    if def.module.required_policy.is_some_and(|p| !auth.module().culture().of(player).policies.contains(&p)) {
        return violation("needs its policy");
    }
    Ok(())
}
