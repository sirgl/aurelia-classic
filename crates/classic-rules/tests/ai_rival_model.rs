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

/// N1: in a duel, before the meeting, the seven rows give the other nation exactly (beside a
/// city-state and the barbarians), its technologies come from its literacy, and nothing else of it
/// is read: no name, score, era, victory progress, science or trend; the net's inputs take the
/// rows and the technologies, and none of the rest. The meeting turns it into a rival met, whose
/// rows are the same numbers.
#[test]
fn n1_a_duel_not_met_gives_the_rows_exactly_and_nothing_else() {
    use classic_rules::ai::strategy_net::features::rivals as net_inputs;
    use classic_rules::ai::strategy_net::weights::fe;
    let (mut game, rules) = world(4);
    game.debug_set_player_kind(PlayerId(2), PlayerKind::Module(ClassicKind::CityState(CityStateId(0))));
    game.debug_set_player_kind(PlayerId(3), PlayerKind::Module(ClassicKind::Barbarian));
    let obs = game.ai_observation(&rules, PlayerId(0));
    assert!(obs.public.scores.iter().all(|(p, _)| *p == PlayerId(0)), "the score list does not show a nation not met");
    let model = estimate(&obs, &rules, &Book::default());
    assert_eq!(model.living, 2);
    assert!(model.rivals.is_empty(), "no rival is met: no name, score, era, capitals, delegates, influence");
    let unmet = model.unmet.as_ref().expect("the rows show the one other nation");
    let real = truth(&game, &rules, PlayerId(1));
    for (i, g) in unmet.rows.iter().enumerate() {
        assert_eq!(*g, Guess::exact(real[i]), "{:?}", Measure::ALL[i]);
    }
    assert_eq!(unmet.techs, Guess::exact(game.view().auth.players().techs_of(PlayerId(1)).len() as i64));
    // The inputs: the seven rows, the doubt and the technologies; the other seven stay 0.
    let mut inputs = [0; fe::N];
    net_inputs(&model, &mut inputs);
    for m in 0..MEASURES {
        let share = if real[m].max(0) + model.own[m].max(0) == 0 { 500 } else { real[m].max(0) * 1000 / (real[m].max(0) + model.own[m].max(0)) };
        assert_eq!(inputs[fe::RIVAL_FIRST + m], share, "{:?}", Measure::ALL[m]);
    }
    assert_eq!(inputs[fe::RIVAL_DOUBT], 0, "exact");
    assert!(inputs[fe::RIVAL_TREE] > 0, "its technologies are known from the literacy");
    for k in [fe::RIVAL_SCIENCE, fe::RIVAL_SCORE_TREND, fe::RIVAL_DOMINATION, fe::RIVAL_CULTURE, fe::RIVAL_DIPLOMACY] {
        assert_eq!(inputs[k], 0, "input {k} reads something no screen shows of a nation not met");
    }
    // What no screen shows does not move the model: its treasury, nor the capitals it holds.
    game.debug_earn(PlayerId(1), 5_000);
    let after = estimate(&game.ai_observation(&rules, PlayerId(0)), &rules, &Book::default());
    assert_eq!(after, model, "treasuries are on no screen");
    // The meeting: the same rows, now with a name.
    game.debug_meet(PlayerId(0), PlayerId(1));
    let met = estimate(&game.ai_observation(&rules, PlayerId(0)), &rules, &Book::default());
    assert!(met.unmet.is_none());
    assert_eq!(met.rivals[0].rows, unmet.rows);
    assert_eq!(met.rivals[0].techs, unmet.techs);
}

/// N1: with more than two nations alive a nation not met is never shown by itself, only inside the
/// ranges of the rows, met or not.
#[test]
fn n1_more_nations_not_met_stay_in_the_range() {
    use classic_rules::ai::strategy_net::features::rivals as net_inputs;
    use classic_rules::ai::strategy_net::weights::fe;
    let (mut game, rules) = world(5);
    let none = estimate(&game.ai_observation(&rules, PlayerId(0)), &rules, &Book::default());
    assert!(none.rivals.is_empty() && none.unmet.is_none(), "five nations, none met: nothing exact to tell");
    let mut inputs = [0; fe::N];
    net_inputs(&none, &mut inputs);
    assert!(inputs.iter().all(|v| *v == 0), "no rival met and none shown: no input");
    game.debug_meet(PlayerId(0), PlayerId(1));
    let one = estimate(&game.ai_observation(&rules, PlayerId(0)), &rules, &Book::default());
    assert!(one.unmet.is_none());
    assert_eq!(one.rivals.len(), 1);
    assert!(one.rivals[0].rows.iter().any(|g| g.lo < g.hi), "the nations not met widen the range");
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
        let memory = Memory { rival_book: book.clone(), sharp, ..plain.clone() };
        assert_eq!(Memory::decode(&memory.encode()), memory);
        let without = Memory { rival_book: Book::default(), ..memory.clone() };
        assert_eq!(Memory::decode(&without.encode()), without, "no book, no section");
    }
}

