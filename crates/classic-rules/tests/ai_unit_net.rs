//! The unit net (`unit-net`, classic/docs/ai-unit-net.md): scenes of one turn
//! (or a few) that show the commander's plan and the tactician's choices —
//! the shooters before the capturer, the key target, concentrated fire, the
//! cover of a shooter, the wounded's retreat, the choke, the gathering, the
//! barbarians' reply, the civilian's flight, the Lanchester retreat — each
//! on open grassland with the scene's units. The truth is read only to set
//! the scene and judge it.

use classic_content::core::core_rules;
use classic_rules::ai::battlefield::KindStats;
use classic_rules::ai::driver::ClassicMind;
use classic_rules::ai::planner::TurnBrief;
use classic_rules::ai::state::AiState;
use classic_rules::ai::trace::{Actor, Outcome};
use classic_rules::ai::unit_net::commander::{self, Task};
use classic_rules::ai::unit_net::fields::{Picture, Theatre};
use classic_rules::ai::unit_net::{self, Prices};
use classic_rules::ai::variant::Variant;
use classic_rules::ai::AiSettings;
use classic_rules::classic::Classic;
use classic_rules::ids::{CityHandle, EntityId, PlayerId, ResourceId, TechId, TerrainId, TileId, UnitHandle, UnitKindId};
use classic_rules::knowledge::public::PlayerQueries;
use classic_rules::math::HexCoord;
use classic_rules::{compile_rules, ClassicGame, Command, CompiledRuleset, GameConfig, GameState};
use kernel::ai::driver::AiDriver;
use kernel::ai::mind::AiMind;

const US: PlayerId = PlayerId(0);
const THEM: PlayerId = PlayerId(1);
/// The codes of the map `GameState::new` builds (not the rules' list order).
const GRASSLAND: u16 = 1;

fn kind(rules: &CompiledRuleset, key: &str) -> UnitKindId {
    let full = format!("core:unit/{key}");
    UnitKindId(rules.units.iter().position(|u| u.key == full).unwrap_or_else(|| panic!("{full}")) as u16)
}

fn tile(game: &GameState, q: i32, r: i32) -> TileId {
    game.view().auth.map().dimensions.to_tile(HexCoord { q, r }).unwrap()
}

/// Open grassland 30 × 20 with our city at (2, 10) and theirs at (26, 10),
/// the map known, the two met, the techs of the ancient army, at war.
fn world(rules: &CompiledRuleset, seed: u64) -> GameState {
    let mut game = GameState::new(rules, GameConfig { players: 2, width: 30, height: 20, rng_seed: seed });
    let dims = game.view().auth.map().dimensions;
    let land: Vec<(TileId, TerrainId)> = (0..dims.tile_count() as u32).map(|t| (TileId(t), TerrainId(GRASSLAND))).collect();
    game.debug_set_terrain_batch(rules, &land);
    let tiles: Vec<TileId> = land.iter().map(|(t, _)| *t).collect();
    game.debug_clear_features(rules, &tiles);
    game.debug_set_river_edges(rules, &[]);
    game.debug_found_city(rules, US, tile(&game, 2, 10));
    game.debug_found_city(rules, THEM, tile(&game, 26, 10));
    let iron = rules.resources.iter().position(|r| r.key == "core:resource/iron").unwrap();
    game.debug_set_deposit(rules, tile(&game, 2, 10), ResourceId(iron as u16), 6);
    for p in [US, THEM] {
        for key in ["core:tech/bronze-working", "core:tech/iron-working", "core:tech/archery", "core:tech/mathematics", "core:tech/horseback-riding"] {
            let tech = rules.technologies.iter().position(|t| t.key == key).unwrap();
            game.debug_learn(rules, p, TechId(tech as u16));
        }
    }
    game.debug_meet(US, THEM);
    game.debug_reveal(US, &tiles);
    game.debug_reveal(THEM, &tiles);
    game.debug_earn(US, 500);
    game.execute(rules, Command::DeclareWar { player: US, enemy: THEM }).unwrap();
    game
}

fn spawn(game: &mut GameState, rules: &CompiledRuleset, owner: PlayerId, q: i32, r: i32, key: &str) -> UnitHandle {
    let t = tile(game, q, r);
    game.debug_spawn_unit(rules, owner, t, kind(rules, key))
}

fn id_of(game: &GameState, unit: UnitHandle) -> EntityId {
    game.view().auth.units().get(unit).unwrap().core.id
}

fn city_at(game: &GameState, t: TileId) -> CityHandle {
    game.view().auth.indexes().city_on_tile(t).unwrap()
}

fn owner_of_city(game: &GameState, t: TileId) -> Option<PlayerId> {
    let auth = game.view().auth;
    auth.indexes().city_on_tile(t).and_then(|c| auth.cities().get(c)).map(|c| c.owner)
}

