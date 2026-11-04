//! World Congress projects on the classic ruleset (classic/docs/classic/rules.md):
//! the resolution starts a project, cities contribute through its process,
//! the members are rewarded by tier at completion (World's Fair,
//! International Games, International Space Station), save/load, the AI;
//! and the Manhattan Project's nuclear weapons.

use classic_rules::ClassicGame;
use classic_content::core::core_rules;
use classic_rules::ids::{PlayerId, ProductionItem, TechId, TileId, UnitKindId};
use classic_rules::math::HexCoord;
use classic_rules::replay::authoritative_hash;
use classic_rules::rules::content::{CityStateId, ProjectId};
use classic_rules::world::city_economy::Process;
use classic_rules::world::congress::{tiers, ProjectProgress, ResolutionKind, YES};
use classic_rules::world::player::PlayerKind;
use classic_rules::world::state::GameConfig;
use classic_rules::{compile_rules, Command, CompiledRuleset, GameState};
use classic_rules::world::player::ClassicKind;

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);
const CS: PlayerId = PlayerId(2);

fn tile(game: &GameState, q: i32, r: i32) -> TileId {
    game.view().auth.map().dimensions.to_tile(HexCoord { q, r }).unwrap()
}

fn tech(rules: &CompiledRuleset, key: &str) -> TechId {
    TechId(rules.technologies.iter().position(|t| t.key == key).unwrap() as u16)
}

fn project(rules: &CompiledRuleset, key: &str) -> ProjectId {
    ProjectId(rules.module.projects.iter().position(|p| p.key == key).unwrap() as u16)
}

const FAIR: &str = "core:project/world_s_fair";
const GAMES: &str = "core:project/international_games";
const STATION: &str = "core:project/international_space_station";

/// Two nations and a city-state, met; the congress founded with P0
/// as host.
fn world() -> (GameState, CompiledRuleset) {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = GameState::new(&rules, GameConfig { players: 3, width: 30, height: 20, rng_seed: 4 });
    game.debug_set_player_kind(CS, PlayerKind::Module(ClassicKind::CityState(CityStateId(0))));
    game.debug_found_city(&rules, P0, tile(&game, 3, 3));
    game.debug_found_city(&rules, P1, tile(&game, 24, 15));
    game.debug_found_city(&rules, CS, tile(&game, 13, 9));
    game.debug_meet_all();
    game.debug_learn(&rules, P0, tech(&rules, "core:tech/printing-press"));
    end_round(&mut game, &rules);
    assert!(game.view().auth.module().congress().founded());
    (game, rules)
}

fn end_round(game: &mut GameState, rules: &CompiledRuleset) {
    let round = game.view().auth.turn().round;
    while game.view().auth.turn().round == round {
        let active = game.view().auth.active_player();
        game.execute(rules, Command::EndTurn { player: active }).unwrap();
    }
}

fn until_active(game: &mut GameState, rules: &CompiledRuleset, player: PlayerId) {
    for _ in 0..4 {
        if game.view().auth.active_player() == player {
            return;
        }
        let active = game.view().auth.active_player();
        game.execute(rules, Command::EndTurn { player: active }).unwrap();
    }
    panic!("{player:?} never became active");
}

fn first_city(game: &GameState, player: PlayerId) -> classic_rules::ids::CityHandle {
    game.view().auth.indexes().cities_of(player)[0]
}

#[test]
fn the_resolution_starts_the_worlds_fair_and_a_city_contributes() {
    let (mut game, rules) = world();
    let fair = project(&rules, FAIR);
    until_active(&mut game, &rules, P0);
    let city = first_city(&game, P0);
    let early = Command::SetProcess { player: P0, city, process: Some(Process::Contribute(fair)) };
    assert!(game.execute(&rules, early).is_err(), "no project under way yet");
    let propose = Command::ProposeResolution { player: P0, kind: ResolutionKind::InternationalProject, choice: u32::from(fair.0), repeal: false };
    game.execute(&rules, propose).unwrap();
    for _ in 0..40 {
        if game.view().auth.module().congress().in_session() {
            break;
        }
        end_round(&mut game, &rules);
    }
    until_active(&mut game, &rules, P0);
    let vote = Command::CastVotes { player: P0, proposal: 0, choice: YES, delegates: 2 };
    game.execute(&rules, vote).unwrap();
    end_round(&mut game, &rules);
    end_round(&mut game, &rules);
    let congress = game.view().auth.module().congress();
    assert!(congress.project(fair).is_some(), "the project is under way");
    assert!(congress.active().is_empty(), "starting a project is not a resolution in force");
    // An empty queue and the project's process: the production goes to it.
    game.debug_set_population(&rules, city, 6);
    until_active(&mut game, &rules, P0);
    let set = Command::SetProcess { player: P0, city, process: Some(Process::Contribute(fair)) };
    game.execute(&rules, set).unwrap();
    let ours = |g: &GameState| g.view().auth.module().congress().project(fair).unwrap().of(P0);
    let before = ours(&game);
    end_round(&mut game, &rules);
    assert!(ours(&game) > before, "the city contributed");
    assert_eq!(ours(&game) % 100, 0, "whole production in hundredths");
    game.validate(&rules).unwrap();
}

