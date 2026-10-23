//! Runtime behaviour of the effect system: triggers fire in their phase,
//! instant modifiers change the world once, timed effects expire, passive
//! modifiers reach costs/combat/movement/unlocks, state survives save/load.

use crate::command::Command;
use crate::effects::city::city_modifiers;
use crate::effects::unit::{unit_modifiers, CombatRole};
use crate::ids::{
    BuildingKindId, CityHandle, PlayerId, ProductionItem, PromotionId, TechId, TileId, UnitHandle,
    UnitKindId,
};
use crate::math::HexCoord;
use crate::replay::authoritative_hash;
use crate::rules::effects::YieldKind;
use crate::rules::CompiledRuleset;
use crate::world::state::GameConfig;
use crate::GameState;

const CONTENT: &str = r#"(
    units: [
        (id: "core:unit/settler", movement: 2, strength: 1, founder: true, cost: 40),
        (id: "core:unit/warrior", movement: 2, strength: 10, cost: 30, effects: [
            (scope: Unit, selector: SourceUnit, trigger: OnBuilt, modifier: AddPromotion("core:promotion/drill"), duration: Once),
        ]),
        (id: "core:unit/archer", movement: 2, strength: 8, cost: 35, required_tech: Some("core:tech/archery")),
    ],
    promotions: [
        (id: "core:promotion/drill", effects: [(scope: Unit, selector: SourceUnit, modifier: ModifyCombat(context: Any, percent: 50))]),
        (id: "core:promotion/veteran", effects: [(scope: Unit, selector: SourceUnit, modifier: ModifyMovement(amount: 1))]),
    ],
    buildings: [
        (id: "core:building/barracks", cost: 1, effects: [
            (scope: City, selector: SourceCity, trigger: OnBuilt, modifier: FreeUnit(unit: "core:unit/settler", count: 1), duration: Once),
        ]),
        (id: "core:building/academy", cost: 1, effects: [
            (scope: Player, selector: Owner, modifier: UnlockUnit("core:unit/archer")),
            (scope: City, selector: OwnerCities, modifier: AddYield(yield_kind: Output, amount: 3), stacking: Unique),
        ]),
        (id: "core:building/forum", cost: 1, effects: [
            (scope: City, selector: OwnerCities, modifier: AddYield(yield_kind: Output, amount: 1)),
        ]),
        (id: "core:building/workshop", cost: 1, effects: [
            (scope: City, selector: SourceCity, modifier: ModifyCost(target: Units, percent: -50)),
        ]),
        (id: "core:building/shrine", cost: 1, effects: [
            (scope: City, selector: SourceCity, trigger: OnTurnStart, modifier: AddYield(yield_kind: Culture, amount: 4), duration: Once),
        ]),
    ],
    technologies: [
        (id: "core:tech/agriculture", cost: 1, effects: [
            (scope: Player, selector: Owner, trigger: OnTechnologyUnlocked, modifier: AddYield(yield_kind: Gold, amount: 50), duration: Once),
            (scope: City, selector: OwnerCities, trigger: OnTechnologyUnlocked, modifier: AddYield(yield_kind: Output, amount: 10), duration: Turns(2)),
        ]),
        (id: "core:tech/archery", cost: 1000),
        (id: "core:tech/masonry", cost: 1, effects: [
            (scope: City, selector: EventCity, trigger: OnCityFounded, modifier: FreeBuilding("core:building/shrine"), duration: Once),
            (scope: Unit, selector: EventUnit, trigger: OnUnitKilled, modifier: AddPromotion("core:promotion/veteran"), duration: Once),
            (scope: Unit, selector: OwnerUnits, modifier: ModifyMovement(amount: 1)),
        ]),
    ],
)"#;

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

fn setup() -> (GameState, CompiledRuleset) {
    let rules = crate::fixtures::compile_ron(CONTENT);
    let config = GameConfig {
        players: 2,
        width: 16,
        height: 16,
        rng_seed: 5,
    };
    (GameState::new(&rules, config), rules)
}

