//! The AI's missiles and nuclear weapons (`ai::strikes`, classic/docs/ai-strikes.md): a
//! missile in range of a war enemy's city that an army besieges is fired; a nuclear weapon
//! is fired only at a war enemy, never when one of our cities is in the blast; the cities
//! build the weapons a war operation can use. The scenes are open grassland.

use classic_content::core::core_rules;
use classic_rules::ai::driver::ClassicMind;
use classic_rules::ai::state::{AiState, Composition, Operation, OperationPhase, SeenCity};
use classic_rules::ai::strikes::{weapon, Weapon};
use classic_rules::ai::variant::Variant;
use classic_rules::ai::AiSettings;
use classic_rules::ids::{PlayerId, ProductionItem, TechId, TerrainId, TileId, UnitHandle, UnitKindId};
use classic_rules::math::HexCoord;
use classic_rules::{compile_rules, ClassicGame, Command, CompiledRuleset, GameConfig, GameState};
use kernel::ai::driver::AiDriver;
use kernel::ai::mind::AiMind;

const AI: PlayerId = PlayerId(0);
const ENEMY: PlayerId = PlayerId(1);
const THIRD: PlayerId = PlayerId(2);
const GRASSLAND: u16 = 1;

fn tile(game: &GameState, q: i32, r: i32) -> TileId {
    game.view().auth.map().dimensions.to_tile(HexCoord { q, r }).unwrap()
}

fn kind(rules: &CompiledRuleset, key: &str) -> UnitKindId {
    UnitKindId(rules.units.iter().position(|u| u.key == key).unwrap_or_else(|| panic!("{key}")) as u16)
}

fn tech(rules: &CompiledRuleset, key: &str) -> TechId {
    TechId(rules.technologies.iter().position(|t| t.key == key).unwrap_or_else(|| panic!("{key}")) as u16)
}

fn spawn(game: &mut GameState, rules: &CompiledRuleset, owner: PlayerId, q: i32, r: i32, key: &str) -> UnitHandle {
    let at = tile(game, q, r);
    game.debug_spawn_unit(rules, owner, at, kind(rules, key))
}

/// Open grassland, three players; the AI's capital (5, 10); the AI is at war with ENEMY, whose
/// city (13, 10) of six citizens stands 8 tiles off; THIRD is far away and at peace with all.
fn scene(rules: &CompiledRuleset) -> GameState {
    scene_of(rules, 3)
}

/// The scene with `players` players: the ones beyond the third live far away and, when
/// `players` is more than 3, have met the AI and are at peace with it (witnesses).
fn scene_of(rules: &CompiledRuleset, players: u8) -> GameState {
    let mut game = GameState::new(rules, GameConfig { players, width: 44, height: 20, rng_seed: 31 });
    let tiles = game.view().auth.map().dimensions.tile_count() as u32;
    let land: Vec<(TileId, TerrainId)> = (0..tiles).map(|t| (TileId(t), TerrainId(GRASSLAND))).collect();
    game.debug_set_terrain_batch(rules, &land);
    game.debug_found_city(rules, AI, tile(&game, 5, 10));
    let city = game.debug_found_city(rules, ENEMY, tile(&game, 13, 10));
    game.debug_set_population(rules, city, 6);
    game.debug_found_city(rules, THIRD, tile(&game, 40, 3));
    game.debug_meet(AI, ENEMY);
    for p in 3..players {
        game.debug_found_city(rules, PlayerId(p), tile(&game, 36, 2 * i32::from(p)));
        game.debug_meet(AI, PlayerId(p));
    }
    // The AI has explored the ground round the enemy city.
    let seen: Vec<TileId> = (8..=18).flat_map(|q| (6..=14).map(move |r| (q, r))).map(|(q, r)| tile(&game, q, r)).collect();
    game.debug_reveal(AI, &seen);
    game.execute(rules, Command::DeclareWar { player: AI, enemy: ENEMY }).unwrap();
    game
}

/// An army of three of ours stands next to the enemy city and an operation marches on it.
fn besiege(game: &mut GameState, rules: &CompiledRuleset) {
    for r in [9, 10, 11] {
        spawn(game, rules, AI, 11, r, "core:unit/swordsman");
    }
}

