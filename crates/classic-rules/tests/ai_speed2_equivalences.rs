//! The structure changes that made the AI's turn cheaper a second time (`perf/ai-turn2`) change no answer:
//! each fast version is compared with the plain one it replaced, on the worlds of whole AI games, and the
//! comparisons are between real, non-zero values (each test counts what it compared and asserts the count).
//!
//! * the job offers of the workers are remembered by what they read (`planner_workers::best_job_memo`);
//! * a search prices the steps out of a tile with the tile read once (`planner_map::StepsFrom`);
//! * a review's runs of the economy cell share the base choices of the cities (`cell::BaseChoices`);
//! * the observation's public screens are built when first read (`observation::LazyScreens`).

use classic_content::core::core_rules;
use classic_rules::ai::planner::TurnBrief;
use classic_rules::ai::planner_map::{step_points, KnownMap, StepsFrom};
use classic_rules::ai::planner_workers::offer_memo_and_fresh;
use classic_rules::ai::state::{Strategy, WorkerJob};
use classic_rules::ai::{AiDriver, AiSettings};
use classic_rules::ids::{PlayerId, TileId};
use classic_rules::knowledge::observation::{public_screens, AIObservation};
use classic_rules::scenario::{generate_new_game, ScenarioConfig};
use classic_rules::{compile_rules, CompiledRuleset, GameState};

const STRATEGIES: [Strategy; 3] = [Strategy::Expand, Strategy::Defend, Strategy::Stabilize];

fn ai_game(rules: &CompiledRuleset, seed: u64) -> (GameState, AiDriver) {
    let config = ScenarioConfig { seed, players: 4, width: 48, height: 36, cities: 0, units: 0, water_ratio: 20 };
    let game = generate_new_game(rules, &config);
    let driver = AiDriver::new(AiSettings::for_rules(rules, rules.default_difficulty()), Vec::new());
    (game, driver)
}

fn play_to(game: &mut GameState, driver: &mut AiDriver, rules: &CompiledRuleset, round: u32) {
    while game.view().auth.turn().round < round && game.winner().is_none() {
        if driver.run(game, rules, u32::MAX).waiting {
            break;
        }
    }
}

/// The tiles of `player`'s land that are not cities.
fn own_land(obs: &AIObservation<'_>) -> Vec<TileId> {
    let count = obs.view.tile_count() as u32;
    (0..count).map(TileId).filter(|t| obs.view.tile(*t).snapshot().is_some_and(|s| s.owner == Some(obs.player) && !s.city)).collect()
}

/// What the compared offers were: how many, how many offered a job, which jobs.
#[derive(Default, Debug)]
struct Seen {
    compared: usize,
    offers: usize,
    improvements: usize,
    chops: usize,
    gains_over_zero: usize,
}

