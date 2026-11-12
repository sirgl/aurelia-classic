//! The model of the rivals (`ai::rival_model`, `rival-net`): it reads what the seat's public
//! screens show and nothing else (threshold M1 of the branch), and in a duel the demographics give
//! the rival's numbers exactly.

use classic_content::core::core_rules;
use classic_rules::ai::rival_model::{estimate, Book, Guess, MEASURES};
use classic_rules::derived::standing::{demographics, Measure};
use classic_rules::game::ClassicGame;
use classic_rules::ids::{PlayerId, TechId, TileId};
use classic_rules::math::HexCoord;
use classic_rules::rules::content::CityStateId;
use classic_rules::world::player::{ClassicKind, PlayerKind};
use classic_rules::{compile_rules, CompiledRuleset, GameConfig, GameState};

fn tile(game: &GameState, q: i32, r: i32) -> TileId {
    game.view().auth.map().dimensions.to_tile(HexCoord { q, r }).unwrap()
}

/// `players` nations with a city each, far apart; nation 1 knows a few more technologies and has a
/// larger city.
fn world(players: u8) -> (GameState, CompiledRuleset) {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = GameState::new(&rules, GameConfig { players, width: 48, height: 30, rng_seed: 21 });
    let spots = [(5, 5), (40, 22), (40, 5), (5, 22), (22, 14)];
    for p in 0..players {
        let (q, r) = spots[p as usize];
        let t = tile(&game, q, r);
        game.debug_found_city(&rules, PlayerId(p), t);
    }
    for t in 0..6 {
        game.debug_learn_tech(&rules, PlayerId(1), TechId(t));
    }
    let city = game.view().auth.indexes().cities_of(PlayerId(1))[0];
    game.debug_set_population(&rules, city, 4);
    (game, rules)
}

fn truth(game: &GameState, rules: &CompiledRuleset, p: PlayerId) -> [i64; MEASURES] {
    let view = game.view();
    let rows = demographics(view.auth, rules, view.derived, p);
    std::array::from_fn(|i| rows.iter().find(|d| d.measure == Measure::ALL[i]).unwrap().value)
}

/// Two nations, a city-state and the barbarians: the rows count the two nations only, so the
/// rival's every row, and its technologies from its literacy, are exact.
#[test]
fn a_duel_gives_the_rival_exactly_beside_city_states_and_barbarians() {
    let (mut game, rules) = world(4);
    game.debug_set_player_kind(PlayerId(2), PlayerKind::Module(ClassicKind::CityState(CityStateId(0))));
    game.debug_set_player_kind(PlayerId(3), PlayerKind::Module(ClassicKind::Barbarian));
    game.debug_meet(PlayerId(0), PlayerId(1));
    let obs = game.ai_observation(&rules, PlayerId(0));
    let model = estimate(&obs, &rules, &Book::default());
    assert_eq!(model.living, 2, "the city-state and the barbarians are not nations");
    assert_eq!(model.rivals.iter().map(|r| r.player).collect::<Vec<_>>(), vec![PlayerId(1)]);
    let rival = &model.rivals[0];
    let real = truth(&game, &rules, PlayerId(1));
    for (i, g) in rival.rows.iter().enumerate() {
        assert_eq!(*g, Guess::exact(real[i]), "{:?}", Measure::ALL[i]);
    }
    assert_eq!(rival.techs, Guess::exact(game.view().auth.players().techs_of(PlayerId(1)).len() as i64));
    let worst_population = obs.public.row(Measure::Population).unwrap().worst;
    assert!(worst_population > 0, "the barbarians (no city) are not the worst population");
}

/// M1: only nations met are estimated, and what no screen shows (a treasury, met or not) does
/// not move the estimate; a nation not met moves it only through the rows' range, as on the screen.
#[test]
fn the_model_reads_only_the_screens() {
    let (mut game, rules) = world(4);
    game.debug_meet(PlayerId(0), PlayerId(1));
    let before = estimate(&game.ai_observation(&rules, PlayerId(0)), &rules, &Book::default());
    assert_eq!(before.living, 4);
    assert_eq!(before.rivals.iter().map(|r| r.player).collect::<Vec<_>>(), vec![PlayerId(1)], "nations 2 and 3 are not met");
    assert!(before.rivals[0].rows.iter().any(|g| g.lo < g.hi), "with four nations a row is a range");
    game.debug_earn(PlayerId(1), 5_000);
    game.debug_earn(PlayerId(2), 5_000);
    let after = estimate(&game.ai_observation(&rules, PlayerId(0)), &rules, &Book::default());
    assert_eq!(after, before, "treasuries are on no screen");
    for t in 6..30 {
        game.debug_learn_tech(&rules, PlayerId(2), TechId(t));
    }
    let learned = estimate(&game.ai_observation(&rules, PlayerId(0)), &rules, &Book::default());
    assert!(learned.rivals.iter().all(|r| r.player == PlayerId(1)), "a nation not met is never estimated");
    let literacy = Measure::ALL.iter().position(|m| *m == Measure::Literacy).unwrap();
    assert!(learned.rivals[0].rows[literacy].hi > before.rivals[0].rows[literacy].hi, "the best literacy (nation 2's, unmet) widens the range");
}

