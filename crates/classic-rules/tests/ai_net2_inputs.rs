//! The inputs of `net2` (part A, `ai::net2::inputs`, `rival_inputs`; design §2): each input on a
//! constructed world, the memory's bytes, the fixed soldiers estimator, and the fog (what the seat does
//! not see does not move an input).
#![cfg(feature = "experimental-ai")]

use classic_content::core::core_rules;
use classic_rules::ai::net2::inputs::{self, InputMemory};
use classic_rules::ai::net2::levers::{self, Pass};
use classic_rules::ai::net2::plan::{CoreMemory, Inputs, Parts, RivalInputs};
use classic_rules::ai::net2::weights::{a, q};
use classic_rules::ai::planner;
use classic_rules::ai::rival_model::{estimate, soldiers, Book, Guess, SoldiersEvidence};
use classic_rules::ai::state::AiState;
use classic_rules::ai::strategy_net::features::neutral;
use classic_rules::ai::strategy_net::weights::kc;
use classic_rules::command::Command;
use classic_rules::derived::standing::{Demographic, Measure};
use classic_rules::ids::{PlayerId, TechId, TerrainId, TileId, UnitKindId};
use classic_rules::math::HexCoord;
use classic_rules::world::relations::DealItem;
use classic_rules::{compile_rules, ClassicGame, CompiledRuleset, GameConfig, GameState};

const US: PlayerId = PlayerId(0);
const A: PlayerId = PlayerId(1);
const B: PlayerId = PlayerId(2);

fn rules() -> CompiledRuleset {
    compile_rules(core_rules()).unwrap()
}

fn tile(game: &GameState, q: i32, r: i32) -> TileId {
    game.view().auth.map().dimensions.to_tile(HexCoord { q, r }).unwrap()
}

fn unit_kind(rules: &CompiledRuleset, key: &str) -> UnitKindId {
    let full = format!("core:unit/{key}");
    UnitKindId(rules.units.iter().position(|u| u.key == full).unwrap() as u16)
}

/// `players` nations on open grassland, a 40 x 24 map, a capital each at the given spots.
fn world(rules: &CompiledRuleset, players: u8, spots: &[(i32, i32)]) -> GameState {
    let mut game = GameState::new(rules, GameConfig { players, width: 40, height: 24, rng_seed: 7 });
    let dims = game.view().auth.map().dimensions;
    let land: Vec<(TileId, TerrainId)> = (0..dims.tile_count() as u32).map(|t| (TileId(t), TerrainId(1))).collect();
    game.debug_set_terrain_batch(rules, &land);
    let tiles: Vec<TileId> = land.iter().map(|(t, _)| *t).collect();
    game.debug_clear_features(rules, &tiles);
    game.debug_set_river_edges(rules, &[]);
    for (p, (q, r)) in spots.iter().enumerate() {
        game.debug_found_city(rules, PlayerId(p as u8), tile(&game, *q, *r));
    }
    game
}

fn spawn(game: &mut GameState, rules: &CompiledRuleset, owner: PlayerId, q: i32, r: i32, kind: &str) {
    let t = tile(game, q, r);
    game.debug_spawn_unit(rules, owner, t, unit_kind(rules, kind));
}

/// The seat's turn: the brief, the model, the inputs; `state` and `memory` carry over.
fn read(game: &GameState, rules: &CompiledRuleset, seat: PlayerId, state: &mut AiState, memory: &mut InputMemory) -> Inputs {
    let obs = game.ai_observation(rules, seat);
    let brief = planner::observe(&obs, state, rules);
    let model = estimate(&obs, rules, &Book::default());
    inputs::read(&obs, &brief, state, rules, &neutral(&[]), &model, memory)
}

fn rival(inputs: &Inputs, p: PlayerId) -> RivalInputs {
    *inputs.rivals.iter().find(|r| r.rival == p).unwrap_or_else(|| panic!("no row of {p:?}"))
}

/// End turns until the round is `rounds` later.
fn advance(game: &mut GameState, rules: &CompiledRuleset, rounds: u32) {
    let target = game.view().auth.turn().round + rounds;
    while game.view().auth.turn().round < target {
        let active = game.view().auth.active_player();
        game.execute(rules, Command::EndTurn { player: active }).unwrap();
    }
}

fn demo(value: i64, best: i64, average: i64, worst: i64) -> Demographic {
    Demographic { measure: Measure::Soldiers, value, rank: 1, best, average, worst }
}

// ---- the soldiers estimator (§2.4) ----