#[test]
fn tiers_follow_the_share_and_the_best_contribution() {
    let (game, rules) = world();
    let league = rules.module.projects[project(&rules, FAIR).0 as usize].league.clone().unwrap();
    let share = u64::from(league.cost_per_player) * 100;
    let tier_of = |ours: u64, theirs: u64| {
        let progress = ProjectProgress { project: project(&rules, FAIR), contributions: vec![ours, theirs, 0] };
        tiers(game.view().auth, &league, &progress)
    };
    assert_eq!(tier_of(share, share / 2), vec![3, 1, 0]);
    assert_eq!(tier_of(share / 2 - 100, share / 2), vec![1, 1, 0], "the best but below a full share: no tier 3");
    assert_eq!(tier_of(share * 39 / 100, 0), vec![0, 0, 0], "below 40 % of the share: nothing");
    assert_eq!(tier_of(share * 91 / 100, share * 40 / 100), vec![2, 1, 0], "90 % of the share is tier 2");
    assert_eq!(tier_of(2 * share, 2 * share), vec![3, 2, 0], "a tie: the first member gets tier 3");
}

/// The project starts and the two nations contribute `ours` and
/// `theirs` (whole production); the league's turn completes it.
fn complete(game: &mut GameState, rules: &CompiledRuleset, key: &str, ours: u32, theirs: u32) -> ProjectId {
    let id = project(rules, key);
    game.debug_start_league_project(rules, id);
    assert!(game.view().auth.module().congress().project(id).is_some());
    game.debug_contribute_to_league_project(rules, P0, id, ours);
    game.debug_contribute_to_league_project(rules, P1, id, theirs);
    end_round(game, rules);
    assert!(game.view().auth.module().congress().completed().contains(&id), "completed");
    id
}

#[test]
fn the_worlds_fair_rewards_by_tier() {
    let (mut game, rules) = world();
    let free = |g: &GameState, p| g.view().auth.module().culture().of(p).free_policies;
    let meter = |g: &GameState, p| g.view().auth.players().economy_of(p).unwrap().golden_age_meter;
    let (free0, free1) = (free(&game, P0), free(&game, P1));
    let meter1 = meter(&game, P1);
    complete(&mut game, &rules, FAIR, 450, 150);
    let rewards = game.view().auth.module().congress().rewards_of(P0);
    assert_eq!(free(&game, P0), free0 + 1, "tier 2: a free policy");
    assert!(rewards.culture_boost_turns >= 14, "tier 3: culture +75 % for 15 turns");
    assert_eq!(free(&game, P1), free1, "tier 1 only");
    // 500 points fill the first meter (threshold 500 for one city): a
    // golden age began in P1's next GoldenAge step, or the meter holds them.
    let economy = game.view().auth.players().economy_of(P1).unwrap();
    let golden = economy.golden_age_turns > 0 || meter(&game, P1) >= meter1 + 340;
    assert!(golden, "tier 1: 360 golden age points");
    assert_eq!(game.view().auth.module().congress().rewards_of(P1).culture_boost_turns, 0);
    game.validate(&rules).unwrap();
}

#[test]
fn the_international_games_bring_happiness_influence_and_tourism() {
    let (mut game, rules) = world();
    let games = project(&rules, GAMES);
    assert!(classic_rules::world::congress::project_problem(game.view().auth, &rules, games).is_some(), "needs Radio");
    game.debug_learn(&rules, P1, tech(&rules, "core:tech/radio"));
    let influence = |g: &GameState| g.view().auth.module().city_states().standing(CS, P0).influence;
    let before = influence(&game);
    complete(&mut game, &rules, GAMES, 900, 300);
    let happiness = classic_rules::derived::happiness::player_happiness(game.view().auth, &rules, P0);
    assert_eq!(happiness.league, 4, "tiers 1 and 2: +2 happiness each");
    assert!(influence(&game) >= before + 25 * 100 - 200, "tier 2: +25 influence (minus a round of decay)");
    assert!(game.view().auth.module().congress().rewards_of(P0).tourism_boost_turns >= 14);
    let theirs = classic_rules::derived::happiness::player_happiness(game.view().auth, &rules, P1);
    assert_eq!(theirs.league, 2, "tier 1");
    game.validate(&rules).unwrap();
}