/// Our turn played by `variant` (they are human: the driver stops after
/// ours): the applied commands of our units in order.
fn our_turn(game: &mut GameState, rules: &CompiledRuleset, variant: Variant) -> Vec<Command> {
    let settings = AiSettings::for_rules(rules, rules.default_difficulty());
    let mut driver = AiDriver::<Classic, ClassicMind>::with_mind(settings, vec![THEM], ClassicMind::new(variant));
    driver.set_trace_capacity(100_000);
    driver.run(game, rules, u32::MAX);
    driver
        .trace()
        .entries()
        .filter(|e| e.player == US && e.outcome == Outcome::Applied && matches!(e.actor, Actor::Unit(_)))
        .filter_map(|e| e.command.clone())
        .collect()
}

/// The brief of our turn under `unit-net`, the memory after it, and the
/// mind (the commander's operations are in the brief).
fn brief(game: &GameState, rules: &CompiledRuleset, state: &mut AiState, variant: Variant) -> (ClassicMind, TurnBrief) {
    let mind = ClassicMind::new(variant);
    let sight = mind.sense(game, rules, US);
    let start = mind.start_turn(&sight, rules, US, state);
    (mind, start.brief)
}

/// The theatre and the turn's plan as the commander makes them at the first
/// decision of `unit`.
fn plan_of(game: &GameState, rules: &CompiledRuleset, unit: UnitHandle, prices: Prices) -> commander::TurnPlan {
    let mut state = AiState::new(US, game.view().auth.turn().round);
    let (mind, brief) = brief(game, rules, &mut state, Variant::UnitNet);
    let settings = AiSettings::for_rules(rules, rules.default_difficulty());
    let near = mind.sense_unit(game, rules, &settings, US, unit).unwrap();
    let view = classic_rules::ai::planner::UnitView { local: &near, brief: &brief };
    let picture: Picture = unit_net::tactician::picture(&view, &state, &[]);
    let theatre = Theatre::build(&near.view, rules, &brief, &near.relations, &picture);
    let units = commander::plan_units(&theatre, rules, &near.relations);
    commander::plan(&theatre, &brief, rules, &prices, &units)
}

fn war() -> Prices {
    Prices::of_posture(3)
}

/// The expected blow of `unit` on the visible `target` (the rules' preview).
fn blow(game: &GameState, rules: &CompiledRuleset, unit: UnitHandle, target: TileId) -> u16 {
    let obs = game.ai_observation(rules, US);
    obs.view.attack_preview(unit, target).map(|p| p.to_defender.1).unwrap_or(0)
}

// ---- 1. the shooters before the capturer ----

/// A city two blows from falling: three archers in range and a swordsman
/// two tiles out that was created first (the driver's queue starts with
/// it). The commander's queue puts every shot before the capture (the
/// kernel's "who is next"), and the city falls this turn.
#[test]
fn the_archers_shoot_before_the_capturer() {
    let rules = compile_rules(core_rules()).unwrap();
    let mut taken = 0;
    let seeds = 6;
    for seed in 0..seeds {
        let mut game = world(&rules, 11 + seed);
        let target = tile(&game, 14, 10);
        game.debug_found_city(&rules, THEM, target);
        let sword = spawn(&mut game, &rules, US, 12, 10, "swordsman");
        let archers = [spawn(&mut game, &rules, US, 12, 11, "archer"), spawn(&mut game, &rules, US, 13, 12, "archer"), spawn(&mut game, &rules, US, 14, 8, "archer")];
        // The city's health: two expected shots and the swordsman's blow (from the wall).
        let shot = blow(&game, &rules, archers[0], target);
        let obs = game.ai_observation(&rules, US);
        let max = obs.visible_foreign_cities.iter().find(|c| c.tile == target).map(|c| c.hp).unwrap();
        let strike = obs.view.attack_preview_from(sword, tile(&game, 13, 10), target).map(|p| p.to_defender.1).unwrap_or(30);
        let left = 2 * shot + strike * 3 / 4;
        drop(obs);
        game.debug_set_city_damage(city_at(&game, target), max.saturating_sub(left));
        let commands = our_turn(&mut game, &rules, Variant::UnitNet);
        // A shooter's blow on a city is a shot whichever command carries it.
        let shooter = |u: &UnitHandle| archers.contains(u);
        let shots: Vec<usize> = commands.iter().enumerate().filter(|(_, c)| matches!(c, Command::RangedAttack { unit, target: t } if *t == target && shooter(unit)) || matches!(c, Command::AttackCity { attacker, .. } if shooter(attacker))).map(|(i, _)| i).collect();
        let capture = commands.iter().position(|c| matches!(c, Command::AttackCity { attacker, .. } if *attacker == sword));
        if let Some(capture) = capture {
            assert!(shots.iter().all(|s| *s < capture), "seed {seed}: every shot before the capture: {commands:?}");
            assert!(!shots.is_empty(), "seed {seed}: shots first");
        }
        taken += usize::from(owner_of_city(&game, target) == Some(US));
        let _ = id_of(&game, sword);
    }
    assert!(taken * 2 >= seeds as usize, "the city falls in most scenes: {taken} of {seeds}");
}

