//! Economy decisions of the AI (`ai::improvements`, `ai::citizens`, ...):
//! they read only the observation — two worlds that look the same to the
//! player give the same commands — and pick the sensible order.

use classic_rules::ClassicGame;
use classic_content::core::core_rules;
use classic_rules::ai::citizens::focus_commands;
use classic_rules::ai::improvements::{best_worker_job, worker_commands};
use classic_rules::ids::{PlayerId, ResourceId, TechId, TerrainId, TileId, UnitKindId};
use classic_rules::math::HexCoord;
use classic_rules::world::citizens::CityFocus;
use classic_rules::{compile_rules, Command, CompiledRuleset, GameConfig, GameState};

const P0: PlayerId = PlayerId(0);

fn rules() -> CompiledRuleset {
    compile_rules(core_rules()).unwrap()
}

fn game(rules: &CompiledRuleset) -> GameState {
    let mut game = GameState::new(
        rules,
        GameConfig {
            players: 1,
            width: 20,
            height: 20,
            rng_seed: 1,
        },
    );
    let all: Vec<_> = (0..400).map(|i| (TileId(i), TerrainId(3))).collect();
    game.debug_set_terrain_batch(rules, &all);
    game
}

fn tile(game: &GameState, q: i32, r: i32) -> TileId {
    game.view()
        .auth
        .map()
        .dimensions
        .to_tile(HexCoord { q, r })
        .unwrap()
}

fn learn(game: &mut GameState, rules: &CompiledRuleset, key: &str) {
    let t = TechId(
        rules
            .technologies
            .iter()
            .position(|t| t.key == key)
            .unwrap() as u16,
    );
    game.debug_learn(rules, P0, t);
}

fn worker(rules: &CompiledRuleset) -> UnitKindId {
    UnitKindId(
        rules
            .units
            .iter()
            .position(|u| u.key == "core:unit/worker")
            .unwrap() as u16,
    )
}

#[test]
fn an_idle_worker_farms_grassland_near_the_city() {
    let rules = rules();
    let mut game = game(&rules);
    let grass = tile(&game, 9, 8);
    game.debug_set_terrain_batch(&rules, &[(grass, TerrainId(1))]);
    game.debug_found_city(&rules, P0, tile(&game, 8, 8));
    learn(&mut game, &rules, "core:tech/agriculture");
    let w = game.debug_spawn_unit(&rules, P0, grass, worker(&rules));
    let obs = game.ai_observation(&rules, P0);
    let unit = obs.own_units.iter().find(|u| u.handle == w).unwrap();
    let job = best_worker_job(&obs, &rules, unit).expect("a farm is worth it");
    assert_eq!(job.tile, grass);
    assert_eq!(
        rules.improvements[job.improvement.0 as usize].key,
        "core:improvement/farm"
    );
    let commands = worker_commands(&obs, &rules);
    assert!(matches!(commands[0], Command::BuildImprovement { unit, .. } if unit == w));
    // The game accepts the order.
    game.execute(&rules, commands[0].clone()).unwrap();
}

#[test]
fn an_idle_worker_mines_known_iron_before_farming() {
    // A grassland farm under the worker against a mine on known iron next
    // to it (both in the city's ring): the iron connects
    // (ai::improvements::STRATEGIC_CONNECTION).
    let rules = rules();
    let mut game = game(&rules);
    let grass = tile(&game, 9, 8);
    let ore = tile(&game, 9, 7);
    game.debug_set_terrain_batch(&rules, &[(grass, TerrainId(1)), (ore, TerrainId(1))]);
    let iron = ResourceId(
        rules
            .resources
            .iter()
            .position(|r| r.key == "core:resource/iron")
            .unwrap() as u16,
    );
    game.debug_set_resource_batch(&rules, &[(ore, iron)]);
    game.debug_found_city(&rules, P0, tile(&game, 8, 8));
    for key in ["core:tech/agriculture", "core:tech/mining", "core:tech/bronze-working", "core:tech/iron-working"] {
        learn(&mut game, &rules, key);
    }
    let w = game.debug_spawn_unit(&rules, P0, grass, worker(&rules));
    let obs = game.ai_observation(&rules, P0);
    let unit = obs.own_units.iter().find(|u| u.handle == w).unwrap();
    let job = best_worker_job(&obs, &rules, unit).expect("work to do");
    assert_eq!(job.tile, ore);
    assert_eq!(rules.improvements[job.improvement.0 as usize].key, "core:improvement/mine");
}

