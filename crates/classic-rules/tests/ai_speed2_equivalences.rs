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
use classic_rules::ai::planner_workers::{offer_key, offer_memo_and_fresh};
use classic_rules::ai::state::{Strategy, WorkerJob};
use classic_rules::ai::{AiDriver, AiSettings};
use classic_rules::ids::{PlayerId, TileId};
use classic_rules::knowledge::observation::{public_screens, AIObservation};
use classic_rules::scenario::{generate_new_game, ScenarioConfig};
use classic_rules::{compile_rules, ClassicGame, CompiledRuleset, GameState};

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
    let mut keyed = [0usize; 3];
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
        let dims = game.view().auth.map().dimensions;
        let twin: Vec<_> = (0..rules.improvements.len()).filter(|i| rules.improvements[*i].placement.no_two_adjacent).map(|i| classic_rules::ids::ImprovementId(i as u16)).collect();
        let fresh_features: Vec<_> = (0..rules.features.len()).filter(|i| rules.features[*i].fresh_water).map(|i| classic_rules::ids::FeatureId(i as u16)).collect();
        let luxuries: Vec<_> = (0..rules.resources.len()).filter(|i| rules.resources[*i].class == Some(classic_rules::rules::content::ResourceClass::Luxury)).map(|i| classic_rules::ids::ResourceId(i as u16)).collect();
        assert!(!twin.is_empty() && !fresh_features.is_empty() && !luxuries.is_empty(), "the rules have what the changes use");
        let only = |tile: TileId| move |_: &AIObservation<'_>| vec![tile];
        for tile in own {
            let key = |game: &GameState| {
                let (obs, _) = AIObservation::for_player(game.view().auth, game.view().derived, &rules, who);
                offer_key(&obs.view, tile)
            };
            for near in dims.neighbors(tile) {
                let was = key(&game);
                for kind in twin.iter() {
                    game.debug_set_improvement_batch(&rules, &[(near, *kind)]);
                    game.debug_reveal(who, &[near]);
                    assert_eq!(compare_offers(&game, &rules, &only(tile), &mut seen, "a twin improvement beside"), 0);
                }
                let after_improvement = key(&game);
                game.debug_set_feature_batch(&rules, &[(near, fresh_features[0])]);
                game.debug_reveal(who, &[near]);
                assert_eq!(compare_offers(&game, &rules, &only(tile), &mut seen, "a feature with fresh water beside"), 0);
                let after_feature = key(&game);
                game.debug_set_resource_batch(&rules, &[(near, luxuries[0])]);
                game.debug_reveal(who, &[near]);
                assert_eq!(compare_offers(&game, &rules, &only(tile), &mut seen, "a luxury beside"), 0);
                let after_resource = key(&game);
                // What the memo keys an offer by sees each change beside the tile (when the change is one: the
                // neighbour may already hold it).
                keyed[0] += usize::from(after_improvement != was);
                keyed[1] += usize::from(after_feature != after_improvement);
                keyed[2] += usize::from(after_resource != after_feature);
            }
        }
    }
    eprintln!("{seen:?}, keys changed by a change beside the tile: {keyed:?}");
    assert!(seen.compared > 1000, "{seen:?}");
    assert!(seen.improvements > 200 && seen.gains_over_zero > 200, "real offers were compared: {seen:?}");
    assert!(seen.chops > 0, "offers to chop a feature were compared: {seen:?}");
    assert!(keyed.iter().all(|k| *k > 50), "a change beside a tile changes the key of its offer: {keyed:?}");
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
                // A plan that changes the empire's mood with no delta on a tile: the cities' choice is asked
                // at another food weight halfway (the base choices are asked at two weights in one run).
                let mut moody = Plan::new(&model);
                moody.deltas[0].push(Delta { from: 5, happy: 300, ..Delta::default() });
                plans.push(moody);
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