/// The commands the AI's first turn applied, in order.
fn first_turn(game: &mut GameState, rules: &CompiledRuleset, variant: Variant, operate: bool) -> Vec<Command> {
    let settings = AiSettings::for_rules(rules, rules.default_difficulty());
    let mut driver = AiDriver::with_mind(settings, vec![ENEMY, THIRD], ClassicMind::new(variant));
    driver.set_trace_capacity(100_000);
    if operate {
        let round = game.view().auth.turn().round;
        let enemy_city = tile(game, 13, 10);
        let memory = driver.state_mut(AI, round);
        memory.see_city(SeenCity { tile: enemy_city, owner: ENEMY, population: 6, last_seen_round: round });
        memory.start_operation(Operation {
            target: enemy_city,
            enemy: ENEMY,
            muster: tile(game, 5, 10),
            need: Composition { melee: 3, ranged: 0, siege: 0 },
            phase: OperationPhase::Marching,
            since_round: round,
            phase_round: round,
        });
    }
    driver.run(game, rules, u32::MAX);
    driver
        .trace()
        .entries()
        .filter(|e| e.outcome == kernel::ai::trace::Outcome::Applied)
        .filter_map(|e| e.command.clone())
        .collect()
}

#[test]
fn the_weapons_are_found_by_data() {
    let rules = compile_rules(core_rules()).unwrap();
    let named = |w: Weapon| -> Vec<&str> {
        (0..rules.units.len()).filter(|i| weapon(&rules, UnitKindId(*i as u16)) == Some(w)).map(|i| rules.units[i].name.as_str()).collect()
    };
    assert_eq!(named(Weapon::Missile), ["Guided Missile"]);
    assert_eq!(named(Weapon::Nuke(1)), ["Atomic Bomb"]);
    assert_eq!(named(Weapon::Nuke(2)), ["Nuclear Missile"]);
}

#[test]
fn a_missile_is_fired_at_a_city_an_army_besieges() {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = scene(&rules);
    let missile = spawn(&mut game, &rules, AI, 5, 10, "core:unit/guided-missile");
    besiege(&mut game, &rules);
    let target = tile(&game, 13, 10);
    let commands = first_turn(&mut game, &rules, Variant::Strikes, true);
    assert!(commands.iter().any(|c| matches!(c, Command::RangedAttack { unit, target: t } if *unit == missile && *t == target)), "{commands:?}");
}

/// The control: the generic advisor shoots the missile at anything in sight (the standard AI
/// never builds one; with one it fires without weighing the shot).
#[test]
fn the_standard_ai_fires_a_missile_it_has_by_the_generic_advisor() {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = scene(&rules);
    let missile = spawn(&mut game, &rules, AI, 5, 10, "core:unit/guided-missile");
    besiege(&mut game, &rules);
    let commands = first_turn(&mut game, &rules, Variant::Previous, true);
    assert!(commands.iter().any(|c| matches!(c, Command::RangedAttack { unit, .. } if *unit == missile)), "{commands:?}");
}

fn scene_with_scout(rules: &CompiledRuleset) -> GameState {
    let mut game = scene(rules);
    spawn(&mut game, rules, AI, 5, 10, "core:unit/guided-missile");
    spawn(&mut game, rules, AI, 10, 10, "core:unit/scout");
    spawn(&mut game, rules, ENEMY, 12, 10, "core:unit/scout");
    game
}

#[test]
fn a_missile_is_not_spent_on_a_lone_scout() {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = scene(&rules);
    let missile = spawn(&mut game, &rules, AI, 5, 10, "core:unit/guided-missile");
    spawn(&mut game, &rules, AI, 10, 10, "core:unit/scout");
    spawn(&mut game, &rules, ENEMY, 12, 10, "core:unit/scout");
    let control = first_turn(&mut scene_with_scout(&rules), &rules, Variant::Previous, false);
    assert!(control.iter().any(|c| matches!(c, Command::RangedAttack { .. })), "the generic advisor spends it: {control:?}");
    let commands = first_turn(&mut game, &rules, Variant::Strikes, false);
    assert!(!commands.iter().any(|c| matches!(c, Command::RangedAttack { unit, .. } if *unit == missile)), "{commands:?}");
}

