//! Schema pieces added for the classic effect vocabulary
//! (classic/docs/classic/effect_vocabulary.json): OnUnitTrained (`units_trained_here`),
//! per-citizen yields (`yield.*.per_pop`), combat contexts
//! (`combat.attack/defense/attack_vs_city`), purchase/border cost targets,
//! LacksTechnology / MinPopulation conditions, FreeUnit counts.

use crate::command::Command;
use crate::effects::city::city_modifiers;
use crate::effects::unit::{owner_unit_modifiers, with_own_sources, CombatRole};
use crate::ids::{BuildingKindId, CityHandle, PlayerId, ProductionItem, PromotionId, UnitKindId};
use crate::math::HexCoord;
use crate::rules::effects::{CostTarget, YieldKind};
use crate::rules::CompiledRuleset;
use crate::world::state::GameConfig;
use crate::GameState;

const CONTENT: &str = r#"(
    units: [
        (id: "core:unit/settler", movement: 2, strength: 1, founder: true, cost: 40),
        (id: "core:unit/warrior", movement: 2, strength: 10, cost: 30),
    ],
    promotions: [
        (id: "core:promotion/shock", effects: [
            (scope: Unit, selector: SourceUnit, modifier: ModifyCombat(context: Attack, percent: 50)),
            (scope: Unit, selector: SourceUnit, modifier: ModifyCombat(context: Defense, percent: 20)),
            (scope: Unit, selector: SourceUnit, modifier: ModifyCombat(context: VsCity, percent: 100)),
        ]),
    ],
    buildings: [
        (id: "core:building/barracks", cost: 1, effects: [
            (scope: Unit, selector: EventUnit, trigger: OnUnitTrained, modifier: AddPromotion("core:promotion/shock"), duration: Once),
        ]),
        (id: "core:building/library", cost: 1, effects: [
            (scope: City, selector: SourceCity, modifier: AddYieldPerPopulation(yield_kind: Science, hundredths: 50)),
        ]),
        (id: "core:building/court", cost: 1, effects: [
            (scope: City, selector: SourceCity, modifier: ModifyCost(target: TilePurchase, percent: -50)),
            (scope: City, selector: SourceCity, modifier: ModifyCost(target: Hurry, percent: -25)),
            (scope: City, selector: SourceCity, modifier: ModifyCost(target: BorderGrowth, percent: -40)),
            (scope: City, selector: SourceCity, condition: MinPopulation(3), modifier: AddYield(yield_kind: Gold, amount: 2)),
            (scope: City, selector: SourceCity, condition: LacksTechnology("core:tech/agriculture"), modifier: AddYield(yield_kind: Culture, amount: 1)),
        ]),
        (id: "core:building/camp", cost: 1, effects: [
            (scope: City, selector: SourceCity, trigger: OnBuilt, modifier: FreeUnit(unit: "core:unit/warrior", count: 2), duration: Once),
        ]),
    ],
    technologies: [(id: "core:tech/agriculture", cost: 1)],
)"#;

const P0: PlayerId = PlayerId(0);

fn setup() -> (GameState, CompiledRuleset) {
    let rules = crate::fixtures::compile_ron(CONTENT);
    let config = GameConfig {
        players: 2,
        width: 16,
        height: 16,
        rng_seed: 3,
    };
    (GameState::new(&rules, config), rules)
}

fn city_at(game: &mut GameState, rules: &CompiledRuleset, q: i32, r: i32) -> CityHandle {
    let dims = game.view().auth.map().dimensions;
    let tile = dims.to_tile(HexCoord { q, r }).unwrap();
    game.debug_found_city(rules, P0, tile)
}

fn building(rules: &CompiledRuleset, key: &str) -> BuildingKindId {
    BuildingKindId(rules.buildings.iter().position(|b| b.key == key).unwrap() as u16)
}

fn warrior(rules: &CompiledRuleset) -> UnitKindId {
    UnitKindId(
        rules
            .units
            .iter()
            .position(|u| u.key == "core:unit/warrior")
            .unwrap() as u16,
    )
}

/// Train one warrior in `city` (hurried with debug gold).
fn train_warrior(game: &mut GameState, rules: &CompiledRuleset, city: CityHandle) {
    let item = ProductionItem::Unit(warrior(rules));
    let enqueue = Command::EnqueueProduction {
        player: P0,
        city,
        item,
    };
    game.execute(rules, enqueue).unwrap();
    game.debug_earn(P0, 1000);
    let hurry = Command::HurryProduction { player: P0, city };
    game.execute(rules, hurry).unwrap();
}

fn shocked_warriors(game: &GameState) -> (usize, usize) {
    let shock = PromotionId(0);
    let units = game.view().auth.units();
    let all = units.iter_cores().count();
    let shocked = units
        .iter_cores()
        .filter(|u| u.promotions.contains(shock))
        .count();
    (shocked, all)
}