/// Six nations: we have 12 soldiers, five rivals 1, 10, 12, 14, 13 (sum 62, average 10). The score
/// of the disarmed one is average: the score-shared guess gives it a fair share, the new one the exact 1
/// (the four others were seen with more units than the worst's 1).
#[test]
fn a_disarmed_rival_of_an_average_score_reads_as_disarmed() {
    let d = demo(12, 14, 74 / 6, 1);
    let ev = |player: u8, lower: i64| SoldiersEvidence { player: PlayerId(player), lower, score: 100 };
    let evidence = [ev(1, 0), ev(2, 3), ev(3, 3), ev(4, 3), ev(5, 3)];
    let g = soldiers(&d, 6, &evidence);
    assert_eq!(g[0], Guess::exact(1), "the worst is exact when the others are excluded by sight: {g:?}");
    assert!(g[0].value * 1000 / (g[0].value + 12) <= 100, "its share against our 12 is at most 100");
    // The old estimate (score-shared, every score equal): the average, not the worst.
    let met: Vec<(PlayerId, i64)> = (1..=5).map(|p| (PlayerId(p), 100)).collect();
    let old = classic_rules::ai::rival_model::row(&d, 6, &met);
    assert!(old[0].value > 5, "the old estimate hides the disarmed neighbour: {}", old[0].value);
    // The others are bounded by what was seen and the rest of the sum.
    assert!(g[1..].iter().all(|x| x.lo >= 3 && x.hi <= 14), "{g:?}");
}

/// Without the exclusion (two rivals might be the worst), the sum above the floors is shared by score, and
/// every value stays in [max(worst, seen), best].
#[test]
fn the_rest_of_the_sum_is_shared_by_score() {
    let d = demo(10, 30, 20, 2);
    let evidence = [
        SoldiersEvidence { player: PlayerId(1), lower: 0, score: 100 },
        SoldiersEvidence { player: PlayerId(2), lower: 0, score: 500 },
        SoldiersEvidence { player: PlayerId(3), lower: 0, score: 100 },
    ];
    let g = soldiers(&d, 4, &evidence);
    assert!(g[1].value > g[0].value, "the nation with the larger score has more: {g:?}");
    assert_eq!(g[0].value, g[2].value);
    assert!(g.iter().all(|x| x.lo >= 2 && x.hi <= 30 && x.value >= x.lo && x.value <= x.hi), "{g:?}");
    // A nation not met: the worst may be its; nothing is assigned.
    let g = soldiers(&demo(10, 30, 20, 2), 5, &evidence);
    assert!(g.iter().all(|x| x.lo != x.hi), "no exact value while a nation is unmet: {g:?}");
}

/// A duel: exact, as the row.
#[test]
fn two_nations_are_exact() {
    let g = soldiers(&demo(5, 9, 7, 5), 2, &[SoldiersEvidence { player: PlayerId(1), lower: 0, score: 100 }]);
    assert_eq!(g, vec![Guess::exact(9)]);
}

/// R4 on a constructed world: three nations, we have 12 warriors, A has 1 (an average score), B has 8
/// that stand next to our city in sight. The demographics' worst is A's 1.
#[test]
fn r4_the_disarmed_neighbour_reads_at_most_100() {
    let rules = rules();
    let mut game = world(&rules, 3, &[(8, 12), (30, 12), (20, 4)]);
    for q in 0..12 {
        spawn(&mut game, &rules, US, 6 + q % 4, 10 + q / 4, "warrior");
    }
    spawn(&mut game, &rules, A, 30, 14, "warrior");
    for (q, r) in [(10, 11), (10, 12), (10, 13), (11, 11), (11, 12), (11, 13), (12, 11), (12, 12)] {
        spawn(&mut game, &rules, B, q, r, "warrior");
    }
    game.debug_meet(US, A);
    game.debug_meet(US, B);
    let (mut state, mut memory) = (AiState::new(US, 1), InputMemory::default());
    let i = read(&game, &rules, US, &mut state, &mut memory);
    let a = rival(&i, A);
    assert!(a.soldiers <= 100, "the disarmed A: {}", a.soldiers);
    let b = rival(&i, B);
    assert!(b.soldiers > 300, "B has units: {}", b.soldiers);
    assert_eq!(memory.soldiers_of(A).unwrap().doubt(), 0, "exact");
    assert!(b.seen_army > 0, "B's units are in sight");
}

// ---- the seat's inputs ----

