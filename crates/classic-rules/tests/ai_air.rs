//! The AI's air arm (`ai::air`, classic/docs/ai-air.md): aircraft strike what is in
//! reach and pays, rebase to the city nearest the front, sweep the
//! interceptors before a strike, wait to intercept, mend when hurt, and are
//! built for the war operations. The scenes are open grassland; the truth
//! is read only to set the scene and judge it.

use classic_content::core::core_rules;
use classic_rules::ai::air::{wing, Wing};
use classic_rules::ai::driver::ClassicMind;
use classic_rules::ai::state::{AiState, Composition, Operation, OperationPhase, SeenCity};
use classic_rules::ai::variant::Variant;
use classic_rules::ai::AiSettings;
use classic_rules::ids::{
    PlayerId, ProductionItem, TechId, TerrainId, TileId, UnitHandle, UnitKindId,
};
use classic_rules::math::HexCoord;
use classic_rules::{compile_rules, ClassicGame, Command, CompiledRuleset, GameConfig, GameState};
use kernel::ai::driver::AiDriver;
use kernel::ai::mind::AiMind;

const AI: PlayerId = PlayerId(0);
const ENEMY: PlayerId = PlayerId(1);
const GRASSLAND: u16 = 1;

fn tile(game: &GameState, q: i32, r: i32) -> TileId {
    game.view()
        .auth
        .map()
        .dimensions
        .to_tile(HexCoord { q, r })
        .unwrap()
}

fn kind(rules: &CompiledRuleset, key: &str) -> UnitKindId {
    UnitKindId(
        rules
            .units
            .iter()
            .position(|u| u.key == key)
            .unwrap_or_else(|| panic!("{key}")) as u16,
    )
}

fn spawn(
    game: &mut GameState,
    rules: &CompiledRuleset,
    owner: PlayerId,
    q: i32,
    r: i32,
    key: &str,
) -> UnitHandle {
    let at = tile(game, q, r);
    game.debug_spawn_unit(rules, owner, at, kind(rules, key))
}

/// Open grassland, the AI's capital (5, 10) and the enemy's far away at
/// war with it.
fn scene(rules: &CompiledRuleset) -> GameState {
    let mut game = GameState::new(
        rules,
        GameConfig {
            players: 2,
            width: 44,
            height: 20,
            rng_seed: 31,
        },
    );
    let tiles = game.view().auth.map().dimensions.tile_count() as u32;
    let land: Vec<(TileId, TerrainId)> = (0..tiles)
        .map(|t| (TileId(t), TerrainId(GRASSLAND)))
        .collect();
    game.debug_set_terrain_batch(rules, &land);
    game.debug_found_city(rules, AI, tile(&game, 5, 10));
    game.debug_found_city(rules, ENEMY, tile(&game, 41, 18));
    game.debug_meet(AI, ENEMY);
    game.execute(
        rules,
        Command::DeclareWar {
            player: AI,
            enemy: ENEMY,
        },
    )
    .unwrap();
    game
}

/// The commands the AI's first turn applied, in order, with who issued them.
fn first_turn(
    game: &mut GameState,
    rules: &CompiledRuleset,
    variant: Variant,
) -> Vec<(kernel::ai::trace::Actor, kernel::ai::trace::Reason, Command)> {
    let settings = AiSettings::for_rules(rules, rules.default_difficulty());
    let mut driver = AiDriver::with_mind(settings, vec![ENEMY], ClassicMind::new(variant));
    driver.set_trace_capacity(100_000);
    driver.run(game, rules, u32::MAX);
    driver
        .trace()
        .entries()
        .filter(|e| e.outcome == kernel::ai::trace::Outcome::Applied)
        .filter_map(|e| e.command.clone().map(|c| (e.actor, e.reason, c)))
        .collect()
}