/// The tile a unit stands on now.
fn at(game: &GameState, unit: UnitHandle) -> Option<TileId> {
    game.view().auth.units().get(unit).map(|u| u.core.tile)
}

fn distance(game: &GameState, a: TileId, b: TileId) -> i32 {
    game.view().auth.map().dimensions.distance(a, b).unwrap()
}

/// The danger on `unit` where it stands (the theatre's D_u) and its health.
fn danger_now(game: &GameState, rules: &CompiledRuleset, unit: UnitHandle) -> (u16, u16) {
    let mut state = AiState::new(US, game.view().auth.turn().round);
    let (mind, brief) = brief(game, rules, &mut state, Variant::UnitNet);
    let settings = AiSettings::for_rules(rules, rules.default_difficulty());
    let near = mind.sense_unit(game, rules, &settings, US, unit).unwrap();
    let view = classic_rules::ai::planner::UnitView { local: &near, brief: &brief };
    let picture = unit_net::tactician::picture(&view, &state, &[]);
    let theatre = Theatre::build(&near.view, rules, &brief, &near.relations, &picture);
    (theatre.unit_danger(rules, &near.unit, near.unit.tile), near.unit.hp)
}

// ---- 2. two misses: the third archer before the capturer ----

/// Three archers and a swordsman against a damaged city; the city's health
/// is `hp_of(shot, strike)` (an archer's blow, the swordsman's blow). Returns
/// the plan, the archers that shoot in it, the archer left over and the
/// capturer.
fn city_siege(hp_of: fn(i64, i64) -> i64) -> (commander::TurnPlan, TileId, Vec<EntityId>, Vec<EntityId>, EntityId, (i64, i64)) {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = world(&rules, 11);
    let target = tile(&game, 14, 10);
    game.debug_found_city(&rules, THEM, target);
    let sword = spawn(&mut game, &rules, US, 12, 10, "swordsman");
    let archers = [spawn(&mut game, &rules, US, 12, 11, "archer"), spawn(&mut game, &rules, US, 13, 12, "archer"), spawn(&mut game, &rules, US, 14, 8, "archer")];
    let shot = blow(&game, &rules, archers[0], target);
    let obs = game.ai_observation(&rules, US);
    let max = obs.visible_foreign_cities.iter().find(|c| c.tile == target).map(|c| c.hp).unwrap();
    let strike = obs.view.attack_preview_from(sword, tile(&game, 13, 10), target).map(|p| p.to_defender.1).unwrap();
    drop(obs);
    let hp = hp_of(i64::from(shot), i64::from(strike)) as u16;
    game.debug_set_city_damage(city_at(&game, target), max - hp);
    let plan = plan_of(&game, &rules, sword, war());
    let ids: Vec<EntityId> = archers.iter().map(|a| id_of(&game, *a)).collect();
    let shooters = plan.assignments.iter().filter(|a| ids.contains(&a.unit)).map(|a| a.unit).collect();
    (plan, target, shooters, ids, id_of(&game, sword), (i64::from(shot), hp_of(i64::from(shot), i64::from(strike))))
}

/// The original scene: the city keeps `2 shots + 0.75 blow` of health, so two
/// shots and the capture do not make the kill near-certain (the damage
/// ranges overlap the health, about four in five). The marginal rule then
/// puts the third archer on the city as well (its price is small, the city is
/// worth sixteen): all three shoot, the capturer is last. An earlier version
/// of this test expected exactly two shooters: that came from the allocation
/// closing a city to further fire once a capturer was chosen, not from the
/// values, and it left a city with a one-in-five survival chance with an idle
/// archer beside it.
#[test]
fn a_kill_that_is_not_near_certain_gets_the_third_archer_too() {
    let (plan, target, shooters, _ids, sid, _) = city_siege(|shot, strike| 2 * shot + strike * 3 / 4);
    assert_eq!(shooters.len(), 3, "the kill is not near-certain with two shots: {:?} {:?}", plan.targets, plan.assignments);
    assert!(plan.targets.iter().any(|t| t.tile == target && t.p_kill >= 900), "{:?}", plan.targets);
    assert_eq!(plan.queue.last(), Some(&sid), "the capturer last: {:?}", plan.queue);
}