#[test]
fn g1_g2_the_capital_and_the_cities_lost() {
    let rules = rules();
    let mut game = world(&rules, 2, &[(8, 12), (30, 12)]);
    let second = game.debug_found_city(&rules, US, tile(&game, 12, 12));
    game.debug_meet(US, A);
    let (mut state, mut memory) = (AiState::new(US, 1), InputMemory::default());
    let i = read(&game, &rules, US, &mut state, &mut memory);
    assert_eq!((i.seat.capital_lost, i.seat.cities_lost), (0, 0));
    // The second city falls; our unit next to it keeps it in sight, so its holder is known.
    spawn(&mut game, &rules, US, 13, 12, "warrior");
    advance(&mut game, &rules, 1);
    let i = read(&game, &rules, US, &mut state, &mut memory);
    game.debug_capture_city(&rules, second, A);
    advance(&mut game, &rules, 1);
    let i2 = read(&game, &rules, US, &mut state, &mut memory);
    let _ = i;
    assert_eq!(i2.seat.cities_lost, 333, "one of three");
    assert_eq!(i2.seat.capital_lost, 0);
    assert_eq!(rival(&i2, A).holds_cities, 500, "A holds one of the two the row counts");
    // The capital falls.
    let capital = game.view().auth.indexes().cities_of(US).iter().copied().find(|c| game.view().auth.cities().get(*c).map(|c| c.tile) == Some(tile(&game, 8, 12))).unwrap();
    game.debug_capture_city(&rules, capital, A);
    advance(&mut game, &rules, 1);
    let i3 = read(&game, &rules, US, &mut state, &mut memory);
    assert_eq!(i3.seat.capital_lost, 1000);
    assert_eq!(rival(&i3, A).holds_capital, 1000);
    assert_eq!(i3.seat.cities_lost, 666);
}

#[test]
fn g3_g4_war_rounds_and_the_gain() {
    let rules = rules();
    let mut game = world(&rules, 2, &[(8, 12), (30, 12)]);
    let theirs = game.debug_found_city(&rules, A, tile(&game, 12, 12));
    game.debug_meet(US, A);
    let (mut state, mut memory) = (AiState::new(US, 1), InputMemory::default());
    let i = read(&game, &rules, US, &mut state, &mut memory);
    assert_eq!((i.seat.war_rounds, i.seat.war_gain), (0, 500), "peace: even");
    spawn(&mut game, &rules, US, 13, 12, "warrior");
    game.debug_force_war(&rules, US, A);
    advance(&mut game, &rules, 15);
    let i = read(&game, &rules, US, &mut state, &mut memory);
    assert_eq!(i.seat.war_rounds, 500, "15 of 30 rounds");
    assert_eq!(rival(&i, A).war_rounds, 500);
    assert_eq!(i.seat.war_gain, 500);
    game.debug_capture_city(&rules, theirs, US);
    advance(&mut game, &rules, 1);
    let i = read(&game, &rules, US, &mut state, &mut memory);
    assert_eq!(i.seat.war_gain, 750, "one capture");
    assert_eq!(rival(&i, A).captured_recent, 1000);
}

#[test]
fn g5_the_happiness_falls() {
    let rules = rules();
    let mut game = world(&rules, 2, &[(8, 12), (30, 12)]);
    game.debug_meet(US, A);
    let mut state = AiState::new(US, 1);
    let mut memory = InputMemory::default();
    let mut last = 0;
    for (step, happy) in [(0, 8), (6, 8), (6, 3)] {
        advance(&mut game, &rules, step);
        let obs = game.ai_observation(&rules, US);
        let mut brief = planner::observe(&obs, &mut state, &rules);
        brief.happiness = happy;
        let model = estimate(&obs, &rules, &Book::default());
        last = inputs::read(&obs, &brief, &state, &rules, &neutral(&[]), &model, &mut memory).seat.unhappy_trend;
    }
    assert_eq!(last, 500, "5 points in 12 rounds, against a window of 10");
}