fn tile(game: &GameState, q: i32, r: i32) -> TileId {
    game.view()
        .auth
        .map()
        .dimensions
        .to_tile(HexCoord { q, r })
        .unwrap()
}

fn building(rules: &CompiledRuleset, key: &str) -> BuildingKindId {
    BuildingKindId(rules.buildings.iter().position(|b| b.key == key).unwrap() as u16)
}

fn unit_kind(rules: &CompiledRuleset, key: &str) -> UnitKindId {
    UnitKindId(rules.units.iter().position(|u| u.key == key).unwrap() as u16)
}

fn tech(rules: &CompiledRuleset, key: &str) -> TechId {
    TechId(
        rules
            .technologies
            .iter()
            .position(|t| t.key == key)
            .unwrap() as u16,
    )
}

fn promotion(rules: &CompiledRuleset, key: &str) -> PromotionId {
    PromotionId(rules.promotions.iter().position(|p| p.key == key).unwrap() as u16)
}

fn end_turn(game: &mut GameState, rules: &CompiledRuleset, player: PlayerId) {
    game.execute(rules, Command::EndTurn { player }).unwrap();
    game.validate(rules).unwrap();
}

fn learn(game: &mut GameState, rules: &CompiledRuleset, player: PlayerId, key: &str) {
    let technology = tech(rules, key);
    game.execute(rules, Command::ChooseTechnology { player, technology })
        .unwrap();
    // A technology of cost 1 costs 2 science with the city-count penalty
    // (rules.md, rounded up); the test city makes 1 a turn.
    for _ in 0..4 {
        end_turn(game, rules, player);
        if game.view().auth.players().techs_of(player).contains(&technology) {
            return;
        }
    }
    panic!("{key} not learned in 4 turns");
}

fn enqueue(game: &mut GameState, rules: &CompiledRuleset, city: CityHandle, item: ProductionItem) {
    let command = Command::EnqueueProduction {
        player: P0,
        city,
        item,
    };
    game.execute(rules, command).unwrap();
}

fn units_on(game: &GameState, t: TileId) -> Vec<UnitHandle> {
    game.view()
        .auth
        .units()
        .iter_handles()
        .filter(|(_, c)| c.tile == t)
        .map(|(h, _)| h)
        .collect()
}

#[test]
fn on_built_spawns_free_unit_and_promotes_trained_unit() {
    let (mut game, rules) = setup();
    let center = tile(&game, 8, 8);
    let city = game.debug_found_city(&rules, P0, center);
    enqueue(
        &mut game,
        &rules,
        city,
        ProductionItem::Building(building(&rules, "core:building/barracks")),
    );
    end_turn(&mut game, &rules, P0);
    let settler = unit_kind(&rules, "core:unit/settler");
    let on_center = units_on(&game, center);
    assert_eq!(on_center.len(), 1, "barracks OnBuilt: one free settler");
    assert_eq!(
        game.view()
            .auth
            .units()
            .get(on_center[0])
            .unwrap()
            .core
            .kind,
        settler
    );

    let warrior = unit_kind(&rules, "core:unit/warrior");
    enqueue(&mut game, &rules, city, ProductionItem::Unit(warrior));
    game.debug_earn(P0, 500);
    game.execute(&rules, Command::HurryProduction { player: P0, city })
        .unwrap();
    let trained = units_on(&game, center)
        .into_iter()
        .find(|h| game.view().auth.units().get(*h).unwrap().core.kind == warrior);
    let core = *game
        .view()
        .auth
        .units()
        .get(trained.expect("warrior trained"))
        .unwrap()
        .core;
    assert!(core
        .promotions
        .contains(promotion(&rules, "core:promotion/drill")));
    let mods = unit_modifiers(game.view().auth, &rules, &core);
    let strength = mods.strength(&rules, warrior, CombatRole::Attack);
    assert_eq!(strength, 15, "drill: +50% of 10");
    game.validate(&rules).unwrap();
}