/// A city that two shots and the capture kill near-certainly (health
/// `1.5 shots + 0.5 blow`): the plan uses two archers and the capture and
/// keeps the third in reserve. Both shots roll low (the city keeps a third
/// of each shot more): the fast update puts the third archer on the city and
/// before the capturer in the queue.
#[test]
fn after_two_misses_a_third_archer_shoots_before_the_capturer() {
    let (mut plan, target, shooters, ids, sid, (shot, hp)) = city_siege(|shot, strike| 3 * shot / 2 + strike / 2);
    assert_eq!(shooters.len(), 2, "two shots and the capture: {:?}", plan.assignments);
    assert_eq!(plan.queue.last(), Some(&sid), "the capturer last: {:?}", plan.queue);
    let third = *ids.iter().find(|i| !shooters.contains(i)).unwrap();
    // Two low rolls: the city keeps a third of each shot more.
    let left = (hp - 2 * (shot / 3)) as u16;
    let healths: Vec<(EntityId, u16, i64)> = ids.iter().chain([&sid]).map(|i| (*i, 100, 1000)).collect();
    commander::update(&mut plan, &[(target, left)], &[], &shooters, &healths, &war());
    let q = &plan.queue;
    let (a, s) = (q.iter().position(|u| *u == third), q.iter().position(|u| *u == sid));
    assert!(a.is_some() && a < s, "the third archer before the capturer: {q:?} {:?}", plan.assignments);
    assert!(matches!(plan.task_of(third), Some(Task::Strike { target: t, .. }) if t == target));
}

// ---- 2b. N3: the fire goes where the city can be taken ----

/// Two enemy cities in reach of three archers and a swordsman; one is damaged so that the
/// force can take it this turn, the other stands whole. The fire is not spread: every strike
/// and the capture go to the city that can fall (the kill chance the fire has already built
/// there decides, not the other city's value).
#[test]
fn a_force_that_can_take_one_city_does_not_spread_its_fire_over_two() {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = world(&rules, 12);
    let weak = tile(&game, 14, 10);
    let whole = tile(&game, 14, 13);
    game.debug_found_city(&rules, THEM, weak);
    game.debug_found_city(&rules, THEM, whole);
    let sword = spawn(&mut game, &rules, US, 12, 10, "swordsman");
    let archers = [spawn(&mut game, &rules, US, 12, 11, "archer"), spawn(&mut game, &rules, US, 13, 12, "archer"), spawn(&mut game, &rules, US, 14, 8, "archer")];
    let shot = blow(&game, &rules, archers[0], weak);
    let obs = game.ai_observation(&rules, US);
    let max = obs.visible_foreign_cities.iter().find(|c| c.tile == weak).map(|c| c.hp).unwrap();
    let strike = obs.view.attack_preview_from(sword, tile(&game, 13, 10), weak).map(|p| p.to_defender.1).unwrap();
    drop(obs);
    let hp = (2 * shot + strike * 3 / 4) as u16;
    game.debug_set_city_damage(city_at(&game, weak), max - hp);
    let plan = plan_of(&game, &rules, sword, war());
    let elsewhere: Vec<_> = plan.assignments.iter().filter(|a| matches!(a.task, Task::Strike { target, .. } | Task::Take { target, .. } if target != weak)).collect();
    assert!(elsewhere.is_empty(), "no unit spent on the city that cannot fall: {:?} {:?}", plan.assignments, plan.targets);
    assert!(plan.assignments.iter().any(|a| matches!(a.task, Task::Take { target, .. } if target == weak)), "the capture of the weak city: {:?}", plan.assignments);
    let _ = whole;
}

// ---- 3. the key target ----

/// Our city at 15 hit points, an enemy swordsman next to it (it alone can
/// take the city next turn) and three enemy archers in reach: the
/// swordsman is the key target and both our archers shoot it.
#[test]
fn the_key_melee_that_could_take_our_city_is_shot_first() {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = world(&rules, 3);
    let home = tile(&game, 2, 10);
    let ours = city_at(&game, home);
    let full = game.ai_observation(&rules, US).own_cities.iter().find(|c| c.tile == home).map(|c| c.hp).unwrap_or(200);
    game.debug_set_city_damage(ours, full.saturating_sub(15));
    let sword = spawn(&mut game, &rules, THEM, 3, 10, "swordsman");
    for (q, r) in [(4, 9), (4, 11), (5, 10)] {
        spawn(&mut game, &rules, THEM, q, r, "archer");
    }
    let a1 = spawn(&mut game, &rules, US, 3, 8, "archer");
    spawn(&mut game, &rules, US, 2, 12, "archer");
    let plan = plan_of(&game, &rules, a1, Prices::of_posture(1));
    let key = at(&game, sword).unwrap();
    let t = plan.target_at(key).expect("the swordsman is a target");
    assert!(t.key, "the swordsman is the key: {:?}", plan.targets);
    assert!(plan.targets.iter().all(|o| o.tile == key || o.value < t.value), "{:?}", plan.targets);
    let strikes: Vec<TileId> = plan.assignments.iter().filter_map(|a| match a.task { Task::Strike { target, .. } => Some(target), _ => None }).collect();
    assert!(!strikes.is_empty() && strikes.iter().all(|s| *s == key), "every shot on the key: {:?}", plan.assignments);
}