/// Open grassland, a city for each of two sides, at war, with soldiers of both standing near each other: the
/// units the AI decides on have enemies in sight.
fn war_game(rules: &CompiledRuleset, seed: u64) -> GameState {
    use classic_rules::ids::{TerrainId, UnitKindId};
    use classic_rules::math::HexCoord;
    use classic_rules::world::state::GameConfig;
    let (us, them) = (PlayerId(0), PlayerId(1));
    let mut game = GameState::new(rules, GameConfig { players: 2, width: 30, height: 20, rng_seed: seed });
    let dims = game.view().auth.map().dimensions;
    let land: Vec<(TileId, TerrainId)> = (0..dims.tile_count() as u32).map(|t| (TileId(t), TerrainId(1))).collect();
    game.debug_set_terrain_batch(rules, &land);
    let tiles: Vec<TileId> = land.iter().map(|(t, _)| *t).collect();
    game.debug_clear_features(rules, &tiles);
    game.debug_set_river_edges(rules, &[]);
    let at = |game: &GameState, q: i32, r: i32| game.view().auth.map().dimensions.to_tile(HexCoord { q, r }).unwrap();
    let (a, b) = (at(&game, 6, 10), at(&game, 22, 10));
    game.debug_found_city(rules, us, a);
    game.debug_found_city(rules, them, b);
    game.debug_meet(us, them);
    game.debug_force_war(rules, us, them);
    let kind = |key: &str| UnitKindId(rules.units.iter().position(|u| u.key == format!("core:unit/{key}")).unwrap() as u16);
    for (i, key) in ["swordsman", "archer", "swordsman", "spearman", "archer"].into_iter().enumerate() {
        let (qa, qb, r) = (11 + (i as i32 % 3), 15 - (i as i32 % 3), 8 + i as i32);
        game.debug_spawn_unit(rules, us, at(&game, qa, r), kind(key));
        game.debug_spawn_unit(rules, them, at(&game, qb, r), kind(key));
    }
    game
}