#[test]
fn a_nuke_is_fired_at_the_city_of_a_war_enemy_that_an_army_besieges() {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = scene(&rules);
    let bomb = spawn(&mut game, &rules, AI, 5, 10, "core:unit/atomic-bomb");
    // The army keeps out of the blast (two tiles) and still counts as near.
    for r in [8, 10, 12] {
        spawn(&mut game, &rules, AI, 8, r, "core:unit/swordsman");
    }
    let target = tile(&game, 13, 10);
    let commands = first_turn(&mut game, &rules, Variant::Strikes, true);
    assert!(commands.iter().any(|c| matches!(c, Command::Nuke { unit, target: t } if *unit == bomb && *t == target)), "{commands:?}");
}

#[test]
fn a_nuke_is_never_fired_with_a_city_of_ours_in_the_blast() {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = scene(&rules);
    let bomb = spawn(&mut game, &rules, AI, 5, 10, "core:unit/atomic-bomb");
    game.debug_found_city(&rules, AI, tile(&game, 11, 10));
    for r in [8, 10, 12] {
        spawn(&mut game, &rules, AI, 8, r, "core:unit/swordsman");
    }
    let commands = first_turn(&mut game, &rules, Variant::Strikes, true);
    assert!(!commands.iter().any(|c| matches!(c, Command::Nuke { unit, .. } if *unit == bomb)), "{commands:?}");
}

#[test]
fn a_nuke_is_never_fired_when_a_player_not_at_war_is_in_the_blast() {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = scene(&rules);
    let bomb = spawn(&mut game, &rules, AI, 5, 10, "core:unit/atomic-bomb");
    // A unit of the third player, at peace with the AI, stands next to the enemy city.
    spawn(&mut game, &rules, THIRD, 14, 10, "core:unit/scout");
    for r in [8, 10, 12] {
        spawn(&mut game, &rules, AI, 8, r, "core:unit/swordsman");
    }
    let commands = first_turn(&mut game, &rules, Variant::Strikes, true);
    assert!(!commands.iter().any(|c| matches!(c, Command::Nuke { unit, .. } if *unit == bomb)), "{commands:?}");
}

#[test]
fn a_nuke_is_never_fired_at_a_player_not_at_war() {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = scene(&rules);
    game.execute(&rules, Command::MakePeace { player: AI, enemy: ENEMY }).ok();
    let at_war = game.view().auth.diplomacy().at_war(AI, ENEMY);
    let bomb = spawn(&mut game, &rules, AI, 5, 10, "core:unit/atomic-bomb");
    let commands = first_turn(&mut game, &rules, Variant::Strikes, true);
    assert!(at_war || !commands.iter().any(|c| matches!(c, Command::Nuke { unit, .. } if *unit == bomb)), "{commands:?}");
}

/// Everything on the way to a weapon the cities may build, and a war operation against a city
/// in its range.
fn armed_scene(rules: &CompiledRuleset, techs: &[&str], uranium: bool) -> (GameState, AiState, TileId) {
    let mut game = scene(rules);
    if uranium {
        let resource = classic_rules::ids::ResourceId(rules.resources.iter().position(|r| r.key == "core:resource/uranium").unwrap() as u16);
        let at = tile(&game, 5, 14);
        game.debug_set_resource_batch(rules, &[(at, resource)]);
    }
    let second = game.debug_found_city(rules, AI, tile(&game, 5, 14));
    game.debug_set_population(rules, second, 25);
    for key in techs {
        game.debug_learn(rules, AI, tech(rules, key));
    }
    game.debug_earn(AI, 5000);
    besiege(&mut game, rules);
    let enemy_city = tile(&game, 13, 10);
    let round = game.view().auth.turn().round;
    let mut state = AiState::new(AI, round);
    state.see_city(SeenCity { tile: enemy_city, owner: ENEMY, population: 6, last_seen_round: round });
    state.start_operation(Operation {
        target: enemy_city,
        enemy: ENEMY,
        muster: tile(&game, 5, 10),
        need: Composition { melee: 3, ranged: 0, siege: 0 },
        phase: OperationPhase::Forming,
        since_round: round,
        phase_round: round,
    });
    (game, state, enemy_city)
}

