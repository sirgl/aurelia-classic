//! Air units on the classic ruleset (classic/docs/classic/rules.md: air strike and
//! interception, nuclear weapons, stacking, aviation):
//! rebasing to cities and carriers, air strikes, interception by fighters
//! and ground or naval interceptors, air sweeps, carriers and cities with
//! aircraft inside, what a player may know about an enemy city's aircraft,
//! nuclear weapons, airlift, amphibious marines and the paradrop.
//!
//! Every test is deterministic: the game rng is seeded; statistical claims
//! loop over a fixed range of seeds.

use classic_content::core::core_rules;
use classic_rules::ids::{FeatureId, ImprovementId, PlayerId, PromotionId, TechId, TerrainId, TileId, UnitHandle, UnitKindId};
use classic_rules::math::{tiles_in_hex_radius, HexCoord};
use classic_rules::transaction::journal::UnitChange;
use classic_rules::world::state::GameConfig;
use classic_rules::{compile_rules, ClassicGame, Command, CompiledRuleset, GameState};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);
const P2: PlayerId = PlayerId(2);
const GRASS: TerrainId = TerrainId(1);
const WATER: TerrainId = TerrainId(4);
#[allow(dead_code)]
const MOUNTAIN: TerrainId = TerrainId(6);

// ---- helpers ----

fn rules() -> CompiledRuleset {
    compile_rules(core_rules()).unwrap()
}

fn world(rules: &CompiledRuleset, players: u8, width: u32, height: u32, seed: u64) -> GameState {
    GameState::new(rules, GameConfig { players, width, height, rng_seed: seed })
}

fn tile(game: &GameState, q: i32, r: i32) -> TileId {
    game.view().auth.map().dimensions.to_tile(HexCoord { q, r }).unwrap()
}

fn kind(rules: &CompiledRuleset, key: &str) -> UnitKindId {
    UnitKindId(rules.units.iter().position(|u| u.key == key).unwrap_or_else(|| panic!("{key}")) as u16)
}

fn tech(rules: &CompiledRuleset, key: &str) -> TechId {
    TechId(rules.technologies.iter().position(|t| t.key == key).unwrap_or_else(|| panic!("{key}")) as u16)
}

fn promotion(rules: &CompiledRuleset, key: &str) -> PromotionId {
    PromotionId(rules.promotions.iter().position(|p| p.key == key).unwrap_or_else(|| panic!("{key}")) as u16)
}

fn building(rules: &CompiledRuleset, key: &str) -> classic_rules::ids::BuildingKindId {
    classic_rules::ids::BuildingKindId(rules.buildings.iter().position(|b| b.key == key).unwrap_or_else(|| panic!("{key}")) as u16)
}

fn spawn(game: &mut GameState, rules: &CompiledRuleset, owner: PlayerId, q: i32, r: i32, key: &str) -> UnitHandle {
    let at = tile(game, q, r);
    game.debug_spawn_unit(rules, owner, at, kind(rules, key))
}

fn city(game: &mut GameState, rules: &CompiledRuleset, owner: PlayerId, q: i32, r: i32) -> classic_rules::ids::CityHandle {
    let at = tile(game, q, r);
    game.debug_found_city(rules, owner, at)
}

/// Strategic resources for `owner`: two far-away cities, one on oil (Biology
/// reveals it), one on aluminum (Electricity). Without the resource a unit
/// fights at -50 %, which the debug spawn does not care about.
fn supply(game: &mut GameState, rules: &CompiledRuleset, owner: PlayerId, q: i32) {
    let height = game.view().auth.map().dimensions.height as i32;
    let resource = |key: &str| classic_rules::ids::ResourceId(rules.resources.iter().position(|r| r.key == key).unwrap() as u16);
    let spots = [(tile(game, q, 0), "core:resource/oil"), (tile(game, q, height - 1), "core:resource/aluminum")];
    for (at, key) in spots {
        game.debug_set_resource_batch(rules, &[(at, resource(key))]);
        game.debug_found_city(rules, owner, at);
    }
    game.debug_learn(rules, owner, tech(rules, "core:tech/biology"));
    game.debug_learn(rules, owner, tech(rules, "core:tech/electricity"));
}

/// Water on the columns `from..to` of every row.
fn sea(game: &mut GameState, rules: &CompiledRuleset, from: i32, to: i32) {
    let dims = game.view().auth.map().dimensions;
    let batch: Vec<(TileId, TerrainId)> = (0..dims.height as i32)
        .flat_map(|r| (from..to).map(move |q| (q, r)))
        .map(|(q, r)| (dims.to_tile(HexCoord { q, r }).unwrap(), WATER))
        .collect();
    game.debug_set_terrain_batch(rules, &batch);
}

fn war(game: &mut GameState, rules: &CompiledRuleset, a: PlayerId, b: PlayerId) {
    game.debug_meet(a, b);
    game.debug_force_war(rules, a, b);
}

/// End turns until `player` is active.
fn to_player(game: &mut GameState, rules: &CompiledRuleset, player: PlayerId) {
    for _ in 0..8 {
        let active = game.view().auth.active_player();
        if active == player {
            return;
        }
        game.execute(rules, Command::EndTurn { player: active }).unwrap();
    }
    panic!("{player:?} never became active");
}

/// A new round for `player`: its units have their moves again.
fn next_round_of(game: &mut GameState, rules: &CompiledRuleset, player: PlayerId) {
    let active = game.view().auth.active_player();
    game.execute(rules, Command::EndTurn { player: active }).unwrap();
    to_player(game, rules, player);
}

fn core(game: &GameState, unit: UnitHandle) -> classic_rules::world::unit::UnitCore {
    *game.view().auth.units().get(unit).unwrap_or_else(|| panic!("unit {unit:?} is gone")).core
}

fn alive(game: &GameState, unit: UnitHandle) -> bool {
    game.view().auth.units().get(unit).is_some()
}

fn hp(game: &GameState, unit: UnitHandle) -> u16 {
    game.view().auth.units().get(unit).map(|u| u.core.hp).unwrap_or(0)
}

/// The result of a command as text: «ok» or the error's debug form.
fn run(game: &mut GameState, rules: &CompiledRuleset, command: Command) -> String {
    match game.execute(rules, command) {
        Ok(_) => "ok".to_string(),
        Err(e) => format!("{e:?}"),
    }
}

#[track_caller]
fn refused(game: &mut GameState, rules: &CompiledRuleset, command: Command, needle: &str) {
    let outcome = run(game, rules, command);
    assert!(outcome != "ok" && outcome.contains(needle), "expected a refusal with «{needle}», got {outcome}");
}

#[track_caller]
fn accepted(game: &mut GameState, rules: &CompiledRuleset, command: Command) {
    let outcome = run(game, rules, command);
    assert_eq!(outcome, "ok", "the command was refused");
}

fn max_city_hp(game: &GameState, rules: &CompiledRuleset, city: classic_rules::ids::CityHandle) -> u16 {
    let auth = game.view().auth;
    classic_rules::combat::city::max_hit_points(auth, rules, auth.cities().get(city).unwrap())
}

fn city_damage(game: &GameState, city: classic_rules::ids::CityHandle) -> u16 {
    game.view().auth.cities().get(city).unwrap().damage
}

// ============================================================================
// 1. Rebase
// ============================================================================

/// The limit is twice the unit's range (a fighter: 8 -> 16 tiles); a rebase
/// ends the turn, and a unit without moves cannot rebase.
#[test]
fn rebase_reaches_twice_the_range_and_ends_the_turn() {
    let rules = rules();
    let mut game = world(&rules, 2, 48, 12, 1);
    let home = city(&mut game, &rules, P0, 2, 5);
    let at_limit = city(&mut game, &rules, P0, 18, 5);
    let beyond = city(&mut game, &rules, P0, 19, 5);
    let near = city(&mut game, &rules, P0, 6, 5);
    let fighter = spawn(&mut game, &rules, P0, 2, 5, "core:unit/fighter");
    let _ = home;
    let range = i32::from(rules.units[kind(&rules, "core:unit/fighter").0 as usize].range);
    assert_eq!(range, 8, "the fighter's range in the data");
    let dims = game.view().auth.map().dimensions;
    let d = |c| {
        let t = game.view().auth.cities().get(c).unwrap().tile;
        dims.distance(core(&game, fighter).tile, t).unwrap()
    };
    assert_eq!((d(at_limit), d(beyond)), (16, 17), "the test geometry: exactly at and one beyond 2 x range");
    refused(&mut game, &rules, Command::Rebase { unit: fighter, city: beyond }, "out of range");
    assert_eq!(core(&game, fighter).moves_left, core(&game, fighter).moves_max, "a refusal changes nothing");
    accepted(&mut game, &rules, Command::Rebase { unit: fighter, city: at_limit });
    let c = core(&game, fighter);
    assert_eq!(c.tile, tile(&game, 18, 5));
    assert_eq!(c.moves_left, 0, "the rebase ends the turn");
    // No moves: no second rebase, not even a short one.
    refused(&mut game, &rules, Command::Rebase { unit: fighter, city: near }, "no moves left");
    next_round_of(&mut game, &rules, P0);
    accepted(&mut game, &rules, Command::Rebase { unit: fighter, city: near });
    assert_eq!(core(&game, fighter).tile, tile(&game, 6, 5));
    game.validate(&rules).unwrap();
}

/// Not an enemy city, not the unit's own tile, not a land unit.
#[test]
fn rebase_refuses_enemy_cities_the_same_tile_and_non_aircraft() {
    let rules = rules();
    let mut game = world(&rules, 2, 32, 12, 1);
    let home = city(&mut game, &rules, P0, 2, 5);
    let foreign = city(&mut game, &rules, P1, 8, 5);
    let fighter = spawn(&mut game, &rules, P0, 2, 5, "core:unit/fighter");
    let warrior = spawn(&mut game, &rules, P0, 2, 5, "core:unit/warrior");
    refused(&mut game, &rules, Command::Rebase { unit: fighter, city: foreign }, "not your city");
    war(&mut game, &rules, P0, P1);
    refused(&mut game, &rules, Command::Rebase { unit: fighter, city: foreign }, "not your city");
    refused(&mut game, &rules, Command::Rebase { unit: fighter, city: home }, "not your city");
    refused(&mut game, &rules, Command::Rebase { unit: warrior, city: home }, "not an aircraft");
    assert_eq!(core(&game, fighter).tile, tile(&game, 2, 5));
}