#[test]
fn the_wings_are_found_by_data() {
    let rules = compile_rules(core_rules()).unwrap();
    let named = |w: Wing| -> Vec<&str> {
        (0..rules.units.len())
            .filter(|i| wing(&rules, UnitKindId(*i as u16)) == Some(w))
            .map(|i| rules.units[i].name.as_str())
            .collect()
    };
    let strikers = named(Wing::Striker);
    let fighters = named(Wing::Fighter);
    assert!(
        strikers.contains(&"Bomber") && strikers.contains(&"Great War Bomber"),
        "{strikers:?}"
    );
    assert!(
        fighters.contains(&"Fighter") && fighters.contains(&"Jet Fighter"),
        "{fighters:?}"
    );
    for never in ["Atomic Bomb", "Guided Missile"] {
        let k = kind(
            &rules,
            &format!("core:unit/{}", never.to_lowercase().replace(' ', "-")),
        );
        assert_eq!(
            wing(&rules, k),
            None,
            "{never} is never commanded by the air arm"
        );
    }
}

#[test]
fn a_bomber_strikes_an_enemy_unit_in_range() {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = scene(&rules);
    let bomber = spawn(&mut game, &rules, AI, 5, 10, "core:unit/bomber");
    spawn(&mut game, &rules, AI, 8, 10, "core:unit/scout");
    spawn(&mut game, &rules, ENEMY, 10, 10, "core:unit/archer");
    let target = tile(&game, 10, 10);
    let commands = first_turn(&mut game, &rules, Variant::Standard);
    let shot = commands
        .iter()
        .find(|(_, _, c)| matches!(c, Command::RangedAttack { unit, .. } if *unit == bomber));
    assert!(
        matches!(shot, Some((_, _, Command::RangedAttack { target: t, .. })) if *t == target),
        "{commands:?}"
    );
}

#[test]
fn the_control_without_an_air_arm_shoots_by_the_generic_advisor_and_does_nothing_else() {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = scene(&rules);
    let bomber = spawn(&mut game, &rules, AI, 5, 10, "core:unit/bomber");
    let fighter = spawn(&mut game, &rules, AI, 5, 10, "core:unit/fighter");
    spawn(&mut game, &rules, AI, 8, 10, "core:unit/scout");
    spawn(&mut game, &rules, ENEMY, 10, 10, "core:unit/archer");
    let commands = first_turn(&mut game, &rules, Variant::NoAir);
    let shot = commands
        .iter()
        .find(|(_, _, c)| matches!(c, Command::RangedAttack { unit, .. } if *unit == bomber));
    assert!(
        matches!(
            shot,
            Some((
                kernel::ai::trace::Actor::Player,
                kernel::ai::trace::Reason::SystemAdvisor,
                _
            ))
        ),
        "{commands:?}"
    );
    assert!(!commands.iter().any(|(_, _, c)| matches!(c, Command::Intercept { unit } | Command::Rebase { unit, .. } if *unit == fighter || *unit == bomber)), "{commands:?}");
}

#[test]
fn a_bomber_rebases_to_the_city_nearest_the_front() {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = scene(&rules);
    let front = game.debug_found_city(&rules, AI, tile(&game, 24, 10));
    let bomber = spawn(&mut game, &rules, AI, 5, 10, "core:unit/bomber");
    spawn(&mut game, &rules, AI, 25, 10, "core:unit/scout");
    spawn(&mut game, &rules, ENEMY, 27, 10, "core:unit/archer");
    let commands = first_turn(&mut game, &rules, Variant::Standard);
    let moved = commands
        .iter()
        .find(|(_, _, c)| matches!(c, Command::Rebase { unit, .. } if *unit == bomber));
    assert!(
        matches!(moved, Some((_, kernel::ai::trace::Reason::Rebase, Command::Rebase { city, .. })) if *city == front),
        "{commands:?}"
    );
}

#[test]
fn an_idle_fighter_waits_to_intercept_once() {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = scene(&rules);
    let fighter = spawn(&mut game, &rules, AI, 5, 10, "core:unit/fighter");
    let commands = first_turn(&mut game, &rules, Variant::Standard);
    let stances = commands
        .iter()
        .filter(|(_, _, c)| matches!(c, Command::Intercept { unit } if *unit == fighter))
        .count();
    assert_eq!(stances, 1, "{commands:?}");
}