/// Headless all-AI classic game: the AI orders no unit or building it lacks
/// the resources for (the core refuses none for that), and no empire runs a
/// deficit from production.
#[test]
fn ai_production_respects_strategic_resources() {
    use classic_rules::ai::trace::Outcome;
    use classic_rules::ai::{AiDriver, AiSettings};
    use classic_rules::scenario::{generate_new_game, ScenarioConfig};
    let rules = rules();
    let config = ScenarioConfig {
        seed: 0x1_7011,
        players: 4,
        width: 40,
        height: 28,
        cities: 0,
        units: 0,
        water_ratio: 20,
    };
    let mut game = generate_new_game(&rules, &config);
    let mut driver = AiDriver::new(AiSettings::for_rules(&rules, rules.default_difficulty()), Vec::new());
    driver.set_trace_capacity(200_000);
    while game.view().auth.turn().round < 150 && game.winner().is_none() {
        driver.run(&mut game, &rules, u32::MAX);
    }
    game.validate(&rules).unwrap();
    let refused = driver
        .trace()
        .entries()
        .filter(|e| matches!(&e.outcome, Outcome::Rejected(err) if format!("{err:?}").contains("needs ")))
        .count();
    assert_eq!(refused, 0, "no order without its resources");
    let mut connected = 0;
    for p in 0..4 {
        let ledger = classic_rules::derived::resources::resource_ledger(game.view().auth, &rules, PlayerId(p));
        // A luxury given away by a deal and then lost is a deficit of the
        // deal, not of production: only what units and buildings consume counts.
        let consumed = |r: &classic_rules::ids::ResourceId| {
            rules.units.iter().any(|u| u.required_resources.iter().any(|(x, _)| x == r))
                || rules.buildings.iter().any(|b| b.consumes_resources.iter().any(|(x, _)| x == r))
        };
        assert_eq!(ledger.deficits().filter(consumed).count(), 0, "player {p} has no deficit");
        connected += ledger.known().count();
    }
    assert!(connected > 0, "the empires connected resources");
}

#[test]
fn hidden_resources_do_not_change_worker_orders() {
    // Iron under a hills tile is hidden without Bronze Working: the world
    // with it and the world without it look the same to the player.
    let rules = rules();
    let build = |with_iron: bool| {
        let mut game = game(&rules);
        let hills = tile(&game, 9, 8);
        game.debug_set_terrain_batch(&rules, &[(hills, TerrainId(2))]);
        if with_iron {
            let iron = ResourceId(
                rules
                    .resources
                    .iter()
                    .position(|r| r.key == "core:resource/iron")
                    .unwrap() as u16,
            );
            game.debug_set_resource_batch(&rules, &[(hills, iron)]);
        }
        game.debug_found_city(&rules, P0, tile(&game, 8, 8));
        learn(&mut game, &rules, "core:tech/mining");
        game.debug_spawn_unit(&rules, P0, hills, worker(&rules));
        let obs = game.ai_observation(&rules, P0);
        format!("{:?}", worker_commands(&obs, &rules))
    };
    assert_eq!(build(true), build(false));
}

#[test]
fn small_cities_focus_on_food() {
    let rules = rules();
    let mut game = game(&rules);
    let city = game.debug_found_city(&rules, P0, tile(&game, 8, 8));
    let obs = game.ai_observation(&rules, P0);
    let commands = focus_commands(&obs, &rules);
    assert_eq!(commands.len(), 1);
    assert!(
        matches!(commands[0], Command::SetCityFocus { focus: CityFocus::Food, city: c, .. } if c == city)
    );
    game.execute(&rules, commands[0].clone()).unwrap();
    let obs = game.ai_observation(&rules, P0);
    assert!(focus_commands(&obs, &rules).is_empty(), "no flip-flop");
}

fn unit_kind(rules: &CompiledRuleset, key: &str) -> UnitKindId {
    UnitKindId(rules.units.iter().position(|u| u.key == key).unwrap() as u16)
}