/// An exposure's answers from its memory, asked for the same place at other health, after a strike or not and
/// with other enemies hurt, are the answers computed afresh (the memo's key holds all of them).
#[test]
fn an_exposure_keys_its_answers_by_the_place_the_health_the_strike_and_the_enemies_hurt() {
    use classic_rules::ai::combat::Exposure;
    use classic_rules::ai::danger::{Source, Threats};
    let rules = compile_rules(core_rules()).unwrap();
    let (mut asked, mut hurt_units, mut distinct) = (0, 0, std::collections::BTreeSet::new());
    for seed in [3, 11] {
        let mut game = war_game(&rules, seed);
        let mut driver = AiDriver::new(AiSettings::for_rules(&rules, rules.default_difficulty()), Vec::new());
        play_to(&mut game, &mut driver, &rules, 3);
        for who in [PlayerId(0), PlayerId(1)] {
            let obs = game.ai_observation(&rules, who);
            let brief = TurnBrief::from_observation(&obs, &rules);
            let threats = Threats::build(&obs, &rules, &brief, &[]);
            let foes: Vec<TileId> = threats.foes.iter().map(|f| f.tile).collect();
            for source in [Source::Net, Source::Search] {
                for unit in threats.own.clone() {
                    let exposure = Exposure::of(&threats, &rules, source, &unit);
                    let places: Vec<TileId> = std::iter::once(unit.tile).chain(obs.view.dimensions().neighbors(unit.tile)).collect();
                    for tile in places {
                        for hp in [unit.hp, unit.hp / 2, 1] {
                            for struck in [false, true] {
                                let hurts: Vec<Vec<(TileId, u16)>> = vec![vec![], foes.iter().take(1).map(|t| (*t, 0)).collect(), foes.iter().take(1).map(|t| (*t, 30)).collect()];
                                // Twice each, in two orders: the second ask of a key is the memory's.
                                for hurt in hurts.iter().chain(hurts.iter().rev()) {
                                    let answer = exposure.danger(tile, hp, struck, hurt);
                                    let fresh = exposure.danger_fresh(tile, hp, struck, hurt);
                                    assert_eq!(answer, fresh, "{tile:?} hp {hp} struck {struck} hurt {hurt:?}");
                                    asked += 1;
                                    hurt_units += usize::from(!hurt.is_empty() && answer.damage > 0);
                                    distinct.insert((answer.damage, answer.death_permille));
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    assert!(asked > 500 && hurt_units > 20 && distinct.len() > 5, "real dangers were compared ({asked}, {hurt_units}, {})", distinct.len());
}

/// The dangers an exposure remembers are the dangers computed afresh: with the memo check on (`memo_check`) every
/// answer taken from the memo is compared with a fresh one while AI armies fight. A guard that compared nothing
/// or only zeros proves nothing, so the counts are asserted.
#[test]
fn the_exposure_answers_from_its_memory_as_it_computes_them() {
    use classic_rules::ai::memo_check;
    let rules = compile_rules(core_rules()).unwrap();
    memo_check::set(true);
    for seed in [3, 11, 21] {
        let mut game = war_game(&rules, seed);
        let mut driver = AiDriver::new(AiSettings::for_rules(&rules, rules.default_difficulty()), Vec::new());
        play_to(&mut game, &mut driver, &rules, 40);
    }
    memo_check::set(false);
    let (compared, not_zero) = memo_check::counts();
    assert!(compared > 100 && not_zero > 30, "real dangers were compared ({compared}, {not_zero})");
}

/// The facts read through the one-tile cache are the facts of the view itself, fact by fact, over every tile
/// of the maps of AI games, asked in the order a rule asks them (all of one tile, then the next) and in a
/// crossing one (two tiles by turns), for every player.
#[test]
fn the_one_tile_facts_answer_as_the_view_does() {
    use classic_rules::world::facts::{OneTileFacts, TileFacts};
    let rules = compile_rules(core_rules()).unwrap();
    let (mut asked, mut visible_resources, mut improved, mut owned, mut coastal, mut rivers, mut water) = (0, 0, 0, 0, 0, 0, 0);
    let (mut game, mut driver) = ai_game(&rules, 7);
    for round in [60, 150] {
        play_to(&mut game, &mut driver, &rules, round);
        let auth = game.view().auth;
        for p in 0..auth.players().count() {
            let (obs, _) = AIObservation::for_player(auth, game.view().derived, &rules, PlayerId(p as u8));
            let view = obs.view;
            let facts = OneTileFacts::new(&view);
            let count = view.tile_count() as u32;
            let same = |tile: TileId| {
                assert_eq!(facts.terrain(tile), view.terrain(tile));
                assert_eq!(facts.feature(tile), view.feature(tile));
                assert_eq!(facts.natural_wonder(tile), view.natural_wonder(tile));
                assert_eq!(facts.resource(tile), view.resource(tile));
                assert_eq!(facts.improvement(tile), view.improvement(tile));
                assert_eq!(facts.pillaged(tile), view.pillaged(tile));
                assert_eq!(facts.owner(tile), view.owner(tile));
                assert_eq!(facts.city(tile), view.city(tile));
                assert_eq!(facts.river(tile), view.river(tile));
                assert_eq!(facts.lake(tile), view.lake(tile));
                assert_eq!(facts.coastal_land(tile), view.coastal_land(tile));
                assert_eq!(facts.is_water(tile), view.is_water(tile));
            };
            for t in (0..count).map(TileId) {
                same(t);
                asked += 1;
                visible_resources += usize::from(view.resource(t).is_some());
                improved += usize::from(view.improvement(t).is_some());
                owned += usize::from(view.owner(t).is_some());
                coastal += usize::from(view.coastal_land(t));
                rivers += usize::from(view.river(t));
                water += usize::from(view.is_water(t));
            }
            for t in (0..count.saturating_sub(1)).map(TileId) {
                // Crossing: the cached tile changes at every ask.
                assert_eq!(facts.improvement(t), view.improvement(t));
                assert_eq!(facts.owner(TileId(t.0 + 1)), view.owner(TileId(t.0 + 1)));
                assert_eq!(facts.improvement(t), view.improvement(t));
                assert_eq!(facts.terrain(TileId(t.0 + 1)), view.terrain(TileId(t.0 + 1)));
            }
            // Outside the map: nothing is known.
            assert_eq!(facts.terrain(TileId(count + 5)), view.terrain(TileId(count + 5)));
        }
    }
    assert!(asked > 1000 && visible_resources > 20 && improved > 5 && owned > 50 && coastal > 50 && rivers > 5 && water > 50, "real facts were compared ({asked} {visible_resources} {improved} {owned} {coastal} {rivers} {water})");
}

/// The owner-level unit modifiers taken from the effect memo are those computed from the sources, for every
/// player of AI games in the memo's verify mode (a hit is recomputed and asserted equal there), and they are
/// not the empty set: the nations have policies, technologies and buildings that change units.
#[test]
fn the_owner_part_of_unit_modifiers_from_the_memo_is_the_one_from_the_sources() {
    use classic_rules::effects::memo::MemoMode;
    use classic_rules::effects::unit::{owner_unit_modifiers, owner_unit_modifiers_fresh, UnitModifiers};
    let rules = compile_rules(core_rules()).unwrap();
    let (mut game, mut driver) = ai_game(&rules, 13);
    game.set_memo_mode(MemoMode::Verify);
    let (mut compared, mut not_empty, mut combat, mut deferred) = (0, 0, 0, 0);
    for round in [25, 70, 120] {
        play_to(&mut game, &mut driver, &rules, round);
        let auth = game.view().auth;
        let hits_before = auth.memo().stats().hits;
        for p in 0..auth.players().count() {
            let owner = PlayerId(p as u8);
            let (first, second) = (owner_unit_modifiers(auth, &rules, owner), owner_unit_modifiers(auth, &rules, owner));
            let fresh = owner_unit_modifiers_fresh(auth, &rules, owner);
            assert_eq!(first, fresh, "round {round} player {p}");
            assert_eq!(second, fresh, "round {round} player {p}");
            compared += 1;
            not_empty += usize::from(fresh != UnitModifiers::default());
            combat += usize::from(fresh.combat.iter().any(|c| *c != 0));
            deferred += fresh.deferred.len();
        }
        assert!(auth.memo().stats().hits > hits_before, "the second ask is answered from the memo");
    }
    assert!(compared >= 12 && not_empty >= 6 && combat + deferred > 0, "real modifiers were compared ({compared} {not_empty} {combat} {deferred})");
}

// ---- The key of the worker offer memo: what the snapshots of the tile and its neighbours do not show. ----
//
// `Around` holds the snapshots of seven tiles, the pillage of the tile, the coast of the tile and the lake
// bits of its neighbours; `Ctx` holds the ruleset, the map's size, the player, the nation, the techs and the
// repair switch of the brief. A water body's size makes a tile a lake (at most 10) or a coast (at least 10), so
// a tile far from the seven can flip them without a snapshot changing; a pillage shows in no snapshot at all.
// Each case below changes one such thing and nothing else the snapshots show, and fails when that part is not
// in the key.

mod offer_key {
    use super::*;
    use classic_rules::ai::planner_workers::WorkersFit;
    use classic_rules::ids::{ImprovementId, TechId, TerrainId, UnitKindId};
    use classic_rules::math::{hex_disc, HexCoord};
    use classic_rules::{ClassicGame, Command, GameConfig};

    const P0: PlayerId = PlayerId(0);
    const P1: PlayerId = PlayerId(1);

    fn terrain(rules: &CompiledRuleset, key: &str) -> TerrainId {
        // The map's terrain code, not the index in the table.
        TerrainId(rules.terrains.iter().find(|t| t.key == key).and_then(|t| t.map_code).unwrap())
    }

    fn grass(rules: &CompiledRuleset) -> TerrainId {
        terrain(rules, "core:terrain/grassland")
    }

    /// The one map code water has.
    fn water(game: &GameState) -> TerrainId {
        let map = game.view().auth.map();
        TerrainId((0..64).find(|c| map.terrain_kinds().is_water(*c)).unwrap())
    }

    type Offer = Option<(WorkerJob, i32, i32)>;

    /// A 30x20 grassland of two players, every technology known, P0's city at (14, 10).
    fn arena(rules: &CompiledRuleset) -> (GameState, TileId) {
        let mut game = GameState::new(rules, GameConfig { players: 2, width: 30, height: 20, rng_seed: 4 });
        let all: Vec<_> = (0..600).map(|i| (TileId(i), grass(rules))).collect();
        game.debug_set_terrain_batch(rules, &all);
        for tech in 0..rules.technologies.len() {
            for p in [P0, P1] {
                game.debug_learn(rules, p, TechId(tech as u16));
            }
        }
        // Polynesia: its moai stands on the coast only.
        let nation = rules.nations.iter().position(|n| n.key == "core:nation/polynesia").unwrap();
        game.debug_set_player_nation(P0, nation as u8);
        let city = game.view().auth.map().dimensions.to_tile(HexCoord { q: 14, r: 10 }).unwrap();
        game.debug_found_city(rules, P0, city);
        (game, city)
    }

    /// What the memo answers, what `best_job` computes, and the key of the offer, for P0 and `tile`.
    fn ask(game: &GameState, rules: &CompiledRuleset, workers: WorkersFit, tile: TileId) -> (Offer, Offer, classic_rules::ai::planner_workers::OfferKey) {
        let (obs, _) = AIObservation::for_player(game.view().auth, game.view().derived, rules, P0);
        let mut brief = TurnBrief::from_observation(&obs, rules);
        brief.workers = workers;
        let (memo, fresh) = offer_memo_and_fresh(&obs.view, rules, &brief, Strategy::Expand, tile);
        (memo, fresh, offer_key(&obs.view, tile))
    }

    fn same_as_fresh(what: &str, answer: &(Offer, Offer, classic_rules::ai::planner_workers::OfferKey)) {
        assert_eq!(answer.0, answer.1, "{what}: the memo answers as the job finder does");
    }

    const REPAIR: WorkersFit = WorkersFit::ALL;
    const NO_REPAIR: WorkersFit = WorkersFit::NONE;

    /// A pillaged own farm (the snapshot of it unchanged: the farm still stands) offers its repair with the switch
    /// on and not without it; the memo, warm from before the pillage and from the other switch, says so.
    #[test]
    fn the_offer_memo_sees_a_pillage_and_the_repair_switch_that_no_snapshot_shows() {
        let rules = compile_rules(core_rules()).unwrap();
        let (mut game, city) = arena(&rules);
        let farm = ImprovementId(rules.improvements.iter().position(|i| i.key == "core:improvement/farm").unwrap() as u16);
        let warrior = UnitKindId(rules.units.iter().position(|u| u.key == "core:unit/warrior").unwrap() as u16);
        let dims = game.view().auth.map().dimensions;
        // P1 has no city: it takes its turns only while it has a unit.
        let pillagers: Vec<_> = dims.neighbors(city).into_iter().map(|tile| (tile, game.debug_spawn_unit(&rules, P1, tile, warrior))).collect();
        game.debug_force_war(&rules, P1, P0);
        for _ in 0..4 {
            let active = game.view().auth.active_player();
            if active == P1 {
                break;
            }
            game.execute(&rules, Command::EndTurn { player: active }).unwrap();
        }
        assert_eq!(game.view().auth.active_player(), P1);
        let (mut pillaged, mut key_changed, mut repair_offered, mut switch_decides) = (0, 0, 0, 0);
        for (tile, unit) in pillagers {
            game.debug_set_improvement_batch(&rules, &[(tile, farm)]);
            // The memo is left warm with the switch on (a change of the switch starts it over).
            let before_off = ask(&game, &rules, NO_REPAIR, tile);
            same_as_fresh("a farm standing, repair off", &before_off);
            let before = ask(&game, &rules, REPAIR, tile);
            same_as_fresh("a farm standing, repair on", &before);
            game.execute(&rules, Command::Pillage { unit }).unwrap();
            if !game.view().auth.map().pillaged(tile) {
                continue;
            }
            pillaged += 1;
            // The memo is warm from before the pillage with the switch on; then the switch is turned off, and on again.
            let on = ask(&game, &rules, REPAIR, tile);
            same_as_fresh("pillaged, repair on", &on);
            let off = ask(&game, &rules, NO_REPAIR, tile);
            same_as_fresh("pillaged, repair off", &off);
            let on_again = ask(&game, &rules, REPAIR, tile);
            same_as_fresh("pillaged, repair on again", &on_again);
            key_changed += usize::from(on.2 != before.2);
            repair_offered += usize::from(on.1.is_some() && on.1 != before.1);
            switch_decides += usize::from(on.1 != off.1);
        }
        eprintln!("pillaged {pillaged}, key changed {key_changed}, repair offered {repair_offered}, switch decides {switch_decides}");
        assert!(pillaged >= 4, "tiles were pillaged: {pillaged}");
        assert_eq!(key_changed, pillaged, "a pillage changes the key of the tile's offer");
        assert!(repair_offered >= 4, "a repair job was offered where the farm was pillaged: {repair_offered}");
        assert!(switch_decides >= 4, "the repair switch decides the offer: {switch_decides}");
    }

    /// A pond of nine tiles beside the tile is a lake and no sea; of ten, both; of eleven, a sea and no lake.
    /// Each tile added is two or more tiles from the tile asked, so no snapshot of its seven changes. The key of the
    /// offer must differ at the first step by the coast alone and at the second by the lake alone.
    #[test]
    fn the_offer_memo_sees_a_lake_and_a_coast_made_by_a_water_body_far_from_the_tile() {
        let rules = compile_rules(core_rules()).unwrap();
        let (mut game, city) = arena(&rules);
        let dims = game.view().auth.map().dimensions;
        let tile = dims.neighbors(city).into_iter().next().unwrap();
        let pond = dims.neighbors(tile).into_iter().find(|n| *n != city && !dims.adjacent(*n, city)).unwrap_or_else(|| dims.neighbors(tile).into_iter().find(|n| *n != city).unwrap());
        let near: std::collections::HashSet<TileId> = hex_disc(dims, tile, 1).collect();
        // The body grows from a tile next to the pond, outward, in an order that keeps it one body.
        let first = dims.neighbors(pond).into_iter().find(|n| !near.contains(n)).unwrap();
        let mut order = vec![first];
        let mut i = 0;
        while order.len() < 12 {
            for n in dims.neighbors(order[i]) {
                if !near.contains(&n) && n != pond && !order.contains(&n) && hex_disc(dims, tile, 9).any(|t| t == n) {
                    order.push(n);
                }
            }
            i += 1;
        }
        assert_eq!(hex_disc(dims, tile, 9).count(), 271, "the disc is whole");
        let lands: Vec<TerrainId> = rules.terrains.iter().filter(|t| !t.water).filter_map(|t| t.map_code).map(TerrainId).collect();
        let (mut coast_decides, mut lake_decides) = (0, 0);
        for terrain in lands {
            let disc: Vec<_> = hex_disc(dims, tile, 9).map(|t| (t, grass(&rules))).collect();
            game.debug_set_terrain_batch(&rules, &disc);
            game.debug_set_terrain_batch(&rules, &[(tile, terrain)]);
            let sea = water(&game);
            let water = |tiles: &[TileId]| tiles.iter().map(|t| (*t, sea)).collect::<Vec<_>>();
            let reveal = |game: &mut GameState| game.debug_reveal(P0, &hex_disc(dims, tile, 9).collect::<Vec<_>>());
            let mut body: Vec<TileId> = vec![pond];
            body.extend(order.iter().take(8));
            game.debug_set_terrain_batch(&rules, &water(&body));
            reveal(&mut game);
            let map = game.view().auth.map();
            assert!(map.lake(pond) && !map.coastal_land(tile), "nine water tiles: a lake, no coast");
            let nine = ask(&game, &rules, REPAIR, tile);
            same_as_fresh("a pond of nine", &nine);

            game.debug_set_terrain_batch(&rules, &water(&order[8..9]));
            reveal(&mut game);
            let map = game.view().auth.map();
            assert!(map.lake(pond) && map.coastal_land(tile), "ten water tiles: a lake and a coast");
            let ten = ask(&game, &rules, REPAIR, tile);
            same_as_fresh("a pond of ten", &ten);
            assert_ne!(nine.2, ten.2, "{terrain:?}: the coast of the tile is in the key");

            game.debug_set_terrain_batch(&rules, &water(&order[9..10]));
            reveal(&mut game);
            let map = game.view().auth.map();
            assert!(!map.lake(pond) && map.coastal_land(tile), "eleven water tiles: no lake, a coast");
            let eleven = ask(&game, &rules, REPAIR, tile);
            same_as_fresh("a pond of eleven", &eleven);
            assert_ne!(ten.2, eleven.2, "{terrain:?}: the lake bit of the neighbour is in the key");
            coast_decides += usize::from(nine.1 != ten.1);
            lake_decides += usize::from(ten.1 != eleven.1);
        }
        eprintln!("the coast decided the offer on {coast_decides} terrains, the lake on {lake_decides}");
        // The lake changes a job's offer (fresh water makes a terrain valid); the coast only lets the moai
        // be built, which the AI never values above its other jobs, so for the coast the key is the proof.
        assert!(lake_decides > 0, "the offers differ where only the lake changed: {lake_decides}");
    }
}