/// A city holds 6 aircraft, +4 with an airport.
#[test]
fn a_city_holds_six_aircraft_ten_with_an_airport() {
    let rules = rules();
    let mut game = world(&rules, 2, 32, 12, 1);
    let home = city(&mut game, &rules, P0, 2, 5);
    let base = city(&mut game, &rules, P0, 6, 5);
    let _ = home;
    for _ in 0..6 {
        spawn(&mut game, &rules, P0, 6, 5, "core:unit/fighter");
    }
    let fighters: Vec<UnitHandle> = (0..6).map(|_| spawn(&mut game, &rules, P0, 2, 5, "core:unit/fighter")).collect();
    refused(&mut game, &rules, Command::Rebase { unit: fighters[0], city: base }, "no room for aircraft");
    game.debug_add_building(&rules, base, building(&rules, "core:building/airport"));
    for f in &fighters[..4] {
        accepted(&mut game, &rules, Command::Rebase { unit: *f, city: base });
    }
    refused(&mut game, &rules, Command::Rebase { unit: fighters[4], city: base }, "no room for aircraft");
    game.validate(&rules).unwrap();
}

/// Based on a carrier: within twice the range, with cargo space, of the
/// right cargo kind (aircraft on a carrier, missiles on a missile cruiser or
/// a nuclear submarine), and only on an own carrier.
#[test]
fn rebase_to_a_carrier_checks_range_space_kind_and_owner() {
    let rules = rules();
    let mut game = world(&rules, 2, 48, 12, 1);
    sea(&mut game, &rules, 8, 48);
    let _home = city(&mut game, &rules, P0, 2, 5);
    // A fighter's reach is 16: carriers at distance 16 and 17.
    let near = spawn(&mut game, &rules, P0, 18, 5, "core:unit/carrier");
    let far = spawn(&mut game, &rules, P0, 19, 7, "core:unit/carrier");
    let enemy_carrier = spawn(&mut game, &rules, P1, 10, 7, "core:unit/carrier");
    let cruiser = spawn(&mut game, &rules, P0, 10, 3, "core:unit/missile-cruiser");
    let submarine = spawn(&mut game, &rules, P0, 11, 3, "core:unit/nuclear-submarine");
    let fighter = spawn(&mut game, &rules, P0, 2, 5, "core:unit/fighter");
    let missile = spawn(&mut game, &rules, P0, 2, 5, "core:unit/guided-missile");
    let nuke = spawn(&mut game, &rules, P0, 2, 5, "core:unit/nuclear-missile");
    let dims = game.view().auth.map().dimensions;
    assert_eq!(dims.distance(tile(&game, 2, 5), tile(&game, 18, 5)), Some(16));
    assert_eq!(dims.distance(tile(&game, 2, 5), tile(&game, 19, 7)), Some(19));
    refused(&mut game, &rules, Command::RebaseToCarrier { unit: fighter, carrier: far }, "out of range");
    refused(&mut game, &rules, Command::RebaseToCarrier { unit: fighter, carrier: enemy_carrier }, "not your carrier");
    // The wrong cargo kind: a missile on an aircraft carrier, an aircraft on
    // a missile ship.
    refused(&mut game, &rules, Command::RebaseToCarrier { unit: missile, carrier: near }, "cannot be carried there");
    refused(&mut game, &rules, Command::RebaseToCarrier { unit: nuke, carrier: near }, "cannot be carried there");
    refused(&mut game, &rules, Command::RebaseToCarrier { unit: fighter, carrier: cruiser }, "cannot be carried there");
    refused(&mut game, &rules, Command::RebaseToCarrier { unit: fighter, carrier: submarine }, "cannot be carried there");
    // The right ones.
    accepted(&mut game, &rules, Command::RebaseToCarrier { unit: fighter, carrier: near });
    assert_eq!(core(&game, fighter).tile, core(&game, near).tile);
    assert_eq!(core(&game, fighter).moves_left, 0, "the turn ends");
    refused(&mut game, &rules, Command::RebaseToCarrier { unit: fighter, carrier: near }, "no moves left");
    accepted(&mut game, &rules, Command::RebaseToCarrier { unit: missile, carrier: cruiser });
    accepted(&mut game, &rules, Command::RebaseToCarrier { unit: nuke, carrier: submarine });
    game.validate(&rules).unwrap();
}

/// Cargo space: a carrier takes 2 (Cargo II), a missile cruiser 3 missiles,
/// a nuclear submarine 2.
#[test]
fn carrier_cargo_space_is_two_a_missile_cruiser_three_a_submarine_two() {
    let rules = rules();
    let mut game = world(&rules, 2, 32, 12, 1);
    sea(&mut game, &rules, 8, 32);
    let _home = city(&mut game, &rules, P0, 6, 5);
    let carrier = spawn(&mut game, &rules, P0, 10, 5, "core:unit/carrier");
    let cruiser = spawn(&mut game, &rules, P0, 10, 6, "core:unit/missile-cruiser");
    let submarine = spawn(&mut game, &rules, P0, 10, 7, "core:unit/nuclear-submarine");
    let load = |game: &mut GameState, key: &str, host: UnitHandle, n: usize| -> Vec<String> {
        (0..n)
            .map(|_| {
                let u = spawn(game, &rules, P0, 6, 5, key);
                run(game, &rules, Command::RebaseToCarrier { unit: u, carrier: host })
            })
            .collect()
    };
    let r = load(&mut game, "core:unit/fighter", carrier, 3);
    assert_eq!(&r[..2], ["ok", "ok"]);
    assert!(r[2].contains("no room"), "{}", r[2]);
    let r = load(&mut game, "core:unit/guided-missile", cruiser, 4);
    assert_eq!(&r[..3], ["ok", "ok", "ok"]);
    assert!(r[3].contains("no room"), "{}", r[3]);
    let r = load(&mut game, "core:unit/guided-missile", submarine, 3);
    assert_eq!(&r[..2], ["ok", "ok"]);
    assert!(r[2].contains("no room"), "{}", r[2]);
}

// ============================================================================
// 2. Air strike
// ============================================================================

/// A bomber over a P1 unit: P0 city at (3,5), P1 xcom squad at (13,5)
/// (distance 10 = the bomber's range), a scout that sees the target.
struct Raid {
    game: GameState,
    bomber: UnitHandle,
    target: UnitHandle,
    target_tile: TileId,
}

fn raid(rules: &CompiledRuleset, seed: u64, bomber_key: &str, target_key: &str, target_q: i32, scout: bool) -> Raid {
    let mut game = world(rules, 2, 48, 12, seed);
    city(&mut game, rules, P0, 3, 5);
    supply(&mut game, rules, P0, 0);
    let bomber = spawn(&mut game, rules, P0, 3, 5, bomber_key);
    let target = spawn(&mut game, rules, P1, target_q, 5, target_key);
    if scout {
        spawn(&mut game, rules, P0, target_q - 1, 5, "core:unit/scout");
    }
    war(&mut game, rules, P0, P1);
    let target_tile = tile(&game, target_q, 5);
    Raid { game, bomber, target, target_tile }
}

#[test]
fn an_air_strike_needs_range_and_sight_and_costs_the_turn() {
    let rules = rules();
    // Out of range: distance 11 against a range of 10.
    let mut far = raid(&rules, 3, "core:unit/bomber", "core:unit/xcom-squad", 14, true);
    let cmd = Command::RangedAttack { unit: far.bomber, target: far.target_tile };
    refused(&mut far.game, &rules, cmd, "out of range");
    // In range but not seen: no scout, nobody sees the tile.
    let mut blind = raid(&rules, 3, "core:unit/bomber", "core:unit/xcom-squad", 13, false);
    let seen = blind.game.player_view(&rules, P0).is_visible(blind.target_tile);
    assert!(!seen, "the test needs an unseen tile");
    let cmd = Command::RangedAttack { unit: blind.bomber, target: blind.target_tile };
    refused(&mut blind.game, &rules, cmd, "not visible");
    // Seen and in range: the strike works, costs the turn and the answer hurts.
    let mut ok = raid(&rules, 3, "core:unit/bomber", "core:unit/xcom-squad", 13, true);
    let cmd = Command::RangedAttack { unit: ok.bomber, target: ok.target_tile };
    accepted(&mut ok.game, &rules, cmd.clone());
    let b = core(&ok.game, ok.bomber);
    assert_eq!(b.moves_left, 0, "the strike costs the turn");
    assert_eq!(b.attacks_made, 1);
    assert!(hp(&ok.game, ok.target) < 100, "the target is hurt");
    assert!(b.hp < 100, "the target answers an air strike");
    refused(&mut ok.game, &rules, cmd, "");
    ok.game.validate(&rules).unwrap();
}

/// AttackUnit on a tile is the same strike for an aircraft; aircraft never
/// walk, enemy city or not.
#[test]
fn aircraft_strike_by_attack_orders_and_never_move() {
    let rules = rules();
    let mut r = raid(&rules, 3, "core:unit/bomber", "core:unit/xcom-squad", 9, true);
    let near = tile(&r.game, 4, 5);
    refused(&mut r.game, &rules, Command::MoveUnit { unit: r.bomber, destination: near }, "");
    refused(&mut r.game, &rules, Command::MoveUnit { unit: r.bomber, destination: r.target_tile }, "");
    assert_eq!(core(&r.game, r.bomber).tile, tile(&r.game, 3, 5));
    accepted(&mut r.game, &rules, Command::AttackUnit { attacker: r.bomber, defender: r.target });
    assert!(hp(&r.game, r.target) < 100);
    // A fighter and a missile do not walk either.
    let mut g = world(&rules, 2, 24, 12, 1);
    city(&mut g, &rules, P0, 3, 5);
    let fighter = spawn(&mut g, &rules, P0, 3, 5, "core:unit/fighter");
    let missile = spawn(&mut g, &rules, P0, 3, 5, "core:unit/guided-missile");
    let to = tile(&g, 4, 5);
    refused(&mut g, &rules, Command::MoveUnit { unit: fighter, destination: to }, "");
    refused(&mut g, &rules, Command::MoveUnit { unit: missile, destination: to }, "");
}