#[test]
fn technology_grants_once_and_timed_effect_expires_after_its_turns() {
    let (mut game, rules) = setup();
    let city = game.debug_found_city(&rules, P0, tile(&game, 8, 8));
    let gold_before = game.view().auth.players().treasury_of(P0);
    learn(&mut game, &rules, P0, "core:tech/agriculture");
    let gold_after = game.view().auth.players().treasury_of(P0);
    assert!(
        gold_after >= gold_before + 50,
        "one-shot +50 gold on unlock"
    );
    let store = game.view().auth.active_effects();
    assert_eq!(store.len(), 1, "one timed entry per owner city");
    assert_eq!(
        store.iter().next().unwrap().turns_left,
        2,
        "not charged for its own resolution"
    );
    let flat = |game: &GameState| {
        let core = game.view().auth.cities().get(city).unwrap();
        city_modifiers(game.view().auth, &rules, core).yields[YieldKind::Output.index()].flat
    };
    assert_eq!(flat(&game), 10);
    end_turn(&mut game, &rules, P0);
    assert_eq!(
        game.view()
            .auth
            .active_effects()
            .iter()
            .next()
            .unwrap()
            .turns_left,
        1
    );
    end_turn(&mut game, &rules, P0);
    assert!(
        game.view().auth.active_effects().is_empty(),
        "expired after two resolutions"
    );
    assert_eq!(flat(&game), 0);
}

#[test]
fn city_founded_and_turn_start_effects_fire_in_their_phase() {
    let (mut game, rules) = setup();
    game.debug_found_city(&rules, P0, tile(&game, 2, 2));
    learn(&mut game, &rules, P0, "core:tech/masonry");
    let settler = unit_kind(&rules, "core:unit/settler");
    let unit = game.debug_spawn_unit(&rules, P0, tile(&game, 10, 10), settler);
    game.execute(&rules, Command::FoundCity { settler: unit })
        .unwrap();
    let city = game
        .view()
        .auth
        .indexes()
        .city_on_tile(tile(&game, 10, 10))
        .unwrap();
    let shrine = building(&rules, "core:building/shrine");
    assert!(game
        .view()
        .auth
        .cities()
        .get(city)
        .unwrap()
        .buildings
        .contains(&shrine));
    game.validate(&rules).unwrap();
    // Shrine: +4 culture at turn start (cities make no culture per
    // citizen: the grant is the only culture of this city).
    let culture = |game: &GameState| game.view().auth.cities().get(city).unwrap().culture;
    let before = culture(&game);
    end_turn(&mut game, &rules, P0);
    assert_eq!(culture(&game), before + 4);
}

#[test]
fn unit_killed_promotes_the_killer() {
    let (mut game, rules) = setup();
    game.debug_found_city(&rules, P0, tile(&game, 2, 2));
    learn(&mut game, &rules, P0, "core:tech/masonry");
    let warrior = unit_kind(&rules, "core:unit/warrior");
    let settler = unit_kind(&rules, "core:unit/settler");
    let killer = game.debug_spawn_unit(&rules, P0, tile(&game, 8, 8), warrior);
    // A civilian is captured, not killed: the victim is a wounded
    // warrior, killed by one strike.
    let _ = settler;
    let victim = game.debug_spawn_unit(&rules, P1, tile(&game, 9, 8), warrior);
    game.debug_set_unit_hp(victim, 1);
    game.execute(
        &rules,
        Command::DeclareWar {
            player: P0,
            enemy: P1,
        },
    )
    .unwrap();
    for _ in 0..10 {
        if game.view().auth.units().get(victim).is_none() {
            break;
        }
        let attack = Command::AttackUnit {
            attacker: killer,
            defender: victim,
        };
        game.execute(&rules, attack).unwrap();
        if game.view().auth.units().get(victim).is_some() {
            crate::fixtures::play_round(&mut game, &rules); // one attack per turn
        }
    }
    assert!(
        game.view().auth.units().get(victim).is_none(),
        "victim must die"
    );
    let core = *game.view().auth.units().get(killer).unwrap().core;
    assert!(core
        .promotions
        .contains(promotion(&rules, "core:promotion/veteran")));
    end_turn(&mut game, &rules, P0);
    let moves = game.view().auth.units().get(killer).unwrap().core.moves_max;
    assert_eq!(moves, (2 + 1 + 1) * 60, "base + masonry (owner units) + veteran");
}