/// The domination input counts the capitals of other nations a rival holds: its own capital, held
/// by every nation, is not a conquest, so a meeting alone leaves the input at 0.
#[test]
fn the_domination_input_does_not_count_the_rivals_own_capital() {
    use classic_rules::ai::strategy_net::features::rivals as net_inputs;
    use classic_rules::ai::strategy_net::weights::fe;
    let (mut game, rules) = world(3);
    game.debug_meet(PlayerId(0), PlayerId(1));
    let obs = game.ai_observation(&rules, PlayerId(0));
    let mut model = estimate(&obs, &rules, &Book::default());
    assert_eq!(model.rivals.len(), 1);
    assert!(model.capitals_all >= 2, "the capitals of the nations are on the race screen");
    assert_eq!(model.rivals[0].capitals, 0, "it holds its own only");
    let mut inputs = [0; fe::N];
    net_inputs(&model, &mut inputs);
    assert_eq!(inputs[fe::RIVAL_DOMINATION], 0, "a meeting is no domination");
    // Holding every other capital is the whole scale.
    model.rivals[0].capitals = model.capitals_all - 1;
    net_inputs(&model, &mut inputs);
    assert_eq!(inputs[fe::RIVAL_DOMINATION], 1000);
}

/// The GNP row reads the gold from the cities' kept yields (`income::gold_now`): the same number as
/// the full income of every nation, in every state of a played game.
#[test]
fn the_gnp_from_kept_yields_is_the_full_income_gold() {
    use classic_rules::ai::driver::ClassicMind;
    use classic_rules::ai::variant::Variant;
    use classic_rules::ai::AiSettings;
    use classic_rules::derived::income::{gold_now, player_income};
    use classic_rules::scenario::{generate_new_game, ScenarioConfig};
    use kernel::ai::driver::AiDriver;
    let rules = compile_rules(core_rules()).unwrap();
    let config = ScenarioConfig { seed: 0x1_7004, players: 4, width: 40, height: 28, cities: 0, units: 0, water_ratio: 20 };
    let mut game = generate_new_game(&rules, &config);
    let settings = AiSettings::for_rules(&rules, rules.default_difficulty());
    let mut driver = AiDriver::with_mind(settings, Vec::new(), ClassicMind::new(Variant::Standard));
    let mut checked = 0;
    while game.view().auth.turn().round < 90 && game.winner().is_none() {
        driver.run(&mut game, &rules, 40);
        let view = game.view();
        for p in 0..4 {
            let full = player_income(view.auth, &rules, PlayerId(p)).gold;
            assert_eq!(gold_now(view.auth, &rules, view.derived, PlayerId(p)), full, "player {p} round {}", view.auth.turn().round);
            checked += 1;
        }
    }
    assert!(checked > 40, "many states checked: {checked}");
    assert!((0..4).any(|p| player_income(game.view().auth, &rules, PlayerId(p)).gold != 0), "a game with gold in it");
}