/// A strike on a city hurts it (and the city answers) but never takes the
/// last hit point.
#[test]
fn an_air_strike_on_a_city_never_takes_the_last_hit_point() {
    let rules = rules();
    for seed in 0..6 {
        let mut game = world(&rules, 2, 48, 12, seed);
        city(&mut game, &rules, P0, 3, 5);
        let bomber = spawn(&mut game, &rules, P0, 3, 5, "core:unit/bomber");
        let enemy = city(&mut game, &rules, P1, 10, 5);
        spawn(&mut game, &rules, P0, 9, 5, "core:unit/scout");
        war(&mut game, &rules, P0, P1);
        let target = tile(&game, 10, 5);
        assert_eq!(city_damage(&game, enemy), 0);
        accepted(&mut game, &rules, Command::RangedAttack { unit: bomber, target });
        assert!(city_damage(&game, enemy) > 0, "seed {seed}: the city is hurt");
        assert!(hp(&game, bomber) < 100, "seed {seed}: the city answers");
        // 1 hp left: more strikes do not finish it.
        let max = max_city_hp(&game, &rules, enemy);
        game.debug_set_city_damage(enemy, max - 1);
        next_round_of(&mut game, &rules, P0);
        let again = spawn(&mut game, &rules, P0, 3, 5, "core:unit/bomber");
        accepted(&mut game, &rules, Command::RangedAttack { unit: again, target });
        assert_eq!(city_damage(&game, enemy), max - 1, "seed {seed}: a city keeps 1 hp against ranged fire");
        assert!(game.view().auth.cities().get(enemy).is_some());
    }
}

// ============================================================================
// 3. Interception and air sweep
// ============================================================================

/// One interceptor to place: kind, tile, in Intercept stance or not.
struct Guard<'a> {
    key: &'a str,
    q: i32,
    r: i32,
    stance: bool,
}

struct Duel {
    game: GameState,
    bomber: UnitHandle,
    targets: Vec<(UnitHandle, TileId)>,
    guards: Vec<UnitHandle>,
}