#[test]
fn units_trained_in_the_building_city_get_its_promotion() {
    let (mut game, rules) = setup();
    let with_barracks = city_at(&mut game, &rules, 3, 3);
    let without = city_at(&mut game, &rules, 11, 11);
    game.debug_add_building(
        &rules,
        with_barracks,
        building(&rules, "core:building/barracks"),
    );
    train_warrior(&mut game, &rules, with_barracks);
    train_warrior(&mut game, &rules, without);
    assert_eq!(
        shocked_warriors(&game),
        (1, 2),
        "only the barracks city promotes"
    );
    game.validate(&rules).unwrap();
}

#[test]
fn combat_contexts_apply_by_role() {
    let (mut game, rules) = setup();
    let city = city_at(&mut game, &rules, 3, 3);
    game.debug_add_building(&rules, city, building(&rules, "core:building/barracks"));
    train_warrior(&mut game, &rules, city);
    let unit = *game.view().auth.units().iter_cores().next().unwrap();
    let auth = game.view().auth;
    let mods = with_own_sources(auth, &rules, owner_unit_modifiers(auth, &rules, P0), &unit);
    let kind = warrior(&rules);
    assert_eq!(mods.strength(&rules, kind, CombatRole::Attack), 15);
    assert_eq!(mods.strength(&rules, kind, CombatRole::Defense), 12);
    assert_eq!(
        mods.strength(&rules, kind, CombatRole::AttackCity),
        25,
        "10 × (100+50+100)%"
    );
}

#[test]
fn per_citizen_yield_and_conditions_follow_the_city() {
    let (mut game, rules) = setup();
    let city = city_at(&mut game, &rules, 8, 8);
    game.debug_add_building(&rules, city, building(&rules, "core:building/library"));
    game.debug_add_building(&rules, city, building(&rules, "core:building/court"));
    let mods = |game: &GameState| {
        let core = game.view().auth.cities().get(city).unwrap();
        city_modifiers(game.view().auth, &rules, core)
    };
    let pop1 = mods(&game);
    assert_eq!(
        pop1.yields[YieldKind::Science.index()].flat,
        0,
        "50 × 1 / 100 = 0"
    );
    assert_eq!(
        pop1.yields[YieldKind::Gold.index()].flat,
        0,
        "MinPopulation(3) not met"
    );
    assert_eq!(
        pop1.yields[YieldKind::Culture.index()].flat,
        1,
        "agriculture unknown"
    );
    assert_eq!(pop1.adjusted_cost(CostTarget::TilePurchase, 50), 25);
    assert_eq!(pop1.adjusted_cost(CostTarget::Hurry, 40), 30);
    assert_eq!(pop1.adjusted_cost(CostTarget::BorderGrowth, 15), 9);
    let technology = crate::ids::TechId(0);
    game.execute(
        &rules,
        Command::ChooseTechnology {
            player: P0,
            technology,
        },
    )
    .unwrap();
    // Cost 1 is 2 science with the city-count penalty (rules.md).
    for _ in 0..4 {
        game.execute(&rules, Command::EndTurn { player: P0 })
            .unwrap();
    }
    assert_eq!(
        mods(&game).yields[YieldKind::Culture.index()].flat,
        0,
        "lost with the tech"
    );
}

#[test]
fn free_unit_count_spawns_that_many() {
    let (mut game, rules) = setup();
    let city = city_at(&mut game, &rules, 8, 8);
    let item = ProductionItem::Building(building(&rules, "core:building/camp"));
    let enqueue = Command::EnqueueProduction {
        player: P0,
        city,
        item,
    };
    game.execute(&rules, enqueue).unwrap();
    game.execute(&rules, Command::EndTurn { player: P0 })
        .unwrap();
    assert_eq!(game.view().auth.units().len(), 2);
    game.validate(&rules).unwrap();
}

#[test]
fn hurry_and_tile_prices_use_the_city_cost_modifiers() {
    let (mut game, rules) = setup();
    let city = city_at(&mut game, &rules, 8, 8);
    game.debug_add_building(&rules, city, building(&rules, "core:building/court"));
    let item = ProductionItem::Unit(warrior(&rules));
    let enqueue = Command::EnqueueProduction {
        player: P0,
        city,
        item,
    };
    game.execute(&rules, enqueue).unwrap();
    // Warrior 30: price ⌊900^0.75⌋ = 164 → 160, court −25% → 120.
    game.debug_earn(P0, 120);
    let before = game.view().auth.players().treasury_of(P0);
    game.execute(&rules, Command::HurryProduction { player: P0, city })
        .unwrap();
    assert_eq!(before - game.view().auth.players().treasury_of(P0), 120);
}