/// The interceptors over a target are swept before the bomber goes in.
#[test]
fn a_fighter_sweeps_the_interceptors_before_the_bomber_strikes() {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = scene(&rules);
    let bomber = spawn(&mut game, &rules, AI, 5, 10, "core:unit/bomber");
    let fighter = spawn(&mut game, &rules, AI, 5, 10, "core:unit/fighter");
    spawn(&mut game, &rules, AI, 10, 10, "core:unit/scout");
    spawn(&mut game, &rules, ENEMY, 11, 10, "core:unit/archer");
    let guard = spawn(&mut game, &rules, ENEMY, 12, 10, "core:unit/fighter");
    game.execute(&rules, Command::EndTurn { player: AI })
        .unwrap();
    game.execute(&rules, Command::Intercept { unit: guard })
        .unwrap();
    game.execute(&rules, Command::EndTurn { player: ENEMY })
        .unwrap();
    let commands = first_turn(&mut game, &rules, Variant::Standard);
    let at = |pred: &dyn Fn(&Command) -> bool| commands.iter().position(|(_, _, c)| pred(c));
    let sweep = at(&|c| matches!(c, Command::AirSweep { unit, .. } if *unit == fighter));
    let strike = at(&|c| matches!(c, Command::RangedAttack { unit, .. } if *unit == bomber));
    assert!(sweep.is_some(), "{commands:?}");
    if let Some(strike) = strike {
        assert!(
            sweep.unwrap() < strike,
            "the sweep comes first: {commands:?}"
        );
    }
}

#[test]
fn a_hurt_bomber_mends_instead_of_striking() {
    let rules = compile_rules(core_rules()).unwrap();
    let struck = |variant: Variant| {
        let mut game = scene(&rules);
        let bomber = spawn(&mut game, &rules, AI, 5, 10, "core:unit/bomber");
        game.debug_set_unit_hp(bomber, 30);
        spawn(&mut game, &rules, AI, 8, 10, "core:unit/scout");
        spawn(&mut game, &rules, ENEMY, 10, 10, "core:unit/archer");
        let commands = first_turn(&mut game, &rules, variant);
        commands.iter().any(|(_, _, c)| matches!(c, Command::RangedAttack { unit, .. } | Command::Rebase { unit, .. } if *unit == bomber))
    };
    assert!(!struck(Variant::Standard));
    assert!(
        struck(Variant::NoAir),
        "the control's generic advisor sends the hurt bomber"
    );
}

/// A bomber is not sent into a city whose visible fighter and anti-aircraft
/// gun would kill it (the forecast of the rules, `air_strike_risk`); the
/// control's generic advisor sends it.
#[test]
fn a_bomber_does_not_strike_at_a_loss() {
    let rules = compile_rules(core_rules()).unwrap();
    let struck = |variant: Variant| {
        let mut game = scene(&rules);
        let bomber = spawn(&mut game, &rules, AI, 5, 10, "core:unit/bomber");
        spawn(&mut game, &rules, AI, 9, 10, "core:unit/scout");
        let city = game.debug_found_city(&rules, ENEMY, tile(&game, 11, 10));
        game.debug_set_population(&rules, city, 6);
        for key in [
            "core:unit/musketman",
            "core:unit/musketman",
            "core:unit/anti-aircraft-gun",
        ] {
            spawn(&mut game, &rules, ENEMY, 11, 10, key);
        }
        let guard = spawn(&mut game, &rules, ENEMY, 11, 10, "core:unit/fighter");
        // The rival's fighter takes the interception posture on its turn.
        game.execute(&rules, Command::EndTurn { player: AI })
            .unwrap();
        game.execute(&rules, Command::Intercept { unit: guard })
            .unwrap();
        game.execute(&rules, Command::EndTurn { player: ENEMY })
            .unwrap();
        let commands = first_turn(&mut game, &rules, variant);
        commands
            .iter()
            .any(|(_, _, c)| matches!(c, Command::RangedAttack { unit, .. } if *unit == bomber))
    };
    assert!(
        !struck(Variant::Standard),
        "the forecast says the bomber dies"
    );
    assert!(
        struck(Variant::NoAir),
        "the control strikes it (no interception in its preview)"
    );
}