// ---- 4. concentration ----

/// Four archers, two wounded spearmen (35 hp: two shots each) and a whole swordsman in
/// reach: the allocation kills both spearmen rather than spreading the
/// shots over the three.
#[test]
fn fire_is_concentrated_to_kill_rather_than_spread() {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = world(&rules, 4);
    let s1 = spawn(&mut game, &rules, THEM, 13, 9, "spearman");
    let s2 = spawn(&mut game, &rules, THEM, 13, 11, "spearman");
    spawn(&mut game, &rules, THEM, 14, 10, "swordsman");
    game.debug_set_unit_hp(s1, 35);
    game.debug_set_unit_hp(s2, 35);
    let a = spawn(&mut game, &rules, US, 11, 10, "archer");
    for (q, r) in [(11, 9), (11, 11), (12, 12)] {
        spawn(&mut game, &rules, US, q, r, "archer");
    }
    let plan = plan_of(&game, &rules, a, war());
    let wounded_only: Vec<_> = plan.targets.iter().filter(|t| t.assigned.1 > 0 && t.p_kill < 500).collect();
    assert!(plan.expected.kills >= 2, "both spearmen die: {:?}", plan.targets);
    assert!(wounded_only.len() <= 1, "no shot spread for a wound alone: {:?}", plan.targets);
}

// ---- 5. cover ----

/// Our archer stands where an enemy warrior reaches it; our spearman is
/// free. The plan sends the spearman to the tile that keeps the warrior
/// off, and the tactician puts it there.
#[test]
fn a_melee_unit_covers_an_exposed_archer() {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = world(&rules, 5);
    let archer = spawn(&mut game, &rules, US, 10, 10, "archer");
    let spear = spawn(&mut game, &rules, US, 9, 10, "spearman");
    spawn(&mut game, &rules, THEM, 12, 10, "warrior");
    let plan = plan_of(&game, &rules, spear, Prices::of_posture(1));
    let ward = id_of(&game, archer);
    let cover = plan.assignments.iter().find_map(|a| match a.task { Task::Cover { ward: w, tile } if w == ward => Some(tile), _ => None });
    let tile_ = cover.unwrap_or_else(|| panic!("a cover of the archer: {:?}", plan.assignments));
    let (before, _) = danger_now(&game, &rules, archer);
    our_turn(&mut game, &rules, Variant::UnitNet);
    assert_eq!(at(&game, spear), Some(tile_), "the spearman stands on the cover tile");
    let (after, _) = danger_now(&game, &rules, archer);
    assert!(after < before, "the archer's danger falls: {before} → {after}");
}

// ---- 6. the wounded retreat ----

/// A swordsman at 25 hit points two tiles from two whole enemy swordsmen
/// steps back to a tile where it survives their turn.
#[test]
fn a_wounded_unit_retreats_to_a_safe_tile() {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = world(&rules, 6);
    let me = spawn(&mut game, &rules, US, 12, 10, "swordsman");
    game.debug_set_unit_hp(me, 25);
    spawn(&mut game, &rules, THEM, 14, 10, "swordsman");
    spawn(&mut game, &rules, THEM, 14, 11, "swordsman");
    let (before, hp) = danger_now(&game, &rules, me);
    assert!(before >= hp, "doomed where it stands: {before} of {hp}");
    our_turn(&mut game, &rules, Variant::UnitNet);
    let (after, hp) = danger_now(&game, &rules, me);
    assert!(after < hp, "safe after the turn: {after} of {hp} at {:?}", at(&game, me));
}

// ---- 7. the choke ----

/// A pass two tiles long through mountains between our city and an enemy
/// army; our spearman holds it (stands in it or at its mouth on our side).
#[test]
#[ignore = "known gap: the choke bonus does not yet hold the pass; see docs/ai-unit-net.md"]
fn a_chokepoint_is_held() {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = world(&rules, 7);
    const MOUNTAIN: u16 = 6;
    let mut walls = Vec::new();
    for q in [10, 11] {
        for r in 0..20 {
            if r != 10 {
                walls.push((tile(&game, q, r), TerrainId(MOUNTAIN)));
            }
        }
    }
    game.debug_set_terrain_batch(&rules, &walls);
    let spear = spawn(&mut game, &rules, US, 8, 10, "spearman");
    for (q, r) in [(15, 9), (15, 10), (15, 11)] {
        spawn(&mut game, &rules, THEM, q, r, "warrior");
    }
    our_turn(&mut game, &rules, Variant::UnitNet);
    let pass = [tile(&game, 10, 10), tile(&game, 11, 10), tile(&game, 9, 10), tile(&game, 9, 11)];
    let end = at(&game, spear).unwrap();
    assert!(pass.contains(&end), "the spearman holds the pass: ends at {end:?}");
}