fn production_orders(rules: &CompiledRuleset, game: &GameState, state: &mut AiState, variant: Variant) -> Vec<ProductionItem> {
    let mind = ClassicMind::new(variant);
    let sight = mind.sense(game, rules, AI);
    let started = mind.start_turn(&sight, rules, AI, state);
    let settings = AiSettings::for_rules(rules, rules.default_difficulty());
    let mut scratch = Default::default();
    let mut counters = Default::default();
    let mut tools = kernel::ai::plan::PlanTools { rules, settings: &settings, scratch: &mut scratch, counters: &mut counters };
    mind.plan_end_of_turn(&sight, AI, &started.brief, state, &mut tools)
        .iter()
        .filter_map(|d| match &d.command {
            Some(Command::EnqueueProduction { item, .. }) => Some(*item),
            _ => None,
        })
        .collect()
}

#[test]
fn a_war_operation_makes_the_cities_build_a_missile() {
    let rules = compile_rules(core_rules()).unwrap();
    let (game, mut state, _) = armed_scene(&rules, &["core:tech/advanced-ballistics"], false);
    let ordered = production_orders(&rules, &game, &mut state, Variant::Strikes);
    let names = |items: &[ProductionItem]| -> Vec<String> { items.iter().map(|i| match i { ProductionItem::Unit(k) => rules.units[k.0 as usize].name.clone(), other => format!("{other:?}") }).collect() };
    assert!(ordered.iter().any(|i| matches!(i, ProductionItem::Unit(k) if weapon(&rules, *k) == Some(Weapon::Missile))), "{:?}", names(&ordered));
    let (game, mut state, _) = armed_scene(&rules, &["core:tech/advanced-ballistics"], false);
    let control = production_orders(&rules, &game, &mut state, Variant::Previous);
    assert!(!control.iter().any(|i| matches!(i, ProductionItem::Unit(k) if weapon(&rules, *k).is_some())), "{:?}", names(&control));
}

/// A nuclear weapon needs a project built first: the cities order the project, and the weapon
/// once the project stands.
#[test]
fn the_cities_build_the_project_a_nuclear_weapon_needs_and_then_the_weapon() {
    let rules = compile_rules(core_rules()).unwrap();
    let techs = ["core:tech/atomic-theory", "core:tech/nuclear-fission"];
    let (game, mut state, _) = armed_scene(&rules, &techs, true);
    let ordered = production_orders(&rules, &game, &mut state, Variant::Strikes);
    let project = rules.module.projects.iter().position(|p| p.allows_nukes).map(|i| classic_rules::rules::content::ProjectId(i as u16)).unwrap();
    assert!(ordered.contains(&ProductionItem::Project(project)), "{ordered:?}");
    assert!(!ordered.iter().any(|i| matches!(i, ProductionItem::Unit(k) if weapon(&rules, *k).is_some())), "the weapon waits for the project: {ordered:?}");

    let (mut game, mut state, _) = armed_scene(&rules, &techs, true);
    game.debug_add_project(AI, project);
    let ordered = production_orders(&rules, &game, &mut state, Variant::Strikes);
    assert!(ordered.iter().any(|i| matches!(i, ProductionItem::Unit(k) if matches!(weapon(&rules, *k), Some(Weapon::Nuke(_))))), "{ordered:?}");
    let (mut game, mut state, _) = armed_scene(&rules, &techs, true);
    game.debug_add_project(AI, project);
    let control = production_orders(&rules, &game, &mut state, Variant::Previous);
    assert!(!control.iter().any(|i| matches!(i, ProductionItem::Unit(k) if weapon(&rules, *k).is_some())), "{control:?}");
}

/// Peace: no weapon is built and none is fired.
#[test]
fn nothing_is_built_in_peace() {
    let rules = compile_rules(core_rules()).unwrap();
    let (mut game, mut state, _) = armed_scene(&rules, &["core:tech/advanced-ballistics"], false);
    game.execute(&rules, Command::MakePeace { player: AI, enemy: ENEMY }).ok();
    if game.view().auth.diplomacy().at_war(AI, ENEMY) {
        return;
    }
    let ordered = production_orders(&rules, &game, &mut state, Variant::Strikes);
    assert!(!ordered.iter().any(|i| matches!(i, ProductionItem::Unit(k) if weapon(&rules, *k).is_some())), "{ordered:?}");
}