#[test]
fn great_people_are_used_and_the_game_accepts_the_orders() {
    use classic_rules::ai::great_people::{great_person_action, great_person_commands};
    use classic_rules::derived::great_people::GreatPersonAction;
    let rules = rules();
    let mut game = game(&rules);
    let centre = tile(&game, 8, 8);
    let near = tile(&game, 8, 9);
    game.debug_found_city(&rules, P0, centre);
    let artist = game.debug_spawn_unit(&rules, P0, centre, unit_kind(&rules, "core:unit/great-artist"));
    let scientist = game.debug_spawn_unit(&rules, P0, near, unit_kind(&rules, "core:unit/great-scientist"));
    let general = game.debug_spawn_unit(&rules, P0, centre, unit_kind(&rules, "core:unit/great-general"));
    let obs = game.ai_observation(&rules, P0);
    let action = |h| great_person_action(&obs, &rules, obs.own_units.iter().find(|u| u.handle == h).unwrap());
    assert_eq!(action(artist), Some(GreatPersonAction::StartGoldenAge));
    // No research queued: the scientist builds its academy on the spot.
    assert!(matches!(action(scientist), Some(GreatPersonAction::Construct(_))));
    assert_eq!(action(general), None, "no citadel inside the city");
    let commands = great_person_commands(&obs, &rules);
    assert_eq!(commands.len(), 2);
    for command in commands {
        game.execute(&rules, command).unwrap();
    }
    game.validate(&rules).unwrap();
}

/// Headless all-AI classic game: the economy advisor's orders (city focus,
/// great person actions) reach the game through the driver.
#[test]
fn the_ai_driver_uses_the_economy_advisor_in_a_headless_game() {
    use classic_rules::ai::trace::{Outcome, Reason};
    use classic_rules::ai::{AiDriver, AiSettings};
    use classic_rules::derived::great_people::GreatPersonAction;
    use classic_rules::scenario::{generate_new_game, ScenarioConfig};
    let rules = rules();
    let config = ScenarioConfig {
        seed: 0xEC0_A1,
        players: 3,
        width: 32,
        height: 24,
        cities: 0,
        units: 0,
        water_ratio: 15,
    };
    let mut game = generate_new_game(&rules, &config);
    let mut driver = AiDriver::new(AiSettings::for_rules(&rules, rules.default_difficulty()), Vec::new());
    driver.record_commands();
    driver.set_trace_capacity(100_000);
    let artist = unit_kind(&rules, "core:unit/great-artist");
    let mut rounds = 0;
    let mut gifted = false;
    while rounds < 30 {
        let report = driver.run(&mut game, &rules, 1000);
        if !report.round_done {
            continue;
        }
        rounds += 1;
        // Round 10: every AI gets a great artist in its first city.
        let cities: Vec<_> = game.view().auth.cities().iter().map(|(_, c)| (c.owner, c.tile)).collect();
        if rounds == 10 && !cities.is_empty() {
            for (owner, tile) in cities.iter().take(3) {
                game.debug_spawn_unit(&rules, *owner, *tile, artist);
            }
            gifted = true;
        }
    }
    game.validate(&rules).unwrap();
    assert!(gifted, "cities were founded");
    let advised = driver
        .trace()
        .entries()
        .filter(|e| e.reason == Reason::SystemAdvisor && e.outcome == Outcome::Applied)
        .count();
    assert!(advised > 0, "the economy advisor's orders were applied");
    let commands = format!("{:?}", driver.commands());
    assert!(commands.contains("SetCityFocus"), "cities get a focus");
    let golden = GreatPersonAction::StartGoldenAge;
    assert!(commands.contains(&format!("{golden:?}")), "great artists start golden ages");
    let ages: u16 = (0..3)
        .filter_map(|p| game.view().auth.players().economy_of(PlayerId(p)).map(|e| e.golden_ages))
        .sum();
    assert!(ages > 0, "a golden age happened");
}

#[test]
fn an_unhappy_empire_queues_a_happiness_building_and_stops_expanding() {
    use classic_rules::ai::happiness::happiness_commands;
    let rules = rules();
    let mut game = game(&rules);
    // Three size-6 cities: 9 − (9 + 18) < 0, each training a warrior (a
    // city with an empty queue gets its order from the planner).
    let warrior = unit_kind(&rules, "core:unit/warrior");
    for q in [4, 10, 16] {
        let city = game.debug_found_city(&rules, P0, tile(&game, q, 8));
        game.debug_grow_city(&rules, city, 5);
        let item = classic_rules::ids::ProductionItem::Unit(warrior);
        game.execute(&rules, Command::EnqueueProduction { player: P0, city, item }).unwrap();
    }
    learn(&mut game, &rules, "core:tech/construction");
    let obs = game.ai_observation(&rules, P0);
    assert!(obs.economy.happiness.excess() < 0);
    let commands = happiness_commands(&obs, &rules);
    assert_eq!(commands.len(), 1);
    let Command::EnqueueProduction { item, city: first, .. } = commands[0].clone() else {
        panic!("{commands:?}");
    };
    let classic_rules::ids::ProductionItem::Building(kind) = item else {
        panic!("{item:?}");
    };
    assert_eq!(rules.buildings[kind.0 as usize].key, "core:building/colosseum");
    game.execute(&rules, commands[0].clone()).unwrap();
    let obs = game.ai_observation(&rules, P0);
    // The next order goes to another city (one order per turn, one
    // happiness building per city queue).
    let next = happiness_commands(&obs, &rules);
    assert!(matches!(next[..], [Command::EnqueueProduction { city, .. }] if city != first), "{next:?}");
    let brief = classic_rules::ai::planner::TurnBrief::from_observation(&obs, &rules);
    assert!(!classic_rules::ai::planner_strategy::happiness_allows_city(&brief));
}