#[test]
fn passive_unlock_and_cost_modifiers_apply() {
    let (mut game, rules) = setup();
    let city = game.debug_found_city(&rules, P0, tile(&game, 8, 8));
    let archer = ProductionItem::Unit(unit_kind(&rules, "core:unit/archer"));
    let blocked = Command::EnqueueProduction {
        player: P0,
        city,
        item: archer,
    };
    assert!(
        game.execute(&rules, blocked.clone()).is_err(),
        "archery unknown"
    );
    game.debug_add_building(&rules, city, building(&rules, "core:building/academy"));
    game.execute(&rules, blocked)
        .expect("academy unlocks the archer");
    let warrior = ProductionItem::Unit(unit_kind(&rules, "core:unit/warrior"));
    let cost = |game: &GameState| {
        let core = game.view().auth.cities().get(city).unwrap();
        city_modifiers(game.view().auth, &rules, core).production_cost(&rules, warrior)
    };
    assert_eq!(cost(&game), Some(30));
    game.debug_add_building(&rules, city, building(&rules, "core:building/workshop"));
    assert_eq!(cost(&game), Some(15), "workshop: units -50%");
    game.validate(&rules).unwrap();
}

#[test]
fn owner_wide_effects_stack_or_count_once() {
    let (mut game, rules) = setup();
    let a = game.debug_found_city(&rules, P0, tile(&game, 3, 3));
    let b = game.debug_found_city(&rules, P0, tile(&game, 11, 11));
    for city in [a, b] {
        game.debug_add_building(&rules, city, building(&rules, "core:building/academy"));
        game.debug_add_building(&rules, city, building(&rules, "core:building/forum"));
        // Incremental yields of BOTH cities must follow (oracle check).
        game.validate(&rules).unwrap();
    }
    for city in [a, b] {
        let core = game.view().auth.cities().get(city).unwrap();
        let mods = city_modifiers(game.view().auth, &rules, core);
        assert_eq!(
            mods.yields[YieldKind::Output.index()].flat,
            3 + 2,
            "Unique +3 once, Stack +1 per city"
        );
    }
}

#[test]
fn promotions_and_timed_effects_survive_save_load() {
    let (mut game, rules) = setup();
    let center = tile(&game, 8, 8);
    let city = game.debug_found_city(&rules, P0, center);
    learn(&mut game, &rules, P0, "core:tech/agriculture");
    let warrior = unit_kind(&rules, "core:unit/warrior");
    enqueue(&mut game, &rules, city, ProductionItem::Unit(warrior));
    game.debug_earn(P0, 500);
    game.execute(&rules, Command::HurryProduction { player: P0, city })
        .unwrap();
    let unit = units_on(&game, center)[0];
    let drill = promotion(&rules, "core:promotion/drill");
    assert!(game
        .view()
        .auth
        .units()
        .get(unit)
        .unwrap()
        .core
        .promotions
        .contains(drill));
    assert_eq!(game.view().auth.active_effects().len(), 1);
    let save = crate::persistence::encode::encode(&game, &rules);
    let loaded = crate::persistence::decode::decode(&save, &rules).unwrap();
    assert_eq!(authoritative_hash(&game), authoritative_hash(&loaded));
    assert_eq!(loaded.view().auth.active_effects().len(), 1);
    let loaded_unit = loaded.view().auth.units().get(unit).unwrap();
    assert!(loaded_unit.core.promotions.contains(drill));
    // A reference that does not resolve is corrupt data, not a panic.
    let mut broken = save.clone();
    broken.world.active_effects[0].slot = 99;
    assert!(crate::persistence::decode::decode(&broken, &rules).is_err());
    let mut bad_promotion = save;
    bad_promotion.world.units[0].promotions = vec![999];
    assert!(crate::persistence::decode::decode(&bad_promotion, &rules).is_err());
}