#[test]
fn g6_g7_g8_embark_air_and_gold() {
    let rules = rules();
    let mut game = world(&rules, 2, &[(8, 12), (30, 12)]);
    game.debug_meet(US, A);
    let (mut state, mut memory) = (AiState::new(US, 1), InputMemory::default());
    let before = read(&game, &rules, US, &mut state, &mut memory).seat;
    assert_eq!((before.embark, before.enemy_air), (0, 0));
    let tech = rules.technologies.iter().position(|t| t.allows_embarking).expect("the rules have an embarking technology");
    game.debug_learn_tech(&rules, US, TechId(tech as u16));
    game.debug_force_war(&rules, US, A);
    spawn(&mut game, &rules, A, 9, 12, "fighter");
    spawn(&mut game, &rules, A, 9, 13, "fighter");
    let have = game.view().auth.players().treasury_of(US);
    game.debug_earn(US, 1500u32.saturating_sub(have));
    advance(&mut game, &rules, 1);
    let after = read(&game, &rules, US, &mut state, &mut memory).seat;
    assert_eq!(after.embark, 1000);
    assert_eq!(after.enemy_air, 500, "two of four");
    assert!(after.gold_stock >= 500, "{}", after.gold_stock);
}

#[test]
fn g9_g10_city_states_held_and_contested() {
    use classic_rules::rules::content::CityStateId;
    use classic_rules::world::player::{ClassicKind, PlayerKind};
    let rules = rules();
    let mut game = world(&rules, 4, &[(8, 12), (30, 12), (20, 3), (20, 20)]);
    for cs in [B, PlayerId(3)] {
        game.debug_set_player_kind(cs, PlayerKind::Module(ClassicKind::CityState(CityStateId(0))));
        game.debug_meet(US, cs);
        game.debug_meet(A, cs);
    }
    // One we ally; one A allies and we trail by 20 (the leader is read as our own alliances' mean).
    game.debug_set_influence(&rules, B, US, 90);
    game.debug_set_influence(&rules, PlayerId(3), A, 90);
    game.debug_set_influence(&rules, PlayerId(3), US, 70);
    let (mut state, mut memory) = (AiState::new(US, 1), InputMemory::default());
    let seat = read(&game, &rules, US, &mut state, &mut memory).seat;
    assert_eq!(seat.cs_allies, 250, "one of four");
    assert_eq!(seat.cs_contested, 250, "one city-state where we trail the leader by under 30");
    game.debug_set_influence(&rules, PlayerId(3), US, 5);
    advance(&mut game, &rules, 1);
    assert_eq!(read(&game, &rules, US, &mut state, &mut memory).seat.cs_contested, 0, "a long way behind is not a cheap flip");
}

// ---- the per-rival inputs ----

#[test]
fn r1_r2_r3_nearness_and_the_ways() {
    let rules = rules();
    // Land: a city of A 9 tiles away on open ground.
    let mut game = world(&rules, 2, &[(8, 12), (17, 12)]);
    game.debug_meet(US, A);
    let all: Vec<TileId> = (0..game.view().auth.map().dimensions.tile_count() as u32).map(TileId).collect();
    game.debug_reveal(US, &all);
    let (mut state, mut memory) = (AiState::new(US, 1), InputMemory::default());
    spawn(&mut game, &rules, US, 9, 12, "warrior");
    spawn(&mut game, &rules, A, 17, 13, "warrior");
    // A's city has to be seen to be a target: the seat sights it from a scout's range.
    spawn(&mut game, &rules, US, 15, 12, "warrior");
    let i = read(&game, &rules, US, &mut state, &mut memory);
    let a = rival(&i, A);
    assert_eq!(a.near, 600, "nine tiles: 1000 at 5, 0 at 15");
    assert_eq!((a.land, a.sea), (1000, 0), "a walkable way: the war machine's own test");

    // Sea: A's city on an island behind a strait, we know embarkation and have a coastal city.
    let mut game = world(&rules, 2, &[(8, 12), (30, 12)]);
    let coast = TerrainId(rules.terrains.iter().find(|t| t.key == "core:terrain/coast").unwrap().map_code.unwrap());
    let ocean: Vec<(TileId, TerrainId)> = (0..24).flat_map(|r| (15..=22).map(move |q| (q, r))).map(|(q, r)| (tile(&game, q, r), coast)).collect();
    game.debug_set_terrain_batch(&rules, &ocean);
    game.debug_meet(US, A);
    let all: Vec<TileId> = (0..game.view().auth.map().dimensions.tile_count() as u32).map(TileId).collect();
    game.debug_reveal(US, &all);
    let (mut state, mut memory) = (AiState::new(US, 1), InputMemory::default());
    // The city of A was seen by a ship earlier: the state remembers it.
    state.see_city(classic_rules::ai::state::SeenCity { tile: tile(&game, 23, 12), owner: A, population: 1, last_seen_round: 1 });
    // A coastal city of ours and a coastal one of A's.
    game.debug_found_city(&rules, US, tile(&game, 14, 12));
    game.debug_found_city(&rules, A, tile(&game, 23, 12));
    spawn(&mut game, &rules, US, 14, 13, "warrior");
    let i = read(&game, &rules, US, &mut state, &mut memory);
    let a = rival(&i, A);
    assert_eq!(a.land, 0, "no way over the sea");
    assert_eq!(a.sea, 0, "no embarkation known yet");
    let tech = rules.technologies.iter().position(|t| t.allows_embarking).unwrap();
    game.debug_learn_tech(&rules, US, TechId(tech as u16));
    advance(&mut game, &rules, 11);
    let i = read(&game, &rules, US, &mut state, &mut memory);
    assert_eq!(rival(&i, A).sea, 1000, "a coastal target within range, and embarkation: the sea route");
}