#[test]
fn an_empire_losing_gold_works_gold_tiles() {
    use classic_rules::ai::citizens::{city_focus, GOLD_COMFORT, GOLD_FOCUS_TURNS};
    let rules = rules();
    let mut game = game(&rules);
    // Grassland: the city feeds itself (a starving city works food first).
    let all: Vec<_> = (0..400).map(|i| (TileId(i), TerrainId(1))).collect();
    game.debug_set_terrain_batch(&rules, &all);
    let city = game.debug_found_city(&rules, P0, tile(&game, 8, 8));
    game.debug_grow_city(&rules, city, 4);
    let warrior = unit_kind(&rules, "core:unit/warrior");
    for _ in 0..20 {
        game.debug_spawn_unit(&rules, P0, tile(&game, 9, 9), warrior);
    }
    let obs = game.ai_observation(&rules, P0);
    assert!(obs.economy.income.gold < 0, "{:?}", obs.economy.income);
    let own = &obs.own_cities[0];
    let income = obs.economy.income.gold;
    // A treasury that cannot carry the deficit works gold; a full one pays it.
    assert_eq!(city_focus(&rules, own, income, 0), CityFocus::Gold);
    assert_eq!(city_focus(&rules, own, income, income.unsigned_abs() * GOLD_FOCUS_TURNS), CityFocus::Default);
    // Kept until the empire earns GOLD_COMFORT a turn (no flip-flop).
    assert_eq!(city_focus(&rules, own, GOLD_COMFORT + 1, 0), CityFocus::Default);
}

/// Headless all-AI classic games: the AI builds trade units and starts routes
/// (ai::trade through the economy advisor); the core refuses none. Whether a route is
/// started in 150 rounds of one game depends on the turn order of its first rounds (a game
/// of the sweep below that begins one in 150 rounds is one in three), so the games of a sweep
/// of seeds are counted together.
#[test]
fn the_ai_runs_trade_routes() {
    use classic_rules::ai::trace::Outcome;
    use classic_rules::ai::{AiDriver, AiSettings};
    use classic_rules::scenario::{generate_new_game, ScenarioConfig};
    let rules = rules();
    let (mut games_with_routes, mut refused_total) = (0, 0);
    for k in 0..12u64 {
        let config = ScenarioConfig { seed: 0x7_4ADE + k, players: 4, width: 40, height: 28, cities: 0, units: 0, water_ratio: 20 };
        let mut game = generate_new_game(&rules, &config);
        let mut driver = AiDriver::new(AiSettings::for_rules(&rules, rules.default_difficulty()), Vec::new());
        driver.set_trace_capacity(300_000);
        driver.record_commands();
        let mut most_routes = 0;
        while game.view().auth.turn().round < 150 && game.winner().is_none() {
            driver.run(&mut game, &rules, u32::MAX);
            most_routes = most_routes.max(game.view().auth.module().trade().routes().len());
        }
        game.validate(&rules).unwrap();
        let refused = driver
            .trace()
            .entries()
            .filter(|e| matches!(e.command, Some(Command::EstablishTradeRoute { .. })))
            .filter(|e| matches!(e.outcome, Outcome::Rejected(_)))
            .count();
        eprintln!("trade: seed {k}: most routes at once {most_routes}, refused {refused}");
        games_with_routes += usize::from(most_routes > 0);
        refused_total += refused;
    }
    assert!(games_with_routes >= 2, "the AI started trade routes in {games_with_routes} of 12 games");
    assert_eq!(refused_total, 0, "every route the AI picked was allowed");
}