// ---- 8. gathering, no trickle ----

/// An operation on the march with its front member four tiles ahead of the
/// others: the front member waits, the others close up.
#[test]
fn the_operation_gathers_with_no_trickle() {
    use classic_rules::ai::state::{Composition, Operation, OperationPhase, SeenCity, UnitTask};
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = world(&rules, 8);
    let target = tile(&game, 26, 10);
    let lead = spawn(&mut game, &rules, US, 16, 10, "swordsman");
    let rest = [spawn(&mut game, &rules, US, 12, 10, "swordsman"), spawn(&mut game, &rules, US, 12, 11, "archer"), spawn(&mut game, &rules, US, 11, 11, "spearman")];
    let settings = AiSettings::for_rules(&rules, rules.default_difficulty());
    let mut driver = AiDriver::<Classic, ClassicMind>::with_mind(settings, vec![THEM], ClassicMind::new(Variant::UnitNet));
    let round = game.view().auth.turn().round;
    let muster = tile(&game, 2, 10);
    {
        let state = driver.state_mut(US, round);
        state.see_city(SeenCity { tile: target, owner: THEM, population: 1, last_seen_round: round });
        state.start_operation(Operation { target, enemy: THEM, muster, need: Composition::default(), phase: OperationPhase::Marching, since_round: round, phase_round: round });
        for u in std::iter::once(lead).chain(rest) {
            state.set_task(id_of(&game, u), UnitTask::Operation { target }, round);
        }
        let memo = unit_net::memory::UnitMemory { ops: vec![unit_net::memory::OpMemory { kind: commander::OpKind::Attack, target, phase: commander::Phase::Approach, since: round, choice: 0 }] };
        state.mind_memory = memo.encode();
    }
    let before: Vec<i32> = rest.iter().map(|u| distance(&game, at(&game, *u).unwrap(), target)).collect();
    let lead_at = at(&game, lead);
    driver.run(&mut game, &rules, u32::MAX);
    assert_eq!(at(&game, lead), lead_at, "the front member waits");
    let after: Vec<i32> = rest.iter().map(|u| distance(&game, at(&game, *u).unwrap(), target)).collect();
    assert!(after.iter().zip(&before).filter(|(a, b)| a < b).count() >= 2, "the others close up: {before:?} → {after:?}");
}

// ---- 9. the barbarians' reply ----

/// A barbarian warrior next to our archer and our spearman: the commander's
/// prediction of its blow (the engine's barbarian rule on our picture) names
/// the unit the engine's barbarian AI attacks.
#[test]
fn the_barbarian_reply_is_the_engines_barbarian_rule() {
    use classic_rules::world::player::{ClassicKind, PlayerKind};
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = world(&rules, 9);
    game.debug_set_player_kind(THEM, PlayerKind::Module(ClassicKind::Barbarian));
    let archer = spawn(&mut game, &rules, US, 12, 10, "archer");
    let spear = spawn(&mut game, &rules, US, 12, 11, "spearman");
    spawn(&mut game, &rules, THEM, 13, 10, "warrior");
    let engine = classic_rules::ai::barbarians::barbarian_orders(&game.ai_observation(&rules, THEM), &rules);
    let struck = engine.iter().find_map(|c| match c {
        Command::AttackUnit { defender, .. } => Some(*defender),
        _ => None,
    });
    let struck = struck.expect("the barbarian attacks");
    let mut state = AiState::new(US, game.view().auth.turn().round);
    let (mind, brief) = brief(&game, &rules, &mut state, Variant::UnitNet);
    let settings = AiSettings::for_rules(&rules, rules.default_difficulty());
    let near = mind.sense_unit(&game, &rules, &settings, US, archer).unwrap();
    let view = classic_rules::ai::planner::UnitView { local: &near, brief: &brief };
    let picture = unit_net::tactician::picture(&view, &state, &[]);
    let theatre = Theatre::build(&near.view, &rules, &brief, &near.relations, &picture);
    let reply = commander::reply_now(&theatre, &brief, &rules, &Prices::of_posture(1));
    let predicted = reply.iter().filter(|e| e.1 > 0).max_by_key(|e| e.1).map(|e| e.0).expect("a predicted blow");
    assert_eq!(predicted, id_of(&game, struck), "the prediction names the engine's victim ({:?} / {:?}): {reply:?}", id_of(&game, archer), id_of(&game, spear));
}

// ---- 10. the civilian flees ----