#[test]
fn r6_open_cities_and_the_garrison_flag() {
    let rules = rules();
    let mut game = world(&rules, 2, &[(8, 12), (14, 12)]);
    game.debug_meet(US, A);
    spawn(&mut game, &rules, US, 13, 12, "warrior");
    let (mut state, mut memory) = (AiState::new(US, 1), InputMemory::default());
    let i = read(&game, &rules, US, &mut state, &mut memory);
    assert_eq!(rival(&i, A).open_cities, 1000, "the city and its neighbours in sight, no soldier");
    spawn(&mut game, &rules, A, 15, 12, "warrior");
    advance(&mut game, &rules, 1);
    let i = read(&game, &rules, US, &mut state, &mut memory);
    assert_eq!(rival(&i, A).open_cities, 0, "a soldier next to it");
}

/// Our capital at (8, 12) with one warrior, the rival `A` met at peace and five of its warriors two tiles off, one
/// round played: the stack at the border of R11 and of `the_stack_that_declares_war_stays_a_menace`.
fn stack_at_the_border(rules: &CompiledRuleset) -> GameState {
    let mut game = world(rules, 2, &[(8, 12), (30, 12)]);
    game.debug_meet(US, A);
    spawn(&mut game, rules, US, 8, 11, "warrior");
    for i in 0..5 {
        spawn(&mut game, rules, A, 10 + i % 2, 11 + i / 2, "warrior");
    }
    advance(&mut game, rules, 1);
    game
}

#[test]
fn r11_a_stack_at_the_border() {
    let rules = rules();
    let mut game = world(&rules, 2, &[(8, 12), (30, 12)]);
    game.debug_meet(US, A);
    spawn(&mut game, &rules, US, 8, 11, "warrior");
    let (mut state, mut memory) = (AiState::new(US, 1), InputMemory::default());
    let none = read(&game, &rules, US, &mut state, &mut memory);
    assert_eq!(rival(&none, A).stack, 0);
    let mut game = stack_at_the_border(&rules);
    let (mut state, mut memory) = (AiState::new(US, 1), InputMemory::default());
    let i = read(&game, &rules, US, &mut state, &mut memory);
    let stack = rival(&i, A).stack;
    assert!(stack > 500, "five against one: {stack}");
    assert_eq!(i.cities[0].stack_owner, Some(A));
    // At war the stack is the attack itself: the input stays (work/nets/capitals: read at peace only, the menace and
    // every lever answering it fell to 0 on the turn of the declaration).
    game.debug_force_war(&rules, US, A);
    advance(&mut game, &rules, 1);
    let i = read(&game, &rules, US, &mut state, &mut memory);
    let at_war = rival(&i, A).stack;
    assert!(at_war > 500, "the same five against one at war: {at_war}");
}

/// The pass of `net2` before the intent on the inputs read from the game (`levers::before_intent`).
fn pass(inputs: &Inputs, game: &GameState) -> Pass {
    let mut carried = [0; kc::N];
    carried[kc::WARLIKE] = 300;
    levers::before_intent(inputs, &carried, &CoreMemory::default(), game.view().auth.turn().round, 100, Parts::All)
}