#[test]
fn the_space_station_gives_a_great_scientist_and_its_wonder() {
    let (mut game, rules) = world();
    game.debug_learn(&rules, P0, tech(&rules, "core:tech/satellites"));
    let scientist = UnitKindId(rules.units.iter().position(|u| u.key == "core:unit/great-scientist").unwrap() as u16);
    let station = rules.buildings.iter().position(|b| b.key == "core:building/international_space_station").unwrap();
    let scientists = |g: &GameState, p| {
        let auth = g.view().auth;
        auth.indexes().units_of(p).iter().filter(|u| auth.units().get(**u).unwrap().core.kind == scientist).count()
    };
    complete(&mut game, &rules, STATION, 1350, 450);
    let capital = first_city(&game, P0);
    let core = game.view().auth.cities().get(capital).unwrap();
    assert!(core.buildings.contains(&classic_rules::ids::BuildingKindId(station as u16)), "tier 3: the wonder");
    assert_eq!(scientists(&game, P0), 1, "tier 2: a Great Scientist");
    assert_eq!(scientists(&game, P1), 0, "tier 1: research only");
    game.validate(&rules).unwrap();
}

#[test]
fn projects_and_their_rewards_survive_save_and_load() {
    let (mut game, rules) = world();
    complete(&mut game, &rules, FAIR, 525, 175);
    game.debug_learn(&rules, P1, tech(&rules, "core:tech/radio"));
    let games = project(&rules, GAMES);
    game.debug_start_league_project(&rules, games);
    game.debug_contribute_to_league_project(&rules, P0, games, 40);
    let city = first_city(&game, P0);
    until_active(&mut game, &rules, P0);
    game.execute(&rules, Command::SetProcess { player: P0, city, process: Some(Process::Contribute(games)) }).unwrap();
    let save = classic_rules::persistence::encode::encode(&game, &rules);
    let loaded = classic_rules::persistence::decode::decode(&save, &rules).unwrap();
    assert_eq!(authoritative_hash(&loaded), authoritative_hash(&game));
    assert_eq!(loaded.view().auth.module().congress(), game.view().auth.module().congress());
}

#[test]
fn the_ai_proposes_a_project_and_works_on_it() {
    let (mut game, rules) = world();
    until_active(&mut game, &rules, P0);
    let obs = game.ai_observation(&rules, P0);
    let w = classic_rules::ai::planner_value::Weights::of(classic_rules::ai::state::Strategy::Expand);
    let orders = classic_rules::ai::congress::proposal_orders(&obs, &rules, &w);
    let fair = project(&rules, FAIR);
    let expected = format!("{:?}", vec![Command::ProposeResolution { player: P0, kind: ResolutionKind::InternationalProject, choice: u32::from(fair.0), repeal: false }]);
    assert_eq!(format!("{orders:?}"), expected, "the World's Fair is the project it may start");
    game.debug_start_league_project(&rules, fair);
    let obs = game.ai_observation(&rules, P0);
    let city = first_city(&game, P0);
    assert_eq!(classic_rules::ai::congress::contributing_city(&obs), Some(city));
    let set = format!("{:?}", vec![Command::SetProcess { player: P0, city, process: Some(Process::Contribute(fair)) }]);
    assert_eq!(format!("{:?}", classic_rules::ai::congress::project_orders(&obs)), set);
    // A full share contributed: the city goes back to its queue.
    game.debug_contribute_to_league_project(&rules, P0, fair, 350);
    let obs = game.ai_observation(&rules, P0);
    assert_eq!(classic_rules::ai::congress::contributing_city(&obs), None);
}

#[test]
fn nuclear_weapons_need_the_manhattan_project() {
    let (mut game, rules) = world();
    for key in ["core:tech/atomic-theory", "core:tech/nuclear-fission"] {
        game.debug_learn(&rules, P0, tech(&rules, key));
    }
    let bomb = UnitKindId(rules.units.iter().position(|u| u.key == "core:unit/atomic-bomb").unwrap() as u16);
    let manhattan = project(&rules, "core:project/manhattan_project");
    assert!(rules.module.projects[manhattan.0 as usize].allows_nukes);
    until_active(&mut game, &rules, P0);
    let city = first_city(&game, P0);
    let order = Command::EnqueueProduction { player: P0, city, item: ProductionItem::Unit(bomb) };
    let refused = format!("{:?}", game.execute(&rules, order.clone()).unwrap_err());
    assert!(refused.contains("Manhattan"), "{refused}");
    assert!(!classic_rules::world::victory::nukes_allowed(game.view().auth, &rules, P0));
    game.debug_add_project(P1, manhattan);
    assert!(!classic_rules::world::victory::nukes_allowed(game.view().auth, &rules, P0), "another nation's project");
    game.debug_add_project(P0, manhattan);
    assert!(classic_rules::world::victory::nukes_allowed(game.view().auth, &rules, P0));
    if let Err(e) = game.execute(&rules, order) {
        assert!(!format!("{e:?}").contains("Manhattan"), "{e:?}");
    }
}