/// A war operation asks for strikers: the cities build them when the
/// technology and the strategic resource are there.
#[test]
fn a_war_operation_makes_the_cities_build_aircraft() {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = scene(&rules);
    let tech = |key: &str| {
        TechId(
            rules
                .technologies
                .iter()
                .position(|t| t.key == key)
                .unwrap() as u16,
        )
    };
    // Oil under a city of the AI, and everything on the way to Radar.
    let resource = classic_rules::ids::ResourceId(
        rules
            .resources
            .iter()
            .position(|r| r.key == "core:resource/oil")
            .unwrap() as u16,
    );
    let oil = tile(&game, 8, 10);
    game.debug_set_resource_batch(&rules, &[(oil, resource)]);
    game.debug_found_city(&rules, AI, oil);
    game.debug_found_city(&rules, AI, tile(&game, 5, 14));
    game.debug_learn(&rules, AI, tech("core:tech/biology"));
    for key in ["core:tech/radar"] {
        game.debug_learn(&rules, AI, tech(key));
    }
    game.debug_earn(AI, 5000);
    let enemy_city = tile(&game, 13, 10);
    game.debug_found_city(&rules, ENEMY, enemy_city);
    let round = game.view().auth.turn().round;
    let mind = ClassicMind::new(Variant::Standard);
    let mut state = AiState::new(AI, round);
    state.see_city(SeenCity {
        tile: enemy_city,
        owner: ENEMY,
        population: 1,
        last_seen_round: round,
    });
    state.start_operation(Operation {
        target: enemy_city,
        enemy: ENEMY,
        muster: tile(&game, 5, 10),
        need: Composition {
            melee: 2,
            ranged: 0,
            siege: 0,
        },
        phase: OperationPhase::Marching,
        since_round: round,
        phase_round: round,
    });
    let sight = mind.sense(&game, &rules, AI);
    let started = mind.start_turn(&sight, &rules, AI, &mut state);
    let settings = AiSettings::for_rules(&rules, rules.default_difficulty());
    let mut scratch = Default::default();
    let mut counters = Default::default();
    let mut tools = kernel::ai::plan::PlanTools {
        rules: &rules,
        settings: &settings,
        scratch: &mut scratch,
        counters: &mut counters,
    };
    let orders = mind.plan_end_of_turn(&sight, AI, &started.brief, &state, &mut tools);
    let ordered: Vec<&UnitKindId> = orders
        .iter()
        .filter_map(|d| match &d.command {
            Some(Command::EnqueueProduction {
                item: ProductionItem::Unit(k),
                ..
            }) => Some(k),
            _ => None,
        })
        .collect();
    assert!(
        ordered
            .iter()
            .any(|k| wing(&rules, **k) == Some(Wing::Striker)),
        "{:?}",
        ordered
            .iter()
            .map(|k| &rules.units[k.0 as usize].name)
            .collect::<Vec<_>>()
    );
    let control = ClassicMind::new(Variant::NoAir);
    let mut state = AiState::new(AI, round);
    state.start_operation(Operation {
        target: enemy_city,
        enemy: ENEMY,
        muster: tile(&game, 5, 10),
        need: Composition {
            melee: 2,
            ranged: 0,
            siege: 0,
        },
        phase: OperationPhase::Marching,
        since_round: round,
        phase_round: round,
    });
    let sight = control.sense(&game, &rules, AI);
    let started = control.start_turn(&sight, &rules, AI, &mut state);
    let orders = control.plan_end_of_turn(&sight, AI, &started.brief, &state, &mut tools);
    assert!(!orders.iter().any(|d| matches!(&d.command, Some(Command::EnqueueProduction { item: ProductionItem::Unit(k), .. }) if wing(&rules, *k).is_some())));
}