/// A worker two tiles from an enemy warrior moves out of its reach.
#[test]
fn a_civilian_flees_danger() {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = world(&rules, 10);
    let worker = spawn(&mut game, &rules, US, 12, 10, "worker");
    let foe = spawn(&mut game, &rules, THEM, 14, 10, "warrior");
    let before = distance(&game, at(&game, worker).unwrap(), at(&game, foe).unwrap());
    our_turn(&mut game, &rules, Variant::UnitNet);
    let after = distance(&game, at(&game, worker).unwrap(), at(&game, foe).unwrap());
    assert!(after > before && after >= 3, "the worker steps away: {before} → {after}");
}

// ---- 11. the Lanchester retreat ----

/// An assault of two warriors on a walled city of ten: the forecast gives
/// no capture and steady losses, so the head turns the assault to a retreat.
#[test]
fn a_bad_lanchester_forecast_turns_the_assault_to_a_retreat() {
    use classic_rules::ai::state::{Composition, Operation, OperationPhase, SeenCity, UnitTask};
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = world(&rules, 12);
    let target = tile(&game, 26, 10);
    let city = city_at(&game, target);
    game.debug_set_population(&rules, city, 10);
    let walls = rules.buildings.iter().position(|b| b.key == "core:building/walls").unwrap();
    game.debug_complete_building(&rules, city, classic_rules::ids::BuildingKindId(walls as u16));
    let members = [spawn(&mut game, &rules, US, 23, 10, "warrior"), spawn(&mut game, &rules, US, 23, 11, "warrior")];
    let round = game.view().auth.turn().round;
    let mut state = AiState::new(US, round);
    state.see_city(SeenCity { tile: target, owner: THEM, population: 10, last_seen_round: round });
    state.start_operation(Operation { target, enemy: THEM, muster: tile(&game, 2, 10), need: Composition::default(), phase: OperationPhase::Marching, since_round: round, phase_round: round });
    for u in members {
        state.set_task(id_of(&game, u), UnitTask::Operation { target }, round);
    }
    let memo = unit_net::memory::UnitMemory { ops: vec![unit_net::memory::OpMemory { kind: commander::OpKind::Attack, target, phase: commander::Phase::Assault, since: round, choice: 0 }] };
    state.mind_memory = memo.encode();
    let (_, brief) = brief(&game, &rules, &mut state, Variant::UnitNet);
    let turn = brief.unit_net.as_ref().unwrap();
    let op = turn.ops.iter().find(|o| o.target == target).expect("the operation");
    assert_eq!(op.phase, commander::Phase::Retreat, "{op:?}");
    assert_eq!(commander::OPTIONS[op.choice], "retreat");
}

// ---- 12. the doom rule ----

/// A whole archer in reach of three enemy swordsmen: whatever it shoots, it
/// does not end its turn where their blows reach its health (it kills
/// nothing there).
#[test]
fn no_turn_ends_where_the_danger_reaches_the_health() {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = world(&rules, 13);
    let archer = spawn(&mut game, &rules, US, 12, 10, "archer");
    for (q, r) in [(14, 9), (14, 10), (15, 10)] {
        spawn(&mut game, &rules, THEM, q, r, "swordsman");
    }
    our_turn(&mut game, &rules, Variant::UnitNet);
    let (danger, hp) = danger_now(&game, &rules, archer);
    assert!(danger < hp, "ends where it survives: {danger} of {hp} at {:?}", at(&game, archer));
}

// ---- 13. the orders ----

/// Our city with no guard and five enemy swordsmen three tiles out: a
/// defence operation, and an order of a defender the cities read.
#[test]
fn a_threatened_city_orders_a_defender() {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = world(&rules, 14);
    for (q, r) in [(5, 9), (5, 10), (5, 11), (6, 9), (6, 10)] {
        spawn(&mut game, &rules, THEM, q, r, "swordsman");
    }
    let mut state = AiState::new(US, game.view().auth.turn().round);
    let (_, brief) = brief(&game, &rules, &mut state, Variant::UnitNet);
    let turn = brief.unit_net.as_ref().unwrap();
    let home = tile(&game, 2, 10);
    assert!(turn.ops.iter().any(|o| o.kind == commander::OpKind::Defence && o.target == home), "{:?}", turn.ops);
    assert!(unit_net::order_value(turn, home, game.view().auth.map().dimensions) > 0, "{:?}", turn.orders);
}

// ---- 14. N1: the city's fire ----

/// The city's fire on `unit` where it stands (the theatre's share of the city's strike), the
/// city's whole blow on it, and the city's chance to fall this turn.
fn city_fire_on(game: &GameState, rules: &CompiledRuleset, unit: UnitHandle, city: TileId) -> (u16, u16, i64) {
    let mut state = AiState::new(US, game.view().auth.turn().round);
    let (mind, brief) = brief(game, rules, &mut state, Variant::UnitNet);
    let settings = AiSettings::for_rules(rules, rules.default_difficulty());
    let near = mind.sense_unit(game, rules, &settings, US, unit).unwrap();
    let view = classic_rules::ai::planner::UnitView { local: &near, brief: &brief };
    let picture = unit_net::tactician::picture(&view, &state, &[]);
    let theatre = Theatre::build(&near.view, rules, &brief, &near.relations, &picture);
    let (i, t) = theatre.target_on(city).expect("the city in the theatre");
    let blow = theatre.field.blow_by(t, &KindStats::of(rules, near.unit.kind), near.unit.hp, near.unit.tile, near.unit.fortified);
    let fire = theatre.city_fire(i, blow, near.unit.tile, &theatre.posture);
    (fire, blow, theatre.fall[i])
}