/// A cut or spoiled book is an empty book; the rest of the memory (target, the old format without the
/// section) survives, as a cut `sharp` section does.
#[test]
fn a_cut_book_leaves_the_rest_of_the_memory() {
    use classic_rules::ai::rival_model::Sample;
    use classic_rules::ai::strategy_net::Memory;
    let book = Book { samples: vec![Sample { round: 12, nations: vec![(PlayerId(0), 40, 5), (PlayerId(3), 55, 7)] }, Sample { round: 17, nations: vec![(PlayerId(0), 44, 6)] }] };
    let plain = Memory { target: Some(TechId(4)), ..Default::default() };
    // The old save: no section at all.
    let old = plain.encode();
    assert_eq!(Memory::decode(&old), plain, "an old save loads");
    // The book is the memory's tail here: cut it at every length.
    let full = Memory { rival_book: book.clone(), ..plain.clone() }.encode();
    assert!(full.len() > old.len() + 4);
    for cut in old.len() + 1..full.len() {
        let got = Memory::decode(&full[..cut]);
        assert_eq!(got.target, Some(TechId(4)), "cut at {cut}: the target stays");
        assert!(got.rival_book.samples.is_empty(), "cut at {cut}");
    }
    let got = Memory::decode(&full[..old.len() + 3]);
    assert_eq!(got, plain, "a cut book is no book");
    // A spoiled count with no samples behind it.
    let mut spoiled = old.clone();
    spoiled.extend_from_slice(&[0x5F, 200]);
    assert_eq!(Memory::decode(&spoiled), plain, "a spoiled book is no book");
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
    let rounds: Vec<u32> = memory.rival_book.samples.iter().map(|s| s.round).collect();
    assert!(rounds.len() >= 3, "samples every 5 rounds: {rounds:?}");
    assert!(memory.rival_book.samples.iter().all(|s| s.nations.iter().any(|n| n.0 == PlayerId(0))), "the seat's own numbers in every sample");
}

/// The retake section (0x5E), the rivals' book (0x5F) and `sharp`'s section (0x5D with the `next` fields) in one
/// byte array: they come in that order after the strategy memory's bytes, every marker where it belongs, and
/// everything reads back without a loss (the sections hold each other's offsets, so a swap or a wrong marker loses one).
#[test]
fn the_retake_book_and_sharp_sections_ride_together_without_loss() {
    use classic_rules::ai::retake::{Lost, Memory as Retake};
    use classic_rules::ai::rival_model::Sample;
    use classic_rules::ai::strategy_net::{sharp, Memory};
    let book = Book { samples: vec![Sample { round: 12, nations: vec![(PlayerId(0), 40, 5), (PlayerId(3), 55, 7)] }, Sample { round: 17, nations: vec![(PlayerId(0), 44, 6)] }] };
    let retake = Retake { held: vec![TileId(7), TileId(9)], lost: vec![Lost { tile: TileId(4), round: 12, captor: Some(PlayerId(1)) }] };
    let plain = Memory { target: Some(TechId(4)), ..Default::default() };
    let prefix = plain.encode();
    for sharp in [Some(sharp::Memory { ratio: 870, trend: 430, rivals: Some(3), fits: Some(true) }), Some(sharp::Memory { ratio: 5, trend: -6, rivals: None, fits: None })] {
        let memory = Memory { retake: Some(retake.clone()), rival_book: book.clone(), sharp, ..plain.clone() };
        let bytes = memory.encode();
        assert_eq!(Memory::decode(&bytes), memory, "all three sections come back");
        assert!(bytes.starts_with(&prefix), "the sections follow the bytes the other variants write");
        // The layout: 0x5E, its length (2 bytes) and body; then 0x5F and the book; then sharp's marker and numbers.
        let at = prefix.len();
        assert_eq!(bytes[at], 0x5E, "the retake marker");
        let len = usize::from(u16::from_le_bytes([bytes[at + 1], bytes[at + 2]]));
        assert_eq!(&bytes[at + 3..at + 3 + len], retake.encode().as_slice(), "the retake body");
        let book_at = at + 3 + len;
        assert_eq!(bytes[book_at], 0x5F, "the book marker follows the retake section");
        let mut book_bytes = Vec::new();
        book.encode(&mut book_bytes);
        assert_eq!(&bytes[book_at + 1..book_at + 1 + book_bytes.len()], book_bytes.as_slice(), "the book body");
        let sharp_at = book_at + 1 + book_bytes.len();
        let marker = if memory.sharp.is_some_and(|m| m.rivals.is_some() || m.fits.is_some()) { 0x5D } else { 0x5C };
        assert_eq!(bytes[sharp_at], marker, "sharp's marker is last");
        assert_eq!(bytes.len(), sharp_at + if marker == 0x5D { 1 + 8 * 4 } else { 1 + 8 * 2 }, "sharp's numbers end the memory");
        // Each pair of sections alone reads back the same.
        for (with_retake, with_book, with_sharp) in [(true, true, false), (true, false, true), (false, true, true)] {
            let part = Memory {
                retake: with_retake.then(|| retake.clone()),
                rival_book: if with_book { book.clone() } else { Book::default() },
                sharp: if with_sharp { memory.sharp } else { None },
                ..plain.clone()
            };
            assert_eq!(Memory::decode(&part.encode()), part, "retake {with_retake} book {with_book} sharp {with_sharp}");
        }
    }
}