/// The forecast of an air strike (`air_strike_risk`) against what the rules
/// really do: over many dice, the mean damage taken by the bomber is near
/// the forecast answer plus the chance-weighted interception.
#[test]
fn the_forecast_of_an_air_strike_matches_the_dice() {
    use classic_rules::knowledge::public::PlayerQueries;
    let rules = compile_rules(core_rules()).unwrap();
    for (name, with_guards) in [
        ("city with a fighter and an AA gun", true),
        ("city alone", false),
    ] {
        let (mut forecast_total, mut actual_total, mut n) = (0.0, 0.0, 0.0);
        for seed in 0..60u64 {
            let mut game = GameState::new(
                &rules,
                GameConfig {
                    players: 2,
                    width: 44,
                    height: 20,
                    rng_seed: seed,
                },
            );
            let tiles = game.view().auth.map().dimensions.tile_count() as u32;
            let land: Vec<(TileId, TerrainId)> = (0..tiles)
                .map(|t| (TileId(t), TerrainId(GRASSLAND)))
                .collect();
            game.debug_set_terrain_batch(&rules, &land);
            game.debug_found_city(&rules, AI, tile(&game, 5, 10));
            let city = game.debug_found_city(&rules, ENEMY, tile(&game, 11, 10));
            game.debug_set_population(&rules, city, 6);
            let bomber = spawn(&mut game, &rules, AI, 5, 10, "core:unit/bomber");
            spawn(&mut game, &rules, AI, 10, 10, "core:unit/scout");
            spawn(&mut game, &rules, ENEMY, 11, 10, "core:unit/musketman");
            let fighter = with_guards.then(|| {
                spawn(
                    &mut game,
                    &rules,
                    ENEMY,
                    11,
                    10,
                    "core:unit/anti-aircraft-gun",
                );
                spawn(&mut game, &rules, ENEMY, 11, 10, "core:unit/fighter")
            });
            game.debug_meet(AI, ENEMY);
            game.execute(
                &rules,
                Command::DeclareWar {
                    player: AI,
                    enemy: ENEMY,
                },
            )
            .unwrap();
            game.execute(&rules, Command::EndTurn { player: AI })
                .unwrap();
            if let Some(f) = fighter {
                game.execute(&rules, Command::Intercept { unit: f })
                    .unwrap();
            }
            game.execute(&rules, Command::EndTurn { player: ENEMY })
                .unwrap();
            let target = tile(&game, 11, 10);
            let risk = {
                let obs = game.ai_observation(&rules, AI);
                obs.view
                    .air_strike_risk(bomber, target)
                    .expect("the bomber can strike")
            };
            game.execute(
                &rules,
                Command::RangedAttack {
                    unit: bomber,
                    target,
                },
            )
            .unwrap();
            let left = game
                .view()
                .auth
                .units()
                .get(bomber)
                .map_or(0, |u| u.core.hp);
            forecast_total += f64::from(risk.answer)
                + f64::from(risk.interception) * f64::from(risk.chance) / 100.0;
            actual_total += f64::from(100 - left);
            n += 1.0;
        }
        let (forecast, actual) = (forecast_total / n, actual_total / n);
        eprintln!("{name}: forecast {forecast:.1} hp, actual mean {actual:.1} hp");
        if with_guards {
            // Neither side has oil: Unciv's flat -25 % missing-resource penalty (was up to -50 %)
            // leaves the bomber sturdier, so the guards take most of its HP (forecast 87, actual
            // 86 at commit time) instead of killing it outright.
            assert!(
                forecast >= 80.0 && actual >= 80.0 && (forecast - actual).abs() <= 8.0 + 0.2 * actual,
                "{name}: a fighter on duty nearly kills the bomber: forecast {forecast:.1}, actual {actual:.1}"
            );
        } else {
            assert!(
                (forecast - actual).abs() <= 8.0 + 0.2 * actual,
                "{name}: forecast {forecast:.1} against {actual:.1}"
            );
        }
    }
}