/// Five of our archers two tiles from a whole enemy city: the rules give a city one strike a
/// turn, so each archer carries about a fifth of it, not the whole blow. With the city worn so
/// far that the archers and a swordsman at its wall take it this turn, it strikes nobody: the
/// archers' danger from it is gone.
#[test]
fn the_city_strikes_once_and_not_at_all_when_it_falls() {
    let rules = compile_rules(core_rules()).unwrap();
    let ring = [(12, 10), (12, 11), (13, 12), (14, 8), (16, 9)];
    // A whole city, no capturer: a fifth of its blow on each archer.
    let mut game = world(&rules, 15);
    let target = tile(&game, 14, 10);
    game.debug_found_city(&rules, THEM, target);
    let archers: Vec<UnitHandle> = ring.iter().map(|&(q, r)| spawn(&mut game, &rules, US, q, r, "archer")).collect();
    let (fire, whole, fall) = city_fire_on(&game, &rules, archers[0], target);
    assert_eq!(fall, 0, "a city nobody can take does not fall");
    assert!(whole > 0 && fire > 0 && i64::from(fire) * 4 <= i64::from(whole), "a share of one strike: {fire} of {whole}");
    // Worn to three shots, with a swordsman at the wall: it falls, its fire is gone.
    let mut game = world(&rules, 15);
    game.debug_found_city(&rules, THEM, target);
    let archers: Vec<UnitHandle> = ring.iter().map(|&(q, r)| spawn(&mut game, &rules, US, q, r, "archer")).collect();
    spawn(&mut game, &rules, US, 13, 10, "swordsman");
    let shot = blow(&game, &rules, archers[0], target);
    let obs = game.ai_observation(&rules, US);
    let max = obs.visible_foreign_cities.iter().find(|c| c.tile == target).map(|c| c.hp).unwrap();
    drop(obs);
    game.debug_set_city_damage(city_at(&game, target), max.saturating_sub(3 * shot));
    let (fire, _, fall) = city_fire_on(&game, &rules, archers[0], target);
    assert!(fall >= 900, "the ring and the capture take it: {fall}");
    assert_eq!(fire, 0, "a falling city strikes nobody");
}

// ---- 15. N4 + N7: the capturer ahead ----

/// Two swordsmen at the walls of an enemy city and an archer in range. Whole, the city cannot
/// fall this turn: the plan keeps the swordsmen for the capture (no blow of theirs on it; the
/// archer shoots). Worn to one and a half of a swordsman's blows, neither takes it alone but
/// the two do: both are sent, and the shot first.
#[test]
fn the_capturers_strike_only_a_city_they_can_take_together() {
    let rules = compile_rules(core_rules()).unwrap();
    let scene = |worn: bool| {
        let mut game = world(&rules, 17);
        let target = tile(&game, 14, 10);
        game.debug_found_city(&rules, THEM, target);
        let swords = [spawn(&mut game, &rules, US, 13, 10, "swordsman"), spawn(&mut game, &rules, US, 15, 9, "swordsman")];
        let archer = spawn(&mut game, &rules, US, 12, 11, "archer");
        if worn {
            let obs = game.ai_observation(&rules, US);
            let max = obs.visible_foreign_cities.iter().find(|c| c.tile == target).map(|c| c.hp).unwrap();
            let strike = obs.view.attack_preview(swords[0], target).map(|p| p.to_defender.1).unwrap();
            drop(obs);
            game.debug_set_city_damage(city_at(&game, target), max - (strike * 3 / 2).max(1));
        }
        let plan = plan_of(&game, &rules, archer, war());
        let ids: Vec<EntityId> = swords.iter().map(|s| id_of(&game, *s)).collect();
        let takers = plan.assignments.iter().filter(|a| ids.contains(&a.unit) && matches!(a.task, Task::Take { target: t, .. } if t == target)).count();
        (plan, takers)
    };
    let (plan, takers) = scene(false);
    assert_eq!(takers, 0, "no capturer's blow on a city that cannot fall this turn: {:?} {:?}", plan.assignments, plan.targets);
    let (plan, takers) = scene(true);
    assert_eq!(takers, 2, "two takers that finish it together: {:?} {:?}", plan.assignments, plan.targets);
}