#[test]
fn the_stack_that_declares_war_stays_a_menace() {
    // work/nets/capitals, offset 3340 seed 3340001 seat 4: a rival's stack at our capital declares war. With R11 read at
    // peace only, `menace` fell from 930 to 0 on that turn, and with it the levers that answered the stack (the
    // military price tilt 800 · max menace, the guard 1200 · stack menace, the defence posture 1500 · max menace); the
    // capital fell 10 rounds later. Read from the game, the answer holds across the declaration.
    let rules = rules();
    let mut game = stack_at_the_border(&rules);
    let (mut state, mut memory) = (AiState::new(US, 1), InputMemory::default());
    let peace = read(&game, &rules, US, &mut state, &mut memory);
    let before = pass(&peace, &game);
    game.debug_force_war(&rules, US, A);
    advance(&mut game, &rules, 1);
    let war = read(&game, &rules, US, &mut state, &mut memory);
    let after = pass(&war, &game);
    let menace = |p: &Pass| p.rivals.iter().find(|r| r.rival == A).unwrap().concepts[q::MENACE];
    assert!(menace(&before) > 500, "the stack at peace is a menace: {}", menace(&before));
    assert!(menace(&after) >= menace(&before), "the attack is no less a menace than the stack before it: {} then {}", menace(&before), menace(&after));
    assert!(after.aggregates[a::MAX_MENACE] >= before.aggregates[a::MAX_MENACE], "the levers read the max menace");
    assert!(levers::tilt(&after).posture[1] >= levers::tilt(&before).posture[1], "defence keeps its term");
}

#[test]
fn r12_to_r17_the_diplomacy_flags() {
    let rules = rules();
    let mut game = world(&rules, 3, &[(8, 12), (30, 12), (20, 3)]);
    game.debug_meet(US, A);
    game.debug_meet(US, B);
    game.debug_deal_item(&rules, US, A, DealItem::DefensivePact).ok();
    game.debug_force_war(&rules, US, B);
    let (mut state, mut memory) = (AiState::new(US, 1), InputMemory::default());
    let i = read(&game, &rules, US, &mut state, &mut memory);
    let b = rival(&i, B);
    assert_eq!((b.at_war, b.friend, b.pact), (1000, 0, 0));
    let a = rival(&i, A);
    assert_eq!(a.at_war, 0);
    // The wars of others are announced to nobody: R22 stays 0 (module doc of `rival_inputs`).
    game.debug_force_war(&rules, A, B);
    let i = read(&game, &rules, US, &mut state, &mut memory);
    assert_eq!(rival(&i, A).ally_hit, 0);
    assert_eq!(rival(&i, B).ally_hit, 0);
}

#[test]
fn r13_a_war_after_a_tie_is_a_betrayal() {
    let rules = rules();
    let mut game = world(&rules, 2, &[(8, 12), (30, 12)]);
    game.debug_meet(US, A);
    game.debug_deal_item(&rules, US, A, DealItem::OpenBorders).expect("open borders");
    let (mut state, mut memory) = (AiState::new(US, 1), InputMemory::default());
    assert_eq!(rival(&read(&game, &rules, US, &mut state, &mut memory), A).betrayed, 0);
    advance(&mut game, &rules, 3);
    game.debug_force_war(&rules, A, US);
    advance(&mut game, &rules, 1);
    let i = read(&game, &rules, US, &mut state, &mut memory);
    assert_eq!(rival(&i, A).betrayed, 500, "one betrayal of two");
}

#[test]
fn r21_a_forward_city_and_a_citadel() {
    let rules = rules();
    let mut game = world(&rules, 2, &[(8, 12), (30, 12)]);
    game.debug_meet(US, A);
    spawn(&mut game, &rules, US, 11, 12, "warrior");
    let (mut state, mut memory) = (AiState::new(US, 1), InputMemory::default());
    assert_eq!(rival(&read(&game, &rules, US, &mut state, &mut memory), A).grab, 0);
    advance(&mut game, &rules, 5);
    game.debug_found_city(&rules, A, tile(&game, 12, 12));
    advance(&mut game, &rules, 1);
    let i = read(&game, &rules, US, &mut state, &mut memory);
    assert_eq!(rival(&i, A).grab, 1000, "a city of A four tiles from ours, founded after it");
    assert_eq!(i.cities[0].rivals_near, vec![A]);

    // A city of A near the citadel: its culture bomb needs a city of the owner within reach of the tiles.
    let mut game = world(&rules, 2, &[(8, 12), (14, 12)]);
    game.debug_meet(US, A);
    spawn(&mut game, &rules, US, 10, 11, "warrior");
    let (mut state, mut memory) = (AiState::new(US, 1), InputMemory::default());
    let _ = read(&game, &rules, US, &mut state, &mut memory);
    assert!(game.debug_raise_citadel(&rules, A, tile(&game, 11, 12)));
    advance(&mut game, &rules, 1);
    let i = read(&game, &rules, US, &mut state, &mut memory);
    assert_eq!(rival(&i, A).grab, 500, "a citadel of A by our land");
}