/// The blast of a nuclear weapon is a public act (`ai::infamy`): each nation the AI has met
/// and is not at war with remembers it, and the AI counts that. With witnesses that cost more
/// than the strike gains, the weapon stays.
#[test]
fn a_nuke_waits_when_the_world_would_remember_it_for_more_than_it_gains() {
    let rules = compile_rules(core_rules()).unwrap();
    let fires = |players: u8| {
        let mut game = scene_of(&rules, players);
        let bomb = spawn(&mut game, &rules, AI, 5, 10, "core:unit/atomic-bomb");
        for r in [8, 10, 12] {
            spawn(&mut game, &rules, AI, 8, r, "core:unit/swordsman");
        }
        let settings = AiSettings::for_rules(&rules, rules.default_difficulty());
        let humans: Vec<PlayerId> = (1..players).map(PlayerId).collect();
        let mut driver = AiDriver::with_mind(settings, humans, ClassicMind::new(Variant::Strikes));
        driver.set_trace_capacity(100_000);
        let round = game.view().auth.turn().round;
        let enemy_city = tile(&game, 13, 10);
        let memory = driver.state_mut(AI, round);
        memory.see_city(SeenCity { tile: enemy_city, owner: ENEMY, population: 6, last_seen_round: round });
        memory.start_operation(Operation {
            target: enemy_city,
            enemy: ENEMY,
            muster: tile(&game, 5, 10),
            need: Composition { melee: 3, ranged: 0, siege: 0 },
            phase: OperationPhase::Marching,
            since_round: round,
            phase_round: round,
        });
        driver.run(&mut game, &rules, u32::MAX);
        let fired = driver.trace().entries().any(|e| matches!(e.command, Some(Command::Nuke { unit, .. }) if unit == bomb) && e.outcome == kernel::ai::trace::Outcome::Applied);
        fired
    };
    assert!(fires(3), "with nobody to remember it the strike pays");
    assert!(fires(4), "one witness: 2,400 of 7,071");
    assert!(!fires(7), "four witnesses cost 9,600 and the strike gains 7,071");
}

/// The AI's price of peace with an aggressor (`ai::infamy::peace_price`): the rival who has
/// just used a nuclear weapon offers white peace; the AI took it from a rival with a clean
/// memory and asks for a price from this one.
#[test]
fn peace_with_a_nation_that_used_a_nuclear_weapon_is_refused() {
    let rules = compile_rules(core_rules()).unwrap();
    let answer = |nuked: bool| {
        let mut game = scene(&rules);
        // The war has lasted (a young war cannot end): the proposer is the enemy.
        for _ in 0..classic_rules::MIN_WAR_ROUNDS_BEFORE_PEACE {
            for player in [AI, ENEMY, THIRD] {
                game.execute(&rules, Command::EndTurn { player }).unwrap();
            }
        }
        if nuked {
            // The enemy detonates a bomb on empty ground of its own, in sight of the AI.
            let bomb = spawn(&mut game, &rules, ENEMY, 13, 10, "core:unit/atomic-bomb");
            game.execute(&rules, Command::EndTurn { player: AI }).unwrap();
            let waste = tile(&game, 13, 14);
            game.debug_reveal(ENEMY, &[waste]);
            game.execute(&rules, Command::Nuke { unit: bomb, target: waste }).unwrap();
        } else {
            game.execute(&rules, Command::EndTurn { player: AI }).unwrap();
        }
        let offer = Command::ProposeDeal { from: ENEMY, to: AI, items: vec![(ENEMY, classic_rules::world::relations::DealItem::Peace)] };
        game.execute(&rules, offer).unwrap();
        game.execute(&rules, Command::EndTurn { player: ENEMY }).unwrap();
        game.execute(&rules, Command::EndTurn { player: THIRD }).unwrap();
        let settings = AiSettings::for_rules(&rules, rules.default_difficulty());
        let mut driver = AiDriver::with_mind(settings, vec![ENEMY, THIRD], ClassicMind::new(Variant::Standard));
        driver.set_trace_capacity(100_000);
        driver.run(&mut game, &rules, u32::MAX);
        let responded = driver.trace().entries().find_map(|e| match e.command {
            Some(Command::RespondDeal { accept, .. }) => Some(accept),
            _ => None,
        });
        let memory = game.view().auth.module().relations().warmonger(AI, ENEMY);
        (responded, memory)
    };
    let (clean, clean_memory) = answer(false);
    let (nuked, nuked_memory) = answer(true);
    assert!(nuked_memory > clean_memory + 100, "{nuked_memory} against {clean_memory}");
    assert_eq!(clean, Some(true), "the AI takes peace from a rival with a clean memory");
    assert_eq!(nuked, Some(false), "and not from the one that used a nuclear weapon");
}