fn compare_offers(game: &GameState, rules: &CompiledRuleset, tiles_of: &dyn Fn(&AIObservation<'_>) -> Vec<TileId>, seen: &mut Seen, context: &str) -> usize {
    let auth = game.view().auth;
    let derived = game.view().derived;
    let mut mismatches = 0;
    for p in 0..auth.players().count() {
        let (obs, _) = AIObservation::for_player(auth, derived, rules, PlayerId(p as u8));
        let brief = TurnBrief::from_observation(&obs, rules);
        for tile in tiles_of(&obs) {
            for strategy in STRATEGIES {
                let (memo, fresh) = offer_memo_and_fresh(&obs.view, rules, &brief, strategy, tile);
                seen.compared += 1;
                if let Some((job, gain, _)) = fresh {
                    seen.offers += 1;
                    seen.gains_over_zero += usize::from(gain > 0);
                    match job {
                        WorkerJob::Improvement(_) => seen.improvements += 1,
                        WorkerJob::Chop => seen.chops += 1,
                        WorkerJob::Road => {}
                    }
                }
                if memo != fresh {
                    mismatches += 1;
                    eprintln!("{context}: player {p} {tile:?} {strategy:?}: memo {memo:?}, fresh {fresh:?}");
                }
            }
        }
    }
    mismatches
}

/// The memo answers as `best_job` computes, every few rounds of AI games for every player, with the memo kept
/// warm from one comparison to the next (so a tile whose neighbourhood changed in between must be asked
/// again), and after changes made to the neighbours of the tiles it holds: an improvement the neighbour
/// cannot have next to the same one, a feature or a deposit beside it, a tile of another owner, a technology.
#[test]
fn the_offer_memo_answers_as_the_job_finder_does_along_ai_games_and_after_changes_beside_the_tiles() {
    let rules = compile_rules(core_rules()).unwrap();
    let mut seen = Seen::default();
    let mut forced_flips = 0;
    for seed in [3, 11] {
        let (mut game, mut driver) = ai_game(&rules, seed);
        for round in (10..=130).step_by(10) {
            play_to(&mut game, &mut driver, &rules, round);
            assert_eq!(compare_offers(&game, &rules, &own_land, &mut seen, &format!("seed {seed} round {round}")), 0);
        }
        // A technology learnt: the memo of the player starts over, the offers follow.
        for tech in 0..rules.technologies.len() {
            for p in 0..game.view().auth.players().count() {
                game.debug_learn(&rules, PlayerId(p as u8), classic_rules::ids::TechId(tech as u16));
            }
        }
        assert_eq!(compare_offers(&game, &rules, &own_land, &mut seen, "every technology learnt"), 0);
        // Changes beside the tiles the memo holds: the tiles of the player with the most offers, each neighbour
        // set to every improvement that may not stand next to itself, to a feature that gives fresh water, to a
        // luxury deposit; the offers of the tile and of the neighbour compared after each.
        let (who, own): (PlayerId, Vec<TileId>) = (0..game.view().auth.players().count())
            .map(|p| {
                let (obs, _) = AIObservation::for_player(game.view().auth, game.view().derived, &rules, PlayerId(p as u8));
                let mut tiles = own_land(&obs);
                tiles.retain(|t| obs.view.tile(*t).snapshot().is_some_and(|s| s.improvement.is_none()));
                tiles.truncate(150);
                (PlayerId(p as u8), tiles)
            })
            .max_by_key(|(_, t)| t.len())
            .unwrap();
        assert!(own.len() > 20, "a player holds land to change beside");
        if std::env::var("DIAG").is_ok() {
            let (obs, _) = AIObservation::for_player(game.view().auth, game.view().derived, &rules, who);
            let brief = TurnBrief::from_observation(&obs, &rules);
            let some = own.iter().filter(|t| offer_memo_and_fresh(&obs.view, &rules, &brief, Strategy::Expand, **t).1.is_some()).count();
            eprintln!("DIAG own {} with an offer {} techs {}", own.len(), some, brief.techs.len());
            for t in own.iter().take(5) { let s = obs.view.tile(*t).snapshot().copied().unwrap(); eprintln!("DIAG {t:?} {s:?}"); }
        }
        let dims = game.view().auth.map().dimensions;
        let twin: Vec<_> = (0..rules.improvements.len()).filter(|i| rules.improvements[*i].placement.no_two_adjacent).map(|i| classic_rules::ids::ImprovementId(i as u16)).collect();
        let fresh_features: Vec<_> = (0..rules.features.len()).filter(|i| rules.features[*i].fresh_water).map(|i| classic_rules::ids::FeatureId(i as u16)).collect();
        let luxuries: Vec<_> = (0..rules.resources.len()).filter(|i| rules.resources[*i].class == Some(classic_rules::rules::content::ResourceClass::Luxury)).map(|i| classic_rules::ids::ResourceId(i as u16)).collect();
        assert!(!twin.is_empty() && !fresh_features.is_empty() && !luxuries.is_empty(), "the rules have what the changes use");
        let only = |tile: TileId| move |_: &AIObservation<'_>| vec![tile];
        for tile in own {
            let before = |game: &GameState| -> [Option<_>; 3] {
                let (obs, _) = AIObservation::for_player(game.view().auth, game.view().derived, &rules, who);
                let brief = TurnBrief::from_observation(&obs, &rules);
                STRATEGIES.map(|st| offer_memo_and_fresh(&obs.view, &rules, &brief, st, tile).1)
            };
            for near in dims.neighbors(tile) {
                let was = before(&game);
                for kind in twin.iter() {
                    game.debug_set_improvement_batch(&rules, &[(near, *kind)]);
                    game.debug_reveal(who, &[near]);
                    assert_eq!(compare_offers(&game, &rules, &only(tile), &mut seen, "a twin improvement beside"), 0);
                }
                game.debug_set_feature_batch(&rules, &[(near, fresh_features[0])]);
                game.debug_reveal(who, &[near]);
                assert_eq!(compare_offers(&game, &rules, &only(tile), &mut seen, "a feature with fresh water beside"), 0);
                game.debug_set_resource_batch(&rules, &[(near, luxuries[0])]);
                game.debug_reveal(who, &[near]);
                assert_eq!(compare_offers(&game, &rules, &only(tile), &mut seen, "a luxury beside"), 0);
                forced_flips += usize::from(before(&game) != was);
                if std::env::var("DIAG").is_ok() {
                    let (obs, _) = AIObservation::for_player(game.view().auth, game.view().derived, &rules, who);
                    let sn = obs.view.tile(near).snapshot().copied();
                    eprintln!("DIAG {tile:?} near {near:?} imp {:?} feat {:?} res {:?} was {was:?}", sn.and_then(|s| s.improvement), sn.and_then(|s| s.feature), sn.and_then(|s| s.resource));
                }
            }
        }
    }
    eprintln!("{seen:?}, offers changed by a change beside the tile: {forced_flips}");
    assert!(seen.compared > 1000, "{seen:?}");
    assert!(seen.improvements > 200 && seen.gains_over_zero > 200, "real offers were compared: {seen:?}");
    assert!(seen.chops > 0, "offers to chop a feature were compared: {seen:?}");
    assert!(forced_flips > 0, "a change beside a tile changes its offer, so the keys of the neighbours are read");
}

/// `StepsFrom` prices a step as `step_points` does: for the tiles of real maps, every neighbour, a land mover, a
/// unit of the player's own and a mover of the sea; the steps priced are real (some known, some not).
#[test]
fn the_steps_out_of_a_tile_cost_what_the_plain_step_costs() {
    let rules = compile_rules(core_rules()).unwrap();
    let (mut some, mut none, mut distinct) = (0, 0, std::collections::BTreeSet::new());
    for seed in [3, 7] {
        let (mut game, mut driver) = ai_game(&rules, seed);
        play_to(&mut game, &mut driver, &rules, 60);
        let auth = game.view().auth;
        for p in 0..auth.players().count() {
            let (obs, _) = AIObservation::for_player(auth, game.view().derived, &rules, PlayerId(p as u8));
            let mut maps = vec![KnownMap::land(&obs.view)];
            maps.extend(obs.own_units.iter().take(4).map(|u| KnownMap::of_unit(&obs.view, u.handle)));
            let dims = obs.view.dimensions();
            for map in &maps {
                for t in (0..obs.view.tile_count() as u32).step_by(3) {
                    let from = TileId(t);
                    let steps = StepsFrom::new(map, from);
                    for to in dims.neighbors(from) {
                        let (fast, plain) = (steps.to(to), step_points(map, from, to));
                        assert_eq!(fast, plain, "{from:?} -> {to:?}");
                        match plain {
                            Some(points) => {
                                some += 1;
                                distinct.insert(points);
                            }
                            None => none += 1,
                        }
                    }
                }
            }
        }
    }
    assert!(some > 1000 && none > 100, "known and unknown steps were priced ({some}, {none})");
    assert!(distinct.len() >= 3, "the steps cost different prices: {distinct:?}");
}

/// The public screens built when first read are the screens built at once, for every player of AI games, and an
/// observation read twice answers the same.
#[test]
fn the_lazy_public_screens_are_the_screens_built_at_once() {
    let rules = compile_rules(core_rules()).unwrap();
    let (mut compared, mut ranked, mut scored) = (0, 0, 0);
    let (mut game, mut driver) = ai_game(&rules, 5);
    for round in [20, 60, 100] {
        play_to(&mut game, &mut driver, &rules, round);
        let auth = game.view().auth;
        for p in 0..auth.players().count() {
            let (obs, _) = AIObservation::for_player(auth, game.view().derived, &rules, PlayerId(p as u8));
            assert!(!obs.public.is_built(), "an observation builds no screens until they are read");
            let at_once = public_screens(auth, game.view().derived, &rules, &obs.view, PlayerId(p as u8));
            assert_eq!(*obs.public, at_once);
            assert!(obs.public.is_built());
            let copy = obs.clone();
            assert!(copy.public.is_built(), "a copy keeps the screens built");
            assert_eq!(*copy.public, *obs.public);
            compared += 1;
            ranked += at_once.demographics.iter().filter(|d| d.value != 0 && d.best != d.worst).count();
            scored += at_once.scores.iter().filter(|(_, s)| *s != 0).count();
        }
    }
    assert!(compared >= 12 && ranked > 20 && scored > 5, "real screens were compared ({compared}, {ranked}, {scored})");
}

/// A review's runs of the economy cell with the base choices shared are the runs made apart: for the models of real
/// empires at three moods (the food weight a city ranks its tiles by is one of three), plans with and without
/// a delta on the tiles of a city, and the food price built of them.
#[test]
fn the_cell_runs_with_shared_base_choices_equal_the_runs_made_apart() {
    use classic_rules::ai::strategy_net::cell::{food_price, food_price_cached, run, run_cached, BaseChoices, CellPrices, Delta, Plan};
    use classic_rules::ai::strategy_net::model::Model;
    let rules = compile_rules(core_rules()).unwrap();
    let (mut runs, mut valued, mut grown, mut moods) = (0, 0, 0, std::collections::BTreeSet::new());
    let (mut game, mut driver) = ai_game(&rules, 9);
    for round in [40, 90, 140] {
        play_to(&mut game, &mut driver, &rules, round);
        let auth = game.view().auth;
        for p in 0..auth.players().count() {
            let (obs, _) = AIObservation::for_player(auth, game.view().derived, &rules, PlayerId(p as u8));
            let model = Model::of_observation(&obs, &rules);
            if model.cities.is_empty() {
                continue;
            }
            for excess in [model.excess, -2, -3000] {
                let model = Model { excess, ..model.clone() };
                let cache = BaseChoices::new(&model);
                let mut plans = vec![Plan::new(&model)];
                // A plan with a delta on tiles of each city, one on its centre, and one that only adds food.
                let mut tiled = Plan::new(&model);
                for (i, city) in model.cities.iter().enumerate() {
                    let tiles: Vec<(usize, [i64; 6])> = (0..city.tiles.len().min(3)).map(|t| (t, [200, 100, 0, 100, 0, 0])).chain([(usize::MAX, [0, 100, 50, 0, 0, 0])]).collect();
                    tiled.deltas[i].push(Delta { from: 5, min_pop: 2, tiles, percent: [0, 10, 10, 0, 0, 0], happy: 300, ..Delta::default() });
                }
                plans.push(tiled);
                let mut fed = Plan::new(&model);
                fed.deltas[0].push(Delta { flat: [100, 0, 0, 0, 0, 0], ..Delta::default() });
                plans.push(fed);
                for plan in &plans {
                    for (unit, citizen, sharp, horizon) in [([0, 100, 100, 100, 50, 50], 3000, false, 30), ([0, 80, 120, 90, 40, 20], 5000, true, 12)] {
                        let prices = CellPrices { unit, citizen, rounds_left: 150, sharp };
                        let (cached, plain) = (run_cached(&model, plan, &prices, horizon, &cache), run(&model, plan, &prices, horizon));
                        assert_eq!(cached, plain, "round {round} player {p} excess {excess} sharp {sharp}");
                        assert_eq!(food_price_cached(&model, plan, &prices, horizon, &cache), food_price(&model, plan, &prices, horizon));
                        runs += 1;
                        valued += usize::from(plain.value != 0);
                        grown += usize::from(plain.citizens != 0);
                        moods.insert(excess);
                    }
                }
            }
        }
    }
    assert!(runs > 100 && valued > 100 && grown > 10, "real runs were compared ({runs}, {valued}, {grown})");
    assert!(moods.len() >= 3, "the moods differ: {moods:?}");
}