#[test]
fn r23_the_force_where_the_war_would_be() {
    let rules = rules();
    let mut game = world(&rules, 2, &[(8, 12), (16, 12)]);
    game.debug_meet(US, A);
    for i in 0..3 {
        spawn(&mut game, &rules, US, 13 + i, 12, "warrior");
    }
    spawn(&mut game, &rules, A, 17, 12, "warrior");
    let (mut state, mut memory) = (AiState::new(US, 1), InputMemory::default());
    let i = read(&game, &rules, US, &mut state, &mut memory);
    assert!(rival(&i, A).local > 600, "three against one: {}", rival(&i, A).local);
}

#[test]
fn r24_the_score_share_is_level_for_equals() {
    let rules = rules();
    let mut game = world(&rules, 2, &[(8, 12), (30, 12)]);
    game.debug_meet(US, A);
    let (mut state, mut memory) = (AiState::new(US, 1), InputMemory::default());
    let i = read(&game, &rules, US, &mut state, &mut memory);
    assert_eq!(rival(&i, A).score, 500);
}

// ---- fog and bytes ----

/// A rival's city and units out of sight do not move any input of the seat.
#[test]
fn the_fog_is_respected() {
    let rules = rules();
    let mut game = world(&rules, 2, &[(8, 12), (30, 12)]);
    game.debug_meet(US, A);
    let (mut state, mut memory) = (AiState::new(US, 1), InputMemory::default());
    let before = read(&game, &rules, US, &mut state, &mut memory);
    for i in 0..4 {
        spawn(&mut game, &rules, A, 30 + i % 2, 14 + i / 2, "warrior");
    }
    game.debug_found_city(&rules, A, tile(&game, 33, 20));
    let after = read(&game, &rules, US, &mut state, &mut memory);
    // The demographics' soldiers row and the score list are public: only R4 and R24 may move.
    let strip = |mut i: Inputs| {
        for r in &mut i.rivals {
            r.soldiers = 0;
            r.score = 0;
        }
        i
    };
    assert_eq!(strip(before), strip(after));
}

#[test]
fn the_memory_round_trips_and_a_wrong_version_does_not_read() {
    let rules = rules();
    let mut game = world(&rules, 2, &[(8, 12), (14, 12)]);
    game.debug_meet(US, A);
    spawn(&mut game, &rules, US, 12, 12, "warrior");
    spawn(&mut game, &rules, A, 15, 12, "warrior");
    let (mut state, mut memory) = (AiState::new(US, 1), InputMemory::default());
    let _ = read(&game, &rules, US, &mut state, &mut memory);
    memory.soldiers.clear();
    let mut bytes = Vec::new();
    memory.encode(&mut bytes);
    bytes.extend_from_slice(&[9, 9, 9]);
    let (back, used) = InputMemory::decode(&bytes).unwrap();
    assert_eq!(used, bytes.len() - 3, "it takes its own bytes");
    assert_eq!(back, memory);
    assert!(!memory.foreign.is_empty() && !memory.sights.is_empty());
    let mut bad = bytes.clone();
    bad[0] = 99;
    assert!(InputMemory::decode(&bad).is_none());
    assert!(InputMemory::decode(&bytes[..bytes.len() - 6]).is_none(), "a cut body is no memory");
}

// ---- the tactical hand-over: `narrowed` ----

#[test]
fn narrowed_cuts_what_a_smaller_sensor_collects() {
    let rules = rules();
    let mut game = world(&rules, 2, &[(8, 12), (30, 12)]);
    game.debug_meet(US, A);
    spawn(&mut game, &rules, US, 8, 11, "warrior");
    spawn(&mut game, &rules, US, 12, 11, "warrior");
    spawn(&mut game, &rules, A, 9, 11, "warrior");
    spawn(&mut game, &rules, A, 13, 11, "warrior");
    let handle = *game.view().auth.indexes().units_of(US).iter().next().unwrap();
    let (wide, _) = game.ai_local_observation(&rules, US, handle, 6).unwrap();
    let (small, _) = game.ai_local_observation(&rules, US, handle, 2).unwrap();
    assert!(wide.foreign_units.len() > small.foreign_units.len());
    let cut = wide.narrowed(2);
    assert_eq!(cut.foreign_units, small.foreign_units);
    assert_eq!(cut.own_units, small.own_units);
    assert_eq!(cut.foreign_cities, small.foreign_cities);
    assert_eq!(cut.radius, 2);
    assert_eq!(wide.narrowed(9).foreign_units, wide.foreign_units, "a wider radius gives the observation itself");
    assert_eq!(wide.narrowed(9).radius, 6);
}