/// The book: a sample of the screens, then a trend once it is old enough.
#[test]
fn the_book_gives_the_trends() {
    let (mut game, rules) = world(2);
    game.debug_meet(PlayerId(0), PlayerId(1));
    let mut book = Book::default();
    let first = estimate(&game.ai_observation(&rules, PlayerId(0)), &rules, &book);
    assert!(book.due(first.round, 100));
    book.record(PlayerId(0), &first, 100);
    assert_eq!(first.rivals[0].science, None, "no sample old enough");
    for t in 6..12 {
        game.debug_learn_tech(&rules, PlayerId(1), TechId(t));
    }
    // The same book read 20 rounds later.
    book.samples[0].round = first.round;
    let mut later_obs = game.ai_observation(&rules, PlayerId(0));
    later_obs.round = first.round + 20;
    let later = estimate(&later_obs, &rules, &book);
    let rival = &later.rivals[0];
    assert!(rival.science.is_some_and(|s| s > 0), "six technologies gained: {rival:?}");
    assert!(rival.score_trend.is_some_and(|t| t > 0), "its score rose: {rival:?}");
}

/// The book rides in the strategy net's memory before `sharp`'s section; both come back.
#[test]
fn the_book_rides_in_the_memory_with_sharps_section() {
    use classic_rules::ai::rival_model::Sample;
    use classic_rules::ai::strategy_net::{sharp, Memory};
    let book = Book { samples: vec![Sample { round: 12, nations: vec![(PlayerId(0), 40, 5), (PlayerId(3), 55, 7)] }, Sample { round: 17, nations: vec![(PlayerId(0), 44, 6)] }] };
    let plain = Memory { target: Some(TechId(4)), ..Default::default() };
    for sharp in [None, Some(sharp::Memory { ratio: 870, trend: 430, rivals: None, fits: None }), Some(sharp::Memory { ratio: 1, trend: 2, rivals: Some(3), fits: Some(true) })] {
        let memory = Memory { rivals: book.clone(), sharp, ..plain.clone() };
        assert_eq!(Memory::decode(&memory.encode()), memory);
        let without = Memory { rivals: Book::default(), ..memory.clone() };
        assert_eq!(Memory::decode(&without.encode()), without, "no book, no section");
    }
}

/// The game's AI keeps the book in its memory as it plays, and its net reads the model's inputs.
#[test]
fn the_standard_ai_keeps_the_book() {
    use classic_rules::ai::driver::ClassicMind;
    use classic_rules::ai::strategy_net::Memory;
    use classic_rules::ai::variant::Variant;
    use classic_rules::ai::AiSettings;
    use classic_rules::scenario::{generate_new_game, ScenarioConfig};
    use kernel::ai::driver::AiDriver;
    let rules = compile_rules(core_rules()).unwrap();
    let config = ScenarioConfig { seed: 0x1_7003, players: 4, width: 40, height: 28, cities: 0, units: 0, water_ratio: 20 };
    let mut game = generate_new_game(&rules, &config);
    let settings = AiSettings::for_rules(&rules, rules.default_difficulty());
    let mut driver = AiDriver::with_mind(settings, Vec::new(), ClassicMind::new(Variant::Standard));
    while game.view().auth.turn().round < 40 && game.winner().is_none() {
        driver.run(&mut game, &rules, u32::MAX);
    }
    let state = driver.states().get(PlayerId(0)).expect("an AI player");
    let memory = Memory::decode(&state.mind_memory);
    assert!(memory.target.is_some() || memory.survey.is_some(), "the net's memory decodes");
    let rounds: Vec<u32> = memory.rivals.samples.iter().map(|s| s.round).collect();
    assert!(rounds.len() >= 3, "samples every 5 rounds: {rounds:?}");
    assert!(memory.rivals.samples.iter().all(|s| s.nations.iter().any(|n| n.0 == PlayerId(0))), "the seat's own numbers in every sample");
}