/// P0 bomber (city at (3,5)) against P1 xcom squads at (9,5) (+ (9,7) with
/// `two_targets`) under the sight of P0's scout; P1's `guards` are placed,
/// the ones with a stance order interception on P1's turn; it is P0's turn
/// when this returns.
fn duel(rules: &CompiledRuleset, seed: u64, bomber_key: &str, guards: &[Guard<'_>], two_targets: bool) -> Duel {
    let mut game = world(rules, 2, 48, 12, seed);
    city(&mut game, rules, P0, 3, 5);
    supply(&mut game, rules, P0, 0);
    let bomber = spawn(&mut game, rules, P0, 3, 5, bomber_key);
    let mut targets = Vec::new();
    let mut spots = vec![(9, 5)];
    if two_targets {
        spots.push((9, 7));
    }
    for (q, r) in spots {
        let unit = spawn(&mut game, rules, P1, q, r, "core:unit/xcom-squad");
        targets.push((unit, tile(&game, q, r)));
    }
    spawn(&mut game, rules, P0, 8, 6, "core:unit/scout");
    city(&mut game, rules, P1, 40, 5);
    supply(&mut game, rules, P1, 47);
    let guard_units: Vec<UnitHandle> = guards.iter().map(|g| spawn(&mut game, rules, P1, g.q, g.r, g.key)).collect();
    war(&mut game, rules, P0, P1);
    to_player(&mut game, rules, P1);
    for (g, u) in guards.iter().zip(&guard_units) {
        if g.stance {
            accepted(&mut game, rules, Command::Intercept { unit: *u });
        }
    }
    to_player(&mut game, rules, P0);
    Duel { game, bomber, targets, guards: guard_units }
}

impl Duel {
    fn strike(&mut self, rules: &CompiledRuleset, bomber: UnitHandle, target: usize) {
        let tile = self.targets[target].1;
        accepted(&mut self.game, rules, Command::RangedAttack { unit: bomber, target: tile });
    }

    fn interceptions(&self, guard: usize) -> u8 {
        self.game.view().auth.units().get(self.guards[guard]).map(|u| u.core.interceptions_made).unwrap_or(0)
    }

    fn bomber_lost(&self) -> u16 {
        100 - hp(&self.game, self.bomber)
    }

    fn target_lost(&self, target: usize) -> u16 {
        100 - hp(&self.game, self.targets[target].0)
    }
}

const FIGHTER: Guard<'static> = Guard { key: "core:unit/fighter", q: 14, r: 5, stance: true };

#[test]
fn a_fighter_on_interception_duty_intercepts_and_the_bomber_pays() {
    let rules = rules();
    let (mut lost_with, mut lost_without, mut dealt_with, mut dealt_without) = (0u32, 0u32, 0u32, 0u32);
    for seed in 0..12 {
        let mut plain = duel(&rules, seed, "core:unit/bomber", &[], false);
        let bomber = plain.bomber;
        plain.strike(&rules, bomber, 0);
        lost_without += u32::from(plain.bomber_lost());
        dealt_without += u32::from(plain.target_lost(0));
        let mut guarded = duel(&rules, seed, "core:unit/bomber", &[FIGHTER], false);
        let bomber = guarded.bomber;
        guarded.strike(&rules, bomber, 0);
        assert_eq!(guarded.interceptions(0), 1, "seed {seed}: Interception IV is a sure interception");
        lost_with += u32::from(guarded.bomber_lost());
        dealt_with += u32::from(guarded.target_lost(0));
    }
    assert!(lost_with > lost_without, "the bomber loses more under interception: {lost_with} against {lost_without}");
    assert!(dealt_with < dealt_without, "the strike deals less when intercepted: {dealt_with} against {dealt_without}");
}

#[test]
fn only_a_ready_enemy_fighter_in_range_intercepts() {
    let rules = rules();
    let own_side = |game: &mut GameState| {
        // P0's own fighter on interception duty does not intercept P0's strike.
        let f = spawn(game, &rules, P0, 5, 5, "core:unit/fighter");
        accepted(game, &rules, Command::Intercept { unit: f });
        f
    };
    // Not on duty, out of range (distance 9 > 8), on duty and in range.
    let guards = [
        Guard { key: "core:unit/fighter", q: 13, r: 5, stance: false },
        Guard { key: "core:unit/fighter", q: 18, r: 5, stance: true },
        Guard { key: "core:unit/fighter", q: 17, r: 5, stance: true },
    ];
    let mut d = duel(&rules, 5, "core:unit/bomber", &guards, false);
    let dims = d.game.view().auth.map().dimensions;
    assert_eq!(dims.distance(d.targets[0].1, tile(&d.game, 18, 5)), Some(9));
    assert_eq!(dims.distance(d.targets[0].1, tile(&d.game, 17, 5)), Some(8));
    let friend = own_side(&mut d.game);
    let bomber = d.bomber;
    d.strike(&rules, bomber, 0);
    assert_eq!(d.interceptions(0), 0, "a fighter without the duty stays out");
    assert_eq!(d.interceptions(1), 0, "out of the interception range");
    assert_eq!(d.interceptions(2), 1, "on duty and in range (exactly 8)");
    assert_eq!(core(&d.game, friend).interceptions_made, 0, "own fighters never intercept own strikes");
}

/// A fighter that moved this turn does not intercept (§2.7). Sequential
/// turns restore the moves at the end of the owner's turn, so the case is
/// reachable only in a simultaneous window: P1 orders interception, then
/// rebases (ending the stance), orders it again with no moves left.
#[test]
fn a_fighter_that_moved_this_turn_does_not_intercept() {
    let rules = rules();
    let guards = [
        Guard { key: "core:unit/fighter", q: 14, r: 5, stance: false },
        Guard { key: "core:unit/fighter", q: 14, r: 5, stance: false },
    ];
    let mut d = duel(&rules, 5, "core:unit/bomber", &guards, false);
    let second_base = city(&mut d.game, &rules, P1, 15, 5);
    d.game.open_window(0b11);
    let (moved, still) = (d.guards[0], d.guards[1]);
    for u in [moved, still] {
        accepted_as(&mut d, &rules, P1, Command::Intercept { unit: u });
    }
    accepted_as(&mut d, &rules, P1, Command::Rebase { unit: moved, city: second_base });
    assert_eq!(core(&d.game, moved).stance, classic_rules::world::unit::Stance::Ready, "moving ends the duty");
    accepted_as(&mut d, &rules, P1, Command::Intercept { unit: moved });
    assert_eq!(core(&d.game, moved).moves_left, 0);
    let bomber = d.bomber;
    let tile = d.targets[0].1;
    accepted_as(&mut d, &rules, P0, Command::RangedAttack { unit: bomber, target: tile });
    assert_eq!(d.interceptions(0), 0, "the fighter that rebased this turn stays out");
    assert_eq!(d.interceptions(1), 1, "the one that did not move intercepts");
}

fn accepted_as(d: &mut Duel, rules: &CompiledRuleset, seat: PlayerId, command: Command) {
    let outcome = match d.game.execute_as(seat, rules, command) {
        Ok(_) => "ok".to_string(),
        Err(e) => format!("{e:?}"),
    };
    assert_eq!(outcome, "ok");
}

/// One interception a turn; Sortie adds one.
#[test]
fn interceptions_per_turn_are_limited_and_sortie_adds_one() {
    let rules = rules();
    for (sortie, expected) in [(false, 1u8), (true, 2u8)] {
        let mut d = duel(&rules, 9, "core:unit/bomber", &[FIGHTER], true);
        if sortie {
            let f = d.guards[0];
            d.game.debug_grant_promotion(&rules, f, promotion(&rules, "core:promotion/sortie"));
        }
        let second = spawn(&mut d.game, &rules, P0, 3, 5, "core:unit/bomber");
        let first = d.bomber;
        d.strike(&rules, first, 0);
        d.strike(&rules, second, 1);
        assert_eq!(d.interceptions(0), expected, "sortie: {sortie}");
    }
}

/// Evasion: the stealth bomber (Evasion II, 100 %) is never intercepted, a
/// bomber with Evasion I (50 %) is intercepted about half of the time; the
/// plain Evasion promotion halves the interception damage.
#[test]
fn evasion_lowers_interception() {
    let rules = rules();
    let mut stealth = 0u32;
    let (mut half, mut plain) = (0u32, 0u32);
    let n = 40;
    for seed in 0..n {
        let mut d = duel(&rules, seed, "core:unit/stealth-bomber", &[FIGHTER], false);
        let bomber = d.bomber;
        d.strike(&rules, bomber, 0);
        stealth += u32::from(d.interceptions(0));
        let mut d = duel(&rules, seed, "core:unit/bomber", &[FIGHTER], false);
        let b = d.bomber;
        d.game.debug_grant_promotion(&rules, b, promotion(&rules, "core:promotion/evasion-i"));
        d.strike(&rules, b, 0);
        half += u32::from(d.interceptions(0));
        let mut d = duel(&rules, seed, "core:unit/bomber", &[FIGHTER], false);
        d.strike(&rules, b_of(&d), 0);
        plain += u32::from(d.interceptions(0));
    }
    assert_eq!(stealth, 0, "Evasion II: never intercepted");
    assert_eq!(plain, n as u32, "no evasion: always intercepted by Interception IV");
    assert!((8..=32).contains(&half), "Evasion I: about half of {n}, got {half}");
    // The damage promotion halves what the interception does.
    let (mut with, mut without) = (0u32, 0u32);
    for seed in 0..12 {
        let mut d = duel(&rules, seed, "core:unit/bomber", &[FIGHTER], false);
        let b = d.bomber;
        d.game.debug_grant_promotion(&rules, b, promotion(&rules, "core:promotion/evasion"));
        d.strike(&rules, b, 0);
        with += u32::from(d.bomber_lost());
        let mut d = duel(&rules, seed, "core:unit/bomber", &[FIGHTER], false);
        d.strike(&rules, b_of(&d), 0);
        without += u32::from(d.bomber_lost());
    }
    assert!(with < without, "Evasion: -50 % interception damage: {with} against {without}");
}

fn b_of(d: &Duel) -> UnitHandle {
    d.bomber
}

/// Ground and naval interceptors work with no stance: an anti-aircraft gun
/// next to the target (Interception IV: sure), a destroyer (Interception II:
/// 40 %); out of their range of 2 they do not.
#[test]
fn anti_aircraft_guns_and_destroyers_intercept_too() {
    let rules = rules();
    let mut gun = 0u32;
    let mut far_gun = 0u32;
    let mut ship = 0u32;
    let n = 40;
    for seed in 0..n {
        let guards = [
            Guard { key: "core:unit/anti-aircraft-gun", q: 10, r: 5, stance: false },
            Guard { key: "core:unit/anti-aircraft-gun", q: 12, r: 5, stance: false },
        ];
        let mut d = duel(&rules, seed, "core:unit/bomber", &guards, false);
        let bomber = d.bomber;
        d.strike(&rules, bomber, 0);
        gun += u32::from(d.interceptions(0));
        far_gun += u32::from(d.interceptions(1));
        let mut d = duel(&rules, seed, "core:unit/bomber", &[], false);
        // A coastal tile next to the target: the destroyer sails there.
        let wet = tile(&d.game, 10, 5);
        d.game.debug_set_terrain_batch(&rules, &[(wet, WATER)]);
        let ship_unit = spawn(&mut d.game, &rules, P1, 10, 5, "core:unit/destroyer");
        let b = d.bomber;
        d.strike(&rules, b, 0);
        ship += u32::from(core(&d.game, ship_unit).interceptions_made);
    }
    assert_eq!(gun, n as u32, "an anti-aircraft gun always intercepts, stance or not");
    assert_eq!(far_gun, 0, "three tiles away is out of its range of 2");
    assert!((8..=28).contains(&ship), "a destroyer (40 %) intercepts about 16 of {n}, got {ship}");
}

/// Air sweep: the sweeper fights the best air interceptor; against a ground
/// interceptor it takes nothing; without interceptors it only spends the
/// turn.
#[test]
fn an_air_sweep_fights_interceptors_and_otherwise_spends_the_turn() {
    let rules = rules();
    // A sweeper in the P0 city at (3,5) with the targets' tile at (9,5).
    let mut empty = duel(&rules, 2, "core:unit/fighter", &[], false);
    let sweeper = empty.bomber;
    let target = empty.targets[0].1;
    let far = tile(&empty.game, 20, 5);
    refused(&mut empty.game, &rules, Command::AirSweep { unit: sweeper, target: far }, "out of range");
    accepted(&mut empty.game, &rules, Command::AirSweep { unit: sweeper, target });
    let c = core(&empty.game, sweeper);
    assert_eq!((c.hp, c.moves_left, c.attacks_made), (100, 0, 1), "nobody to fight: the turn is spent");
    refused(&mut empty.game, &rules, Command::AirSweep { unit: sweeper, target }, "no attacks left");
    // A bomber cannot sweep.
    let mut b = duel(&rules, 2, "core:unit/bomber", &[], false);
    let (bomber, target) = (b.bomber, b.targets[0].1);
    refused(&mut b.game, &rules, Command::AirSweep { unit: bomber, target }, "cannot sweep");
    // Dogfight against an interceptor fighter.
    let mut hurt = 0;
    for seed in 0..8 {
        let mut d = duel(&rules, seed, "core:unit/fighter", &[FIGHTER], false);
        let (sweeper, target) = (d.bomber, d.targets[0].1);
        accepted(&mut d.game, &rules, Command::AirSweep { unit: sweeper, target });
        let guard_hp = d.game.view().auth.units().get(d.guards[0]).map(|u| u.core.hp).unwrap_or(0);
        let sweeper_hp = d.game.view().auth.units().get(sweeper).map(|u| u.core.hp).unwrap_or(0);
        if guard_hp < 100 || sweeper_hp < 100 {
            hurt += 1;
        }
    }
    assert_eq!(hurt, 8, "every dogfight hurt at least one side");
    // Against an anti-aircraft gun the sweeper takes no damage (the original game
    // rule: sweep interception damage modifier = 0); the gun spends its interception.
    let guards = [Guard { key: "core:unit/anti-aircraft-gun", q: 10, r: 5, stance: false }];
    let mut d = duel(&rules, 4, "core:unit/fighter", &guards, false);
    let (sweeper, target) = (d.bomber, d.targets[0].1);
    accepted(&mut d.game, &rules, Command::AirSweep { unit: sweeper, target });
    assert_eq!(hp(&d.game, sweeper), 100, "no damage from a ground interceptor");
    assert_eq!(d.interceptions(0), 1, "the gun used its interception");
}

// ============================================================================
// 4. Carriers and cities with aircraft
// ============================================================================

/// P0 carrier with two fighters at sea next to a P1 destroyer (P1 to move).
fn carrier_world(rules: &CompiledRuleset, carrier_hp: u16) -> (GameState, UnitHandle, [UnitHandle; 2], UnitHandle) {
    let mut game = world(rules, 2, 32, 12, 4);
    sea(&mut game, rules, 8, 32);
    city(&mut game, rules, P0, 4, 5);
    let carrier = spawn(&mut game, rules, P0, 12, 5, "core:unit/carrier");
    let fighters = [
        spawn(&mut game, rules, P0, 4, 5, "core:unit/fighter"),
        spawn(&mut game, rules, P0, 4, 5, "core:unit/fighter"),
    ];
    for f in fighters {
        accepted(&mut game, rules, Command::RebaseToCarrier { unit: f, carrier });
    }
    let destroyer = spawn(&mut game, rules, P1, 13, 5, "core:unit/destroyer");
    game.debug_set_unit_hp(carrier, carrier_hp);
    war(&mut game, rules, P0, P1);
    to_player(&mut game, rules, P1);
    (game, carrier, fighters, destroyer)
}

#[test]
fn a_ship_hurts_a_carrier_and_a_sunk_carrier_takes_its_aircraft_down() {
    let rules = rules();
    // Hurt: the carrier keeps floating with less health, the cargo stays.
    let (mut game, carrier, fighters, destroyer) = carrier_world(&rules, 100);
    accepted(&mut game, &rules, Command::AttackUnit { attacker: destroyer, defender: carrier });
    assert!(alive(&game, carrier) && hp(&game, carrier) < 100, "the carrier loses hit points");
    assert!(fighters.iter().all(|f| alive(&game, *f)), "a damaged carrier keeps its aircraft");
    // Sunk: every aircraft aboard is destroyed and the journal says so.
    let (mut game, carrier, fighters, destroyer) = carrier_world(&rules, 1);
    let ids: Vec<_> = std::iter::once(carrier).chain(fighters).map(|u| core(&game, u).id).collect();
    let result = game.execute(&rules, Command::AttackUnit { attacker: destroyer, defender: carrier }).unwrap();
    assert!(!alive(&game, carrier));
    for f in fighters {
        assert!(!alive(&game, f), "the aircraft sink with the carrier");
    }
    let destroyed: Vec<_> = result
        .changes()
        .unit_changes
        .iter()
        .filter_map(|c| match c {
            UnitChange::Destroyed { id, .. } => Some(*id),
            _ => None,
        })
        .collect();
    for id in &ids {
        assert!(destroyed.contains(id), "the journal lists a Destroyed for {id:?}: {destroyed:?}");
    }
    assert_eq!(destroyed.last(), Some(&ids[0]), "the carrier's entry is the last");
    game.validate(&rules).unwrap();
}

/// A P1 city that holds only aircraft: the aircraft do not defend, the city
/// falls to a melee attack at 0 hp, and the aircraft die with it.
#[test]
fn a_city_with_only_aircraft_is_captured_and_the_aircraft_are_destroyed() {
    let rules = rules();
    let mut game = world(&rules, 2, 32, 12, 6);
    city(&mut game, &rules, P0, 2, 2);
    let capital = city(&mut game, &rules, P1, 25, 10);
    let target = city(&mut game, &rules, P1, 12, 5);
    let _ = capital;
    let bare = {
        let warrior = spawn(&mut game, &rules, P0, 11, 5, "core:unit/warrior");
        war(&mut game, &rules, P0, P1);
        let plan = classic_rules::combat::plan_attack(
            game.view(),
            &rules,
            warrior,
            tile(&game, 12, 5),
            classic_rules::combat::exchange::AttackKind::Melee,
        )
        .unwrap();
        plan.defender_strength
    };
    let bomber = spawn(&mut game, &rules, P1, 12, 5, "core:unit/bomber");
    let fighter = spawn(&mut game, &rules, P1, 12, 5, "core:unit/fighter");
    let missile = spawn(&mut game, &rules, P1, 12, 5, "core:unit/guided-missile");
    let warrior = game.view().auth.indexes().units_on(tile(&game, 11, 5)).first().copied().unwrap();
    let plan = classic_rules::combat::plan_attack(
        game.view(),
        &rules,
        warrior,
        tile(&game, 12, 5),
        classic_rules::combat::exchange::AttackKind::Melee,
    )
    .unwrap();
    assert!(matches!(plan.defender, classic_rules::combat::exchange::Defender::City(_)), "the aircraft are not defenders");
    assert_eq!(plan.defender_strength, bare, "aircraft add nothing to the city's strength");
    let max = max_city_hp(&game, &rules, target);
    game.debug_set_city_damage(target, max - 1);
    let ids: Vec<_> = [bomber, fighter, missile].iter().map(|u| core(&game, *u).id).collect();
    let result = game.execute(&rules, Command::AttackCity { attacker: warrior, city: target }).unwrap();
    let auth = game.view().auth;
    assert_eq!(auth.cities().get(target).unwrap().owner, P0, "the city is captured");
    for u in [bomber, fighter, missile] {
        assert!(!alive(&game, u), "the aircraft are destroyed");
    }
    let created = result.changes().unit_changes.iter().filter(|c| matches!(c, UnitChange::Created(_))).count();
    assert_eq!(created, 0, "no aircraft are captured");
    let destroyed: Vec<_> = result
        .changes()
        .unit_changes
        .iter()
        .filter_map(|c| match c {
            UnitChange::Destroyed { id, .. } => Some(*id),
            _ => None,
        })
        .collect();
    for id in ids {
        assert!(destroyed.contains(&id), "the journal lists the destroyed aircraft");
    }
    game.validate(&rules).unwrap();
}

/// A city full of aircraft does not stop the owner's land units: the 1UPT
/// rule counts only the land classes. An enemy cannot stop in the city.
#[test]
fn aircraft_in_a_city_do_not_block_the_owners_land_units() {
    let rules = rules();
    let mut game = world(&rules, 2, 24, 12, 1);
    let home = city(&mut game, &rules, P0, 8, 5);
    let airport = building(&rules, "core:building/airport");
    game.debug_add_building(&rules, home, airport);
    for _ in 0..10 {
        spawn(&mut game, &rules, P0, 8, 5, "core:unit/fighter");
    }
    let warrior = spawn(&mut game, &rules, P0, 6, 5, "core:unit/warrior");
    let second = spawn(&mut game, &rules, P0, 6, 6, "core:unit/warrior");
    let worker = spawn(&mut game, &rules, P0, 7, 5, "core:unit/worker");
    let inside = tile(&game, 8, 5);
    accepted(&mut game, &rules, Command::MoveUnit { unit: warrior, destination: inside });
    accepted(&mut game, &rules, Command::MoveUnit { unit: worker, destination: inside });
    assert_eq!(core(&game, warrior).tile, inside);
    assert_eq!(core(&game, worker).tile, inside, "a land military, a civilian and ten aircraft share the city");
    // The normal rule stays: a second land military unit does not fit.
    refused(&mut game, &rules, Command::MoveUnit { unit: second, destination: inside }, "");
    game.validate(&rules).unwrap();
}

// ============================================================================
// 5. Fog and knowledge
// ============================================================================

#[test]
fn the_other_player_does_not_see_aircraft_in_an_unseen_city() {
    let rules = rules();
    let mut game = world(&rules, 2, 40, 12, 1);
    city(&mut game, &rules, P0, 3, 5);
    city(&mut game, &rules, P1, 30, 5);
    let bomber = spawn(&mut game, &rules, P1, 30, 5, "core:unit/bomber");
    let guard = spawn(&mut game, &rules, P1, 30, 5, "core:unit/warrior");
    let lone = spawn(&mut game, &rules, P1, 30, 8, "core:unit/warrior");
    war(&mut game, &rules, P0, P1);
    let listed = |game: &GameState, unit: UnitHandle| {
        let obs = game.ai_observation(&rules, P0);
        let seen_in_observation = obs.visible_foreign_units.iter().any(|u| u.handle == unit);
        let c = core(game, unit);
        let seen_by_view = game.player_view(&rules, P0).sees_unit(&c);
        assert_eq!(seen_in_observation, seen_by_view, "the AI's list and the view agree");
        seen_by_view
    };
    for u in [bomber, guard, lone] {
        assert!(!listed(&game, u), "nothing of the enemy is seen from afar");
    }
    // A scout next to the city: the garrison and the aircraft are seen, the
    // far unit still not.
    spawn(&mut game, &rules, P0, 28, 5, "core:unit/scout");
    assert!(listed(&game, guard), "a land unit in a seen city is listed");
    assert!(listed(&game, bomber), "an aircraft in a seen city is listed");
    assert!(!listed(&game, lone));
}

/// A strike needs a tile the player sees now: unexplored and merely
/// remembered tiles are refused.
#[test]
fn an_air_strike_cannot_target_an_unknown_or_a_remembered_tile() {
    let rules = rules();
    let mut r = raid(&rules, 3, "core:unit/bomber", "core:unit/xcom-squad", 13, false);
    let target = r.target_tile;
    assert!(!r.game.player_view(&rules, P0).is_known(target), "unexplored");
    refused(&mut r.game, &rules, Command::RangedAttack { unit: r.bomber, target }, "not visible");
    r.game.debug_reveal(P0, &[target]);
    assert!(r.game.player_view(&rules, P0).is_known(target) && !r.game.player_view(&rules, P0).is_visible(target));
    refused(&mut r.game, &rules, Command::RangedAttack { unit: r.bomber, target }, "not visible");
    // Seen: accepted.
    spawn(&mut r.game, &rules, P0, 12, 5, "core:unit/scout");
    accepted(&mut r.game, &rules, Command::RangedAttack { unit: r.bomber, target });
}

// ============================================================================
// 6. Nuclear weapons
// ============================================================================

const NUKE_MAP: (u32, u32) = (30, 20);
/// The launching city's column: the blast centre (14,10) is 8 tiles away.
const BASE_Q: i32 = 6;

/// P0 city at (6,10) with a nuclear weapon; the blast centre is (14,10),
/// known to P0 when `known`. P1 has its capital far away.
fn nuke_world_with(rules: &CompiledRuleset, seed: u64, weapon: &str, known: bool) -> (GameState, UnitHandle, TileId) {
    let mut game = world(rules, 3, NUKE_MAP.0, NUKE_MAP.1, seed);
    city(&mut game, rules, P0, BASE_Q, 10);
    city(&mut game, rules, P1, 27, 17);
    let bomb = spawn(&mut game, rules, P0, BASE_Q, 10, weapon);
    war(&mut game, rules, P0, P1);
    let centre = tile(&game, 14, 10);
    if known {
        game.debug_reveal(P0, &[centre]);
    }
    (game, bomb, centre)
}

fn nuke_world(rules: &CompiledRuleset, seed: u64, weapon: &str) -> (GameState, UnitHandle, TileId) {
    nuke_world_with(rules, seed, weapon, true)
}

fn ring_tiles(game: &GameState, centre: TileId, radius: i32) -> Vec<TileId> {
    let dims = game.view().auth.map().dimensions;
    tiles_in_hex_radius(&dims, centre, radius)
        .into_iter()
        .filter(|t| dims.distance(centre, *t) == Some(radius))
        .collect()
}

fn fallout(rules: &CompiledRuleset) -> FeatureId {
    FeatureId(rules.features.iter().position(|f| f.key == "core:feature/fallout").unwrap() as u16)
}

/// The blast is exactly radius 2: every unit at distance 0..=2 of a nuclear
/// missile's target is destroyed, every unit at distance 3 is untouched.
#[test]
fn the_blast_radius_is_exactly_two() {
    let rules = rules();
    let (mut game, missile, centre) = nuke_world(&rules, 1, "core:unit/nuclear-missile");
    let mut inside = Vec::new();
    let mut outside = Vec::new();
    for d in 0..=3 {
        for t in ring_tiles(&game, centre, d) {
            let unit = game.debug_spawn_unit(&rules, P1, t, kind(&rules, "core:unit/warrior"));
            if d <= 2 { inside.push(unit) } else { outside.push(unit) }
        }
    }
    assert_eq!((inside.len(), outside.len()), (19, 18));
    accepted(&mut game, &rules, Command::Nuke { unit: missile, target: centre });
    assert!(inside.iter().all(|u| !alive(&game, *u)), "distance 0..=2: destroyed");
    assert!(outside.iter().all(|u| hp(&game, *u) == 100), "distance 3: untouched");
    assert!(!alive(&game, missile), "the missile is spent");
    game.validate(&rules).unwrap();
}

/// Level 1 (atomic bomb): the centre is destroyed, the ring takes 30 + 2 x
/// rand(40) damage (30..=108 as drawn, not clamped by the rules table);
/// civilians die from the ring on.
#[test]
fn an_atomic_bomb_destroys_the_centre_and_wounds_the_ring() {
    let rules = rules();
    let mut ring_damage = Vec::new();
    for seed in 0..6 {
        let (mut game, bomb, centre) = nuke_world(&rules, seed, "core:unit/atomic-bomb");
        let middle = game.debug_spawn_unit(&rules, P1, centre, kind(&rules, "core:unit/warrior"));
        let ring: Vec<UnitHandle> = ring_tiles(&game, centre, 1)
            .into_iter()
            .chain(ring_tiles(&game, centre, 2))
            .map(|t| game.debug_spawn_unit(&rules, P1, t, kind(&rules, "core:unit/musketman")))
            .collect();
        let civilians: Vec<UnitHandle> = ring_tiles(&game, centre, 2)
            .into_iter()
            .map(|t| game.debug_spawn_unit(&rules, P1, t, kind(&rules, "core:unit/worker")))
            .collect();
        accepted(&mut game, &rules, Command::Nuke { unit: bomb, target: centre });
        assert!(!alive(&game, middle), "seed {seed}: the centre is destroyed");
        for u in &ring {
            let left = hp(&game, *u);
            let damage = 100 - left;
            ring_damage.push(if left == 0 { 100 } else { damage });
            assert!(left == 0 || (1..=70).contains(&left), "seed {seed}: ring damage 30..: hp left {left}");
        }
        assert!(civilians.iter().all(|u| !alive(&game, *u)), "seed {seed}: civilians die");
        game.validate(&rules).unwrap();
    }
    let wounded = ring_damage.iter().filter(|d| **d < 100).count();
    assert!(wounded > ring_damage.len() / 2, "most of the ring survives a level-1 bomb: {wounded} of {}", ring_damage.len());
    let mean = ring_damage.iter().map(|d| u32::from(*d)).sum::<u32>() as f64 / ring_damage.len() as f64;
    assert!((55.0..=90.0).contains(&mean), "mean ring damage is about 70 (30 + 2 x 19.5, some killed): {mean}");
}

/// Cities: level 1 halves the remaining hit points and takes 30..68 % of the
/// people; level 2 takes 60..78 % and destroys a non-capital city under 5
/// citizens.
#[test]
fn nuclear_strikes_hurt_cities_by_level() {
    let rules = rules();
    for (weapon, low, high) in [("core:unit/atomic-bomb", 30u32, 68u32), ("core:unit/nuclear-missile", 60, 78)] {
        let level2 = weapon.contains("nuclear-missile");
        for seed in 0..8 {
            let (mut game, bomb, centre) = nuke_world(&rules, seed, weapon);
            // Two plain cities of P1 in the blast (its capital is far away).
            let big = game.debug_found_city(&rules, P1, tile(&game, 15, 10));
            game.debug_set_population(&rules, big, 8);
            let small = game.debug_found_city(&rules, P1, tile(&game, 12, 10));
            game.debug_set_population(&rules, small, 3);
            let max = max_city_hp(&game, &rules, big);
            game.debug_set_city_damage(big, max - 10);
            accepted(&mut game, &rules, Command::Nuke { unit: bomb, target: centre });
            let auth = game.view().auth;
            match auth.cities().get(small).map(|c| c.population) {
                None => assert!(level2, "seed {seed}: only a level-2 strike destroys a small city"),
                Some(p) => {
                    assert!(!level2, "seed {seed}: a level-2 strike destroys a city under 5");
                    assert!((1..=3).contains(&p));
                }
            }
            // The big one survives: the people lost lie in the ranges and half
            // of the remaining hit points are gone, never the last one.
            let city = auth.cities().get(big).expect("a city of 8 survives both levels");
            let lost = 8 - city.population;
            let (lo, hi) = (8 * low / 100, 8 * high / 100);
            assert!((lo..=hi).contains(&lost), "{weapon} seed {seed}: lost {lost} of 8, expected {lo}..={hi}");
            assert_eq!(city.damage, (max - 10) + 10 * 50 / 100, "half of the remaining hit points gone");
        }
    }
}

/// An original capital is never destroyed, whatever the level or size.
#[test]
fn a_level_two_strike_spares_an_original_capital() {
    let rules = rules();
    let mut game = world(&rules, 3, NUKE_MAP.0, NUKE_MAP.1, 3);
    city(&mut game, &rules, P0, BASE_Q, 10);
    let capital = city(&mut game, &rules, P1, 14, 10);
    game.debug_set_population(&rules, capital, 2);
    let bomb = spawn(&mut game, &rules, P0, BASE_Q, 10, "core:unit/nuclear-missile");
    war(&mut game, &rules, P0, P1);
    let target = tile(&game, 14, 10);
    game.debug_reveal(P0, &[target]);
    accepted(&mut game, &rules, Command::Nuke { unit: bomb, target });
    let c = game.view().auth.cities().get(capital).expect("the original capital stands");
    assert!(c.original_capital);
    assert_eq!(c.population, 1, "but it is reduced to the last citizen");
    game.validate(&rules).unwrap();
}

/// Bomb Shelter (-75 %) lowers the people lost; the same seed makes the
/// comparison exact.
#[test]
fn a_bomb_shelter_lowers_the_nuclear_losses_of_a_city() {
    let rules = rules();
    let mut lost = [0u32; 2];
    for seed in 0..10 {
        for (i, shelter) in [false, true].into_iter().enumerate() {
            let (mut game, bomb, centre) = nuke_world(&rules, seed, "core:unit/atomic-bomb");
            let c = game.debug_found_city(&rules, P1, tile(&game, 14, 10));
            game.debug_set_population(&rules, c, 20);
            if shelter {
                game.debug_add_building(&rules, c, building(&rules, "core:building/bomb_shelter"));
            }
            let unit = game.debug_spawn_unit(&rules, P1, centre, kind(&rules, "core:unit/warrior"));
            accepted(&mut game, &rules, Command::Nuke { unit: bomb, target: centre });
            lost[i] += 20 - game.view().auth.cities().get(c).unwrap().population;
            if shelter {
                assert!(hp(&game, unit) > 0, "seed {seed}: the shelter saves the units in the city");
            }
        }
    }
    assert!(lost[1] * 2 < lost[0], "shelter {} against {} lost citizens", lost[1], lost[0]);
}

/// Fallout: the centre always, other land tiles half of the time, never on
/// water or under a city; improvements go; the feature is the data's Fallout.
#[test]
fn fallout_covers_the_centre_and_about_half_of_the_other_land() {
    let rules = rules();
    let mut other = 0usize;
    let mut land_total = 0usize;
    let farm = ImprovementId(rules.improvements.iter().position(|i| i.key == "core:improvement/farm").unwrap() as u16);
    for seed in 0..20 {
        let (mut game, bomb, centre) = nuke_world(&rules, seed, "core:unit/atomic-bomb");
        let sea_tile = ring_tiles(&game, centre, 2)[0];
        let city_tile = ring_tiles(&game, centre, 1)[0];
        game.debug_set_terrain_batch(&rules, &[(sea_tile, WATER)]);
        game.debug_found_city(&rules, P1, city_tile);
        game.debug_set_improvement_batch(&rules, &[(centre, farm)]);
        accepted(&mut game, &rules, Command::Nuke { unit: bomb, target: centre });
        let map = game.view().auth.map();
        assert_eq!(map.feature(centre), Some(Some(fallout(&rules))), "seed {seed}: the centre always gets fallout");
        assert_eq!(map.improvement(centre).flatten(), None, "the farm in the centre is gone");
        assert_eq!(map.feature(sea_tile).flatten(), None, "no fallout on water");
        assert_eq!(map.feature(city_tile).flatten(), None, "no fallout under a city");
        for t in tiles_in_hex_radius(&map.dimensions, centre, 2) {
            if t == centre || t == sea_tile || t == city_tile {
                continue;
            }
            land_total += 1;
            if map.feature(t).flatten() == Some(fallout(&rules)) {
                other += 1;
            }
        }
        // Outside the blast: nothing.
        for t in ring_tiles(&game, centre, 3) {
            assert_eq!(map.feature(t).flatten(), None);
        }
    }
    let share = other as f64 / land_total as f64;
    assert!((0.4..=0.6).contains(&share), "about half of the other land tiles: {other} of {land_total}");
}

/// What fallout does to a tile: the data's yields (-3/-3/-3 on top of the
/// terrain), which the yield query clamps at 0 for a citizen.
#[test]
fn fallout_takes_three_of_each_yield_and_nothing_else_is_modelled() {
    use classic_rules::derived::tile_yields::plot_yields;
    use classic_rules::world::facts::TruthFacts;
    let rules = rules();
    let mut game = world(&rules, 1, 12, 12, 1);
    let grass = tile(&game, 4, 4);
    game.debug_set_terrain_batch(&rules, &[(grass, GRASS)]);
    let yields = |game: &GameState| {
        let facts = TruthFacts::new(game.view().auth, &rules, P0);
        plot_yields(&facts, &rules, grass, &facts.techs)
    };
    let before = yields(&game);
    assert_eq!((before.food, before.production, before.gold), (2, 0, 0));
    game.debug_set_feature_batch(&rules, &[(grass, fallout(&rules))]);
    let after = yields(&game);
    assert_eq!((after.food, after.production, after.gold), (-1, -3, -3), "grassland 2/0/0 plus fallout -3/-3/-3");
    let def = &rules.features[fallout(&rules).0 as usize];
    assert!(def.no_improvement, "nothing can be built on fallout");
    assert_eq!(def.movement_cost, 2);
    assert!(def.removal.is_none(), "the data marks fallout unremovable");
}

/// Workers cannot clear fallout (a gap against Civ V, where workers remove
/// it): the removal order is refused. Units standing in fallout take no
/// damage at the end of the turn.
#[test]
fn workers_cannot_clear_fallout_and_units_in_it_are_not_hurt() {
    let rules = rules();
    let mut game = world(&rules, 2, 16, 12, 1);
    let home = city(&mut game, &rules, P0, 4, 5);
    let _ = home;
    let spot = tile(&game, 5, 5);
    game.debug_set_feature_batch(&rules, &[(spot, fallout(&rules))]);
    let worker = spawn(&mut game, &rules, P0, 5, 5, "core:unit/worker");
    let warrior = spawn(&mut game, &rules, P0, 5, 5, "core:unit/warrior");
    // The control: the same order removes a forest once Mining is known.
    let wood = tile(&game, 3, 5);
    let forest = FeatureId(rules.features.iter().position(|f| f.key == "core:feature/forest").unwrap() as u16);
    game.debug_set_feature_batch(&rules, &[(wood, forest)]);
    game.debug_learn(&rules, P0, tech(&rules, "core:tech/mining"));
    let chopper = spawn(&mut game, &rules, P0, 3, 5, "core:unit/worker");
    accepted(&mut game, &rules, Command::ChopForest { unit: chopper });
    let outcome = run(&mut game, &rules, Command::ChopForest { unit: worker });
    assert_ne!(outcome, "ok", "the removal of fallout is refused: {outcome}");
    next_round_of(&mut game, &rules, P0);
    next_round_of(&mut game, &rules, P0);
    assert_eq!(hp(&game, warrior), 100, "no damage in fallout");
    assert_eq!(game.view().auth.map().feature(spot), Some(Some(fallout(&rules))), "fallout stays");
}

/// The order's rules: range, war with everybody hit (also a third player),
/// moves, a nuclear weapon at all, a known target.
#[test]
fn nuking_needs_range_war_with_all_victims_moves_and_a_known_tile() {
    let rules = rules();
    // Range of the atomic bomb: 10.
    for (key, range) in [("core:unit/atomic-bomb", 10), ("core:unit/nuclear-missile", 12)] {
        assert_eq!(i32::from(rules.units[kind(&rules, key).0 as usize].range), range);
        let (mut game, bomb, _) = nuke_world(&rules, 1, key);
        let ok = tile(&game, BASE_Q + range, 10);
        let far = tile(&game, BASE_Q + 1 + range, 10);
        spawn(&mut game, &rules, P0, BASE_Q + range, 11, "core:unit/scout");
        refused(&mut game, &rules, Command::Nuke { unit: bomb, target: far }, "out of range");
        accepted(&mut game, &rules, Command::Nuke { unit: bomb, target: ok });
    }
    // Not at war with a victim: a unit of P2 within the blast stops it; so
    // does land of a player at peace.
    let (mut game, bomb, centre) = nuke_world(&rules, 1, "core:unit/atomic-bomb");
    spawn(&mut game, &rules, P0, 14, 12, "core:unit/scout");
    let neutral = game.debug_spawn_unit(&rules, P2, ring_tiles(&game, centre, 2)[0], kind(&rules, "core:unit/warrior"));
    game.debug_spawn_unit(&rules, P1, centre, kind(&rules, "core:unit/warrior"));
    refused(&mut game, &rules, Command::Nuke { unit: bomb, target: centre }, "not at war");
    game.debug_meet(P0, P2);
    refused(&mut game, &rules, Command::Nuke { unit: bomb, target: centre }, "not at war");
    // At war with P2 as well: it goes.
    game.debug_force_war(&rules, P0, P2);
    accepted(&mut game, &rules, Command::Nuke { unit: bomb, target: centre });
    assert!(hp(&game, neutral) < 100, "the third player was hit once at war");
    // A bomber is no nuclear weapon; a spent unit (no moves) cannot launch.
    let (mut game, _, centre) = nuke_world(&rules, 1, "core:unit/atomic-bomb");
    let bomber = spawn(&mut game, &rules, P0, BASE_Q, 10, "core:unit/bomber");
    refused(&mut game, &rules, Command::Nuke { unit: bomber, target: centre }, "not a nuclear weapon");
    let missile = spawn(&mut game, &rules, P0, BASE_Q, 10, "core:unit/nuclear-missile");
    let other = city(&mut game, &rules, P0, 8, 10);
    accepted(&mut game, &rules, Command::Rebase { unit: missile, city: other });
    refused(&mut game, &rules, Command::Nuke { unit: missile, target: centre }, "no moves left");
    // Own units in the blast are not protected.
    let (mut game, bomb, centre) = nuke_world(&rules, 1, "core:unit/atomic-bomb");
    let own = game.debug_spawn_unit(&rules, P0, centre, kind(&rules, "core:unit/warrior"));
    spawn(&mut game, &rules, P0, 14, 12, "core:unit/scout");
    game.debug_spawn_unit(&rules, P1, ring_tiles(&game, centre, 2)[0], kind(&rules, "core:unit/warrior"));
    accepted(&mut game, &rules, Command::Nuke { unit: bomb, target: centre });
    assert!(!alive(&game, own), "friendly fire");
}

/// A nuclear strike needs a target tile the launching player knows; the
/// refusal must not depend on what stands on tiles it has never seen.
#[test]
fn a_nuclear_strike_cannot_target_an_unexplored_tile() {
    let rules = rules();
    let (mut game, bomb, centre) = nuke_world_with(&rules, 1, "core:unit/atomic-bomb", false);
    assert!(!game.player_view(&rules, P0).is_known(centre));
    refused(&mut game, &rules, Command::Nuke { unit: bomb, target: centre }, "unknown");
    game.debug_reveal(P0, &[centre]);
    accepted(&mut game, &rules, Command::Nuke { unit: bomb, target: centre });
}

/// The Manhattan Project gates building a nuclear weapon.
#[test]
fn nuclear_weapons_need_the_manhattan_project() {
    use classic_rules::ids::ProductionItem;
    let rules = rules();
    let project = classic_rules::rules::content::ProjectId(
        rules.module.projects.iter().position(|p| p.key == "core:project/manhattan_project").unwrap() as u16,
    );
    for (key, tech_key) in [("core:unit/atomic-bomb", "core:tech/nuclear-fission"), ("core:unit/nuclear-missile", "core:tech/advanced-ballistics")] {
        let mut game = world(&rules, 2, 16, 12, 1);
        let c = city(&mut game, &rules, P0, 4, 5);
        game.debug_learn(&rules, P0, tech(&rules, tech_key));
        let item = ProductionItem::Unit(kind(&rules, key));
        let enqueue = Command::EnqueueProduction { player: P0, city: c, item };
        let without = run(&mut game, &rules, enqueue.clone());
        assert!(without.contains("Manhattan") || without.contains("project"), "{key} without the project: {without}");
        game.debug_add_project(P0, project);
        let with = run(&mut game, &rules, enqueue);
        assert!(!with.contains("Manhattan") && !with.contains("project"), "{key} with the project: {with}");
        // Another player's project does not arm P1.
        let other = city(&mut game, &rules, P1, 10, 5);
        game.debug_learn(&rules, P1, tech(&rules, tech_key));
        to_player(&mut game, &rules, P1);
        let theirs = run(&mut game, &rules, Command::EnqueueProduction { player: P1, city: other, item });
        assert!(theirs.contains("Manhattan") || theirs.contains("project"), "{key}, P1 without its own project: {theirs}");
    }
}

// ============================================================================
// 7. Airlift and amphibious marines
// ============================================================================

#[test]
fn airlift_needs_airports_in_both_cities_ignores_distance_and_is_once_a_turn() {
    let rules = rules();
    let mut game = world(&rules, 2, 64, 12, 1);
    let home = city(&mut game, &rules, P0, 2, 5);
    let far = city(&mut game, &rules, P0, 60, 8);
    let enemy = city(&mut game, &rules, P1, 40, 5);
    let airport = building(&rules, "core:building/airport");
    let warrior = spawn(&mut game, &rules, P0, 2, 5, "core:unit/warrior");
    let fighter = spawn(&mut game, &rules, P0, 2, 5, "core:unit/fighter");
    refused(&mut game, &rules, Command::Airlift { unit: warrior, city: far }, "airport");
    game.debug_add_building(&rules, home, airport);
    refused(&mut game, &rules, Command::Airlift { unit: warrior, city: far }, "airport");
    refused(&mut game, &rules, Command::Airlift { unit: warrior, city: home }, "airport");
    game.debug_add_building(&rules, enemy, airport);
    refused(&mut game, &rules, Command::Airlift { unit: warrior, city: enemy }, "airport");
    refused(&mut game, &rules, Command::Airlift { unit: fighter, city: far }, "land units");
    game.debug_add_building(&rules, far, airport);
    // Distance 58: no range limit.
    let occupant = spawn(&mut game, &rules, P0, 60, 8, "core:unit/spearman");
    refused(&mut game, &rules, Command::Airlift { unit: warrior, city: far }, "cannot stop");
    // A civilian takes another class and goes through.
    let worker = spawn(&mut game, &rules, P0, 2, 5, "core:unit/worker");
    accepted(&mut game, &rules, Command::Airlift { unit: worker, city: far });
    assert_eq!(core(&game, worker).tile, tile(&game, 60, 8));
    assert_eq!(core(&game, worker).moves_left, 0);
    refused(&mut game, &rules, Command::Airlift { unit: worker, city: home }, "moved");
    // A unit that moved is refused; the next round it may fly.
    let walker = spawn(&mut game, &rules, P0, 2, 5, "core:unit/settler");
    let step = tile(&game, 3, 5);
    accepted(&mut game, &rules, Command::MoveUnit { unit: walker, destination: step });
    refused(&mut game, &rules, Command::Airlift { unit: walker, city: far }, "moved");
    let _ = occupant;
    next_round_of(&mut game, &rules, P0);
    accepted(&mut game, &rules, Command::Airlift { unit: worker, city: home });
    assert_eq!(core(&game, worker).tile, tile(&game, 2, 5));
    game.validate(&rules).unwrap();
}

/// A marine attacks from a boat at full strength, a spearman at -50 %.
#[test]
fn a_marine_attacks_from_the_water_without_the_landing_penalty() {
    use classic_rules::combat::exchange::AttackKind;
    use classic_rules::combat::plan_attack;
    let rules = rules();
    let mut game = world(&rules, 2, 20, 12, 7);
    sea(&mut game, &rules, 6, 12);
    game.debug_learn(&rules, P0, tech(&rules, "core:tech/optics"));
    let marine = spawn(&mut game, &rules, P0, 6, 5, "core:unit/marine");
    let spearman = spawn(&mut game, &rules, P0, 6, 6, "core:unit/spearman");
    let a = tile(&game, 5, 5);
    let b = tile(&game, 5, 7);
    spawn(&mut game, &rules, P1, 5, 5, "core:unit/warrior");
    spawn(&mut game, &rules, P1, 5, 7, "core:unit/warrior");
    war(&mut game, &rules, P0, P1);
    let marine_plan = plan_attack(game.view(), &rules, marine, a, AttackKind::Melee).unwrap();
    let spear_plan = plan_attack(game.view(), &rules, spearman, b, AttackKind::Melee).unwrap();
    let strength = |k| i64::from(rules.units[kind(&rules, k).0 as usize].strength);
    assert_eq!(marine_plan.attacker_strength, strength("core:unit/marine") * 100, "amphibious: no landing penalty");
    assert_eq!(spear_plan.attacker_strength, strength("core:unit/spearman") * 50, "a landing costs 50 %");
}

// ============================================================================
// 8. Paradrop (airborne landing)
// ============================================================================

/// A wide plain with P0's city at (6, 5) and a paratrooper in it; the tiles
/// around are revealed to P0.
fn paradrop_world(rules: &CompiledRuleset, seed: u64) -> (GameState, UnitHandle) {
    let mut game = world(rules, 2, 40, 12, seed);
    city(&mut game, rules, P0, 6, 5);
    let trooper = spawn(&mut game, rules, P0, 6, 5, "core:unit/paratrooper");
    let all: Vec<TileId> = (0..12).flat_map(|r| (0..40).map(move |q| (q, r))).map(|(q, r)| tile(&game, q, r)).collect();
    game.debug_reveal(P0, &all);
    (game, trooper)
}

#[test]
fn a_paratrooper_jumps_within_five_tiles_and_the_jump_ends_its_turn() {
    let rules = rules();
    let (mut game, trooper) = paradrop_world(&rules, 3);
    // (6, 10) is five rows away: a jump of 5 tiles, the whole range
    // (Unciv: Units.json, Paratrooper, paradrop up to 5 tiles).
    let to = tile(&game, 6, 10);
    let from = core(&game, trooper).tile;
    let outcome = game.execute(&rules, Command::Paradrop { unit: trooper, destination: to }).unwrap();
    let c = core(&game, trooper);
    assert_eq!(c.tile, to);
    assert_eq!(c.moves_left, 0, "the jump spends the whole turn");
    let moved: Vec<_> = outcome
        .changes()
        .unit_changes
        .iter()
        .filter_map(|ch| match *ch {
            UnitChange::Moved { unit, from: f, to: t } if unit == trooper => Some((f, t)),
            _ => None,
        })
        .collect();
    assert_eq!(moved, vec![(from, to)], "one journal step, from the city to the landing");
    game.validate(&rules).unwrap();
    // Landed in the open: nothing to jump from in the next round.
    next_round_of(&mut game, &rules, P0);
    let again = tile(&game, 6, 9);
    refused(&mut game, &rules, Command::Paradrop { unit: trooper, destination: again }, "own city or on an own fort");
}

#[test]
fn the_range_is_five_tiles_and_the_extended_promotion_reaches_further() {
    let rules = rules();
    let (mut game, trooper) = paradrop_world(&rules, 3);
    // Distance 6 along a row is out of range, 5 is not (Unciv: Units.json,
    // Paratrooper, paradrop up to 5 tiles).
    let far = tile(&game, 12, 5);
    let near = tile(&game, 11, 5);
    refused(&mut game, &rules, Command::Paradrop { unit: trooper, destination: far }, "beyond");
    accepted(&mut game, &rules, Command::Paradrop { unit: trooper, destination: near });
    assert_eq!(core(&game, trooper).tile, near);
    // The XCOM squad (extended paradrop) crosses the whole map.
    let (mut game, _) = paradrop_world(&rules, 3);
    let squad = spawn(&mut game, &rules, P0, 6, 5, "core:unit/xcom-squad");
    let dest = tile(&game, 36, 5);
    accepted(&mut game, &rules, Command::Paradrop { unit: squad, destination: dest });
    assert_eq!(core(&game, squad).tile, dest);
}

#[test]
fn only_paratroopers_jump_and_only_from_a_base() {
    let rules = rules();
    let (mut game, trooper) = paradrop_world(&rules, 3);
    let warrior = spawn(&mut game, &rules, P0, 6, 5, "core:unit/warrior");
    let dest = tile(&game, 8, 5);
    refused(&mut game, &rules, Command::Paradrop { unit: warrior, destination: dest }, "cannot paradrop");
    // Out in the field the trooper has no base.
    let field = spawn(&mut game, &rules, P0, 20, 5, "core:unit/paratrooper");
    let next = tile(&game, 22, 5);
    refused(&mut game, &rules, Command::Paradrop { unit: field, destination: next }, "own city or on an own fort");
    // An own fort outside the borders is a base.
    let fort = ImprovementId(rules.improvements.iter().position(|i| i.key == "core:improvement/fort").unwrap() as u16);
    let at = tile(&game, 20, 5);
    game.debug_set_improvement_batch(&rules, &[(at, fort)]);
    accepted(&mut game, &rules, Command::Paradrop { unit: field, destination: next });
    assert_eq!(core(&game, field).tile, next);
    // The one in the city can still go the same way.
    accepted(&mut game, &rules, Command::Paradrop { unit: trooper, destination: dest });
    game.validate(&rules).unwrap();
}

#[test]
fn a_unit_that_moved_or_attacked_this_turn_does_not_jump() {
    let rules = rules();
    let (mut game, trooper) = paradrop_world(&rules, 3);
    let step = tile(&game, 7, 5);
    accepted(&mut game, &rules, Command::MoveUnit { unit: trooper, destination: step });
    let back = tile(&game, 6, 5);
    accepted(&mut game, &rules, Command::MoveUnit { unit: trooper, destination: back });
    let dest = tile(&game, 9, 5);
    refused(&mut game, &rules, Command::Paradrop { unit: trooper, destination: dest }, "moved");
    next_round_of(&mut game, &rules, P0);
    accepted(&mut game, &rules, Command::Paradrop { unit: trooper, destination: dest });
}

#[test]
fn a_jump_needs_known_dry_free_ground() {
    let rules = rules();
    let mut game = world(&rules, 2, 40, 12, 3);
    city(&mut game, &rules, P0, 6, 5);
    let trooper = spawn(&mut game, &rules, P0, 6, 5, "core:unit/paratrooper");
    // Never seen by P0: refused (the tile is outside the city's sight, within
    // the range of 5).
    let unseen = tile(&game, 11, 5);
    refused(&mut game, &rules, Command::Paradrop { unit: trooper, destination: unseen }, "unknown");
    let all: Vec<TileId> = (0..12).flat_map(|r| (0..40).map(move |q| (q, r))).map(|(q, r)| tile(&game, q, r)).collect();
    game.debug_reveal(P0, &all);
    let here = tile(&game, 6, 5);
    refused(&mut game, &rules, Command::Paradrop { unit: trooper, destination: here }, "already there");
    // Water.
    let lake = tile(&game, 9, 5);
    game.debug_set_terrain_batch(&rules, &[(lake, WATER)]);
    refused(&mut game, &rules, Command::Paradrop { unit: trooper, destination: lake }, "cannot land");
    // Mountains stop a jump no more than a walk: they are land (Civ V: any passable land).
    // A foreign unit holds a tile.
    spawn(&mut game, &rules, P1, 10, 5, "core:unit/warrior");
    let held = tile(&game, 10, 5);
    refused(&mut game, &rules, Command::Paradrop { unit: trooper, destination: held }, "another player");
    // A foreign city holds a tile (at war the borders are open, the city still holds it).
    war(&mut game, &rules, P0, P1);
    // Within the range of 5 (Unciv: Units.json, Paratrooper).
    city(&mut game, &rules, P1, 5, 10);
    let town = tile(&game, 5, 10);
    refused(&mut game, &rules, Command::Paradrop { unit: trooper, destination: town }, "another player");
    // An own unit of the same class is in the way.
    spawn(&mut game, &rules, P0, 8, 5, "core:unit/spearman");
    let crowded = tile(&game, 8, 5);
    refused(&mut game, &rules, Command::Paradrop { unit: trooper, destination: crowded }, "cannot stop");
    // The free tile next to all of that works.
    let free = tile(&game, 9, 6);
    accepted(&mut game, &rules, Command::Paradrop { unit: trooper, destination: free });
    game.validate(&rules).unwrap();
}

#[test]
fn a_jump_into_closed_foreign_land_is_refused_and_into_a_war_enemys_is_not() {
    let rules = rules();
    let (mut game, trooper) = paradrop_world(&rules, 3);
    // Within the range of 5 (Unciv: Units.json, Paratrooper).
    let their = city(&mut game, &rules, P1, 10, 5);
    let _ = their;
    let inside = tile(&game, 11, 5);
    game.debug_meet(P0, P1);
    let owner = game.view().auth.map().owner(inside).flatten();
    assert_eq!(owner, Some(P1), "the tile next to the city is P1's");
    refused(&mut game, &rules, Command::Paradrop { unit: trooper, destination: inside }, "closed");
    war(&mut game, &rules, P0, P1);
    accepted(&mut game, &rules, Command::Paradrop { unit: trooper, destination: inside });
    assert_eq!(core(&game, trooper).tile, inside);
}

#[test]
fn a_paratrooper_cannot_be_ordered_by_the_wrong_player() {
    let rules = rules();
    let (mut game, trooper) = paradrop_world(&rules, 3);
    let dest = tile(&game, 8, 5);
    let outcome = game.execute_as(P1, &rules, Command::Paradrop { unit: trooper, destination: dest });
    assert!(outcome.is_err(), "P1 may not order P0's unit");
    assert_eq!(core(&game, trooper).tile, tile(&game, 6, 5));
}

/// The landed trooper survives a save: bytes, hash and the next round.
#[test]
fn a_world_after_a_paradrop_round_trips_through_a_save() {
    use classic_rules::persistence::{decode::decode, encode::encode};
    use classic_rules::replay::hash::authoritative_hash;
    use classic_rules::save_codec::{from_bytes, to_bytes};
    let rules = rules();
    let (mut game, trooper) = paradrop_world(&rules, 3);
    let dest = tile(&game, 9, 7);
    accepted(&mut game, &rules, Command::Paradrop { unit: trooper, destination: dest });
    let bytes = to_bytes(&encode(&game, &rules));
    let loaded = decode(&from_bytes(&bytes).unwrap(), &rules).unwrap();
    assert_eq!(authoritative_hash(&loaded), authoritative_hash(&game));
    assert_eq!(to_bytes(&encode(&loaded, &rules)), bytes);
}