// ---- the estimator against the truth on played games (run with `--ignored --nocapture`) ----

/// Plays six-nation games of the game's AI and, at every fifth round, compares for every seat and every
/// met rival its soldiers by the old estimate (score-shared, `rival_model::estimate`) and by the fixed one
/// (`net2`'s memory of sightings, `inputs::read`) with the truth (the rival's own demographics value).
/// Prints the mean absolute error of both, over every reading and over the readings where the rival has
/// at most half of the seat's soldiers (the disarmed neighbour).
#[test]
#[ignore]
fn the_soldiers_estimate_against_the_truth_on_played_games() {
    use classic_rules::ai::{AiDriver, AiSettings};
    use classic_rules::derived::standing::demographics;
    use classic_rules::scenario::{generate_new_game, ScenarioConfig};
    let rules = rules();
    let seeds: Vec<u64> = std::env::var("NET2_SEEDS").ok().map(|v| v.split(',').filter_map(|s| s.parse().ok()).collect()).unwrap_or_else(|| vec![11, 12, 13]);
    let rounds: u32 = std::env::var("NET2_ROUNDS").ok().and_then(|v| v.parse().ok()).unwrap_or(150);
    let soldiers_at = Measure::ALL.iter().position(|m| *m == Measure::Soldiers).unwrap();
    // (readings, old error, new error, doubt) overall and for the disarmed.
    let (mut all, mut weak) = ((0i64, 0i64, 0i64), (0i64, 0i64, 0i64));
    let mut exact = 0;
    let mut exact_wrong = 0;
    let (mut in_range, mut sums) = (0, 0);
    for seed in seeds {
        let mut game = generate_new_game(&rules, &ScenarioConfig { seed, players: 6, width: 60, height: 40, cities: 0, units: 0, water_ratio: 15 });
        let mut driver = AiDriver::new(AiSettings::for_rules(&rules, rules.default_difficulty()), Vec::new());
        let mut seats: Vec<(PlayerId, AiState, InputMemory)> = (0..6).map(|p| (PlayerId(p), AiState::new(PlayerId(p), 1), InputMemory::default())).collect();
        let mut last = 0;
        while game.view().auth.turn().round < rounds {
            let report = driver.run(&mut game, &rules, 200);
            assert!(!report.waiting);
            let round = game.view().auth.turn().round;
            if !report.round_done || round == last {
                continue;
            }
            last = round;
            for (seat, state, memory) in seats.iter_mut() {
                if !game.view().auth.players().is_alive(*seat) || game.view().auth.indexes().cities_of(*seat).is_empty() {
                    continue;
                }
                let obs = game.ai_observation(&rules, *seat);
                let brief = planner::observe(&obs, state, &rules);
                let model = estimate(&obs, &rules, &Book::default());
                let _ = inputs::read(&obs, &brief, state, &rules, &neutral(&[]), &model, memory);
                if round % 5 != 0 {
                    continue;
                }
                let own = model.own[soldiers_at];
                for r in &model.rivals {
                    let view = game.view();
                    let truth = demographics(view.auth, &rules, view.derived, r.player).iter().find(|d| d.measure == Measure::Soldiers).map_or(0, |d| d.value);
                    let old = r.rows[soldiers_at].value;
                    let Some(new) = memory.soldiers_of(r.player) else { continue };
                    let (e_old, e_new) = ((old - truth).abs(), (new.value - truth).abs());
                    all = (all.0 + 1, all.1 + e_old, all.2 + e_new);
                    if truth * 2 <= own {
                        weak = (weak.0 + 1, weak.1 + e_old, weak.2 + e_new);
                    }
                    if new.lo == new.hi {
                        exact += 1;
                        exact_wrong += i32::from(new.value != truth);
                    }
                    sums += 1;
                    in_range += i32::from(truth >= new.lo && truth <= new.hi);
                }
            }
        }
    }
    let mean = |s: i64, n: i64| if n == 0 { 0.0 } else { s as f64 / n as f64 };
    eprintln!("soldiers: {} readings, mean abs error old {:.2} new {:.2}; exact values given {} ({} wrong); truth inside the range {}/{}", all.0, mean(all.1, all.0), mean(all.2, all.0), exact, exact_wrong, in_range, sums);
    eprintln!("soldiers of the disarmed (truth at most half of ours): {} readings, old {:.2} new {:.2}", weak.0, mean(weak.1, weak.0), mean(weak.2, weak.0));
}
