//! Effects next to combat on the classic ruleset: yields from kills (Honor,
//! the Aztecs, God of War), triggered policy effects (Total War), beliefs
//! near following cities (Defender of the Faith, Just War, Faith Healers),
//! timed bonuses (Clausewitz's Legacy), plunder (Songhai, Burial Tomb),
//! movement (Great Wall, airlift).

use classic_rules::classic::ClassicChange;
use classic_rules::rules::effects::{SourceRef, YieldKind};
use classic_rules::classic::effects::{ClassicSource, ClassicYield};
use classic_rules::ClassicGame;
use classic_content::core::core_rules;
use classic_rules::combat::exchange::AttackKind;
use classic_rules::combat::plan_attack;
use classic_rules::derived::religion::city_majority;
use classic_rules::navigation::cost::{step_cost_for, StepCost};
use classic_rules::navigation::MoveProfile;
use classic_rules::persistence::{decode::decode, encode::encode};
use classic_rules::replay::authoritative_hash;
use classic_rules::rules::content::ReligionId;
use classic_rules::world::religion::Creed;
use classic_rules::ids::{BeliefId, BuildingKindId, CityHandle, PlayerId, PolicyId, ProductionItem, TileId, UnitHandle, UnitKindId};
use classic_rules::math::HexCoord;
use classic_rules::world::player::PlayerKind;
use classic_rules::{compile_rules, Command, CompiledRuleset, GameConfig, GameState};
use classic_rules::world::player::ClassicKind;
use classic_rules::world::player::ClassicPlayers;

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);
const BARBARIAN: PlayerId = PlayerId(2);

fn rules() -> CompiledRuleset {
    compile_rules(core_rules()).unwrap()
}

fn world(rules: &CompiledRuleset) -> GameState {
    let mut game = GameState::new(rules, GameConfig { players: 3, width: 24, height: 16, rng_seed: 5 });
    game.debug_set_player_kind(BARBARIAN, PlayerKind::Module(ClassicKind::Barbarian));
    game
}

fn tile(game: &GameState, q: i32, r: i32) -> TileId {
    game.view().auth.map().dimensions.to_tile(HexCoord { q, r }).unwrap()
}

fn unit(rules: &CompiledRuleset, key: &str) -> UnitKindId {
    UnitKindId(rules.units.iter().position(|u| u.key == key).unwrap_or_else(|| panic!("{key}")) as u16)
}

fn policy(rules: &CompiledRuleset, key: &str) -> PolicyId {
    PolicyId(rules.module.policies.iter().position(|p| p.key == key).unwrap_or_else(|| panic!("{key}")) as u16)
}

fn belief(rules: &CompiledRuleset, key: &str) -> BeliefId {
    BeliefId(rules.module.beliefs.iter().position(|b| b.key == key).unwrap_or_else(|| panic!("{key}")) as u16)
}

fn culture(game: &GameState) -> u32 {
    game.view().auth.module().culture().of(P0).culture
}

fn gold(game: &GameState) -> u32 {
    game.view().auth.players().treasury_of(P0)
}

fn faith(game: &GameState) -> u32 {
    game.view().auth.module().religion().player(P0).faith
}

/// A warrior of P0 at (q, r) kills a wounded `victim_key` of `enemy` on
/// the next tile east (one strike: the victim has 1 HP).
fn kill(game: &mut GameState, rules: &CompiledRuleset, enemy: PlayerId, victim_key: &str, q: i32, r: i32) -> Vec<ClassicChange> {
    let killer = game.debug_spawn_unit(rules, P0, tile(game, q, r), unit(rules, "core:unit/warrior"));
    let victim = game.debug_spawn_unit(rules, enemy, tile(game, q + 1, r), unit(rules, victim_key));
    game.debug_set_unit_hp(victim, 1);
    if !game.view().auth.players().is_barbarian(enemy) && !game.view().auth.diplomacy().at_war(P0, enemy) {
        game.debug_meet(P0, enemy);
        game.debug_force_war(rules, P0, enemy);
    }
    let result = game.execute(rules, Command::AttackUnit { attacker: killer, defender: victim }).unwrap();
    assert!(game.view().auth.units().get(victim).is_none(), "the victim dies");
    game.validate(rules).unwrap();
    result.changes().module_changes.clone()
}

/// The yields of kills in `changes`: (kind, amount, source, victim a barbarian).
fn kill_yields(changes: &[ClassicChange]) -> Vec<(YieldKind, u32, SourceRef, bool)> {
    changes
        .iter()
        .filter_map(|c| match *c {
            ClassicChange::KillYield { player, kind, amount, source, barbarian, .. } if player == P0 => Some((kind, amount, source, barbarian)),
            _ => None,
        })
        .collect()
}

#[test]
fn honor_opener_gives_culture_for_a_barbarian_kill_only() {
    let rules = rules();
    let mut game = world(&rules);
    game.debug_adopt_policy(&rules, P0, policy(&rules, "core:policy/honor/opener"));
    let before = culture(&game);
    let opener = policy(&rules, "core:policy/honor/opener");
    let changes = kill(&mut game, &rules, BARBARIAN, "core:unit/spearman", 4, 4);
    assert_eq!(culture(&game) - before, 11, "100 % of a spearman's 11");
    assert_eq!(
        kill_yields(&changes),
        [(YieldKind::Culture, 11, SourceRef::Module(ClassicSource::Policy(opener)), true)],
        "the journal names the culture the kill paid, its source and the barbarian"
    );
    let spot = changes.iter().find_map(|c| match c {
        ClassicChange::KillYield { tile, .. } => Some(*tile),
        _ => None,
    });
    assert_eq!(spot, Some(tile(&game, 5, 4)), "on the tile of the kill");
    let before = culture(&game);
    let changes = kill(&mut game, &rules, P1, "core:unit/spearman", 4, 8);
    assert_eq!(culture(&game), before, "not for a nation's unit");
    assert!(kill_yields(&changes).is_empty(), "and nothing to tell");
}

#[test]
fn honor_finisher_gives_gold_for_every_kill() {
    let rules = rules();
    let mut game = world(&rules);
    game.debug_adopt_policy(&rules, P0, policy(&rules, "core:policy/honor/finisher"));
    let before = gold(&game);
    let changes = kill(&mut game, &rules, P1, "core:unit/archer", 4, 4);
    // An archer: melee 5, ranged 7 — the greater counts.
    assert_eq!(gold(&game) - before, 7);
    let finisher = policy(&rules, "core:policy/honor/finisher");
    assert_eq!(kill_yields(&changes), [(YieldKind::Gold, 7, SourceRef::Module(ClassicSource::Policy(finisher)), false)]);
}

#[test]
fn aztecs_gain_culture_for_every_kill() {
    let rules = rules();
    let mut game = world(&rules);
    let aztecs = rules.nations.iter().position(|c| c.key == "core:nation/aztecs").unwrap();
    game.debug_set_player_nation(P0, aztecs as u8);
    let before = culture(&game);
    let changes = kill(&mut game, &rules, P1, "core:unit/warrior", 4, 4);
    assert_eq!(culture(&game) - before, 8);
    assert_eq!(kill_yields(&changes), [(YieldKind::Culture, 8, SourceRef::Nation(aztecs as u8), false)]);
}

/// God of War: 50 % of the victim's strength as faith within 4 tiles of
/// the killer's nearest city following the pantheon.
#[test]
fn god_of_war_gives_faith_for_a_kill_near_a_following_city() {
    let rules = rules();
    let mut game = world(&rules);
    let city: CityHandle = game.debug_found_city(&rules, P0, tile(&game, 2, 4));
    game.debug_grant_faith(P0, 100);
    let god_of_war = belief(&rules, "core:belief/pantheon/god-of-war");
    game.execute(&rules, Command::FoundPantheon { player: P0, belief: god_of_war }).unwrap();
    let _ = city;
    let before = faith(&game);
    // The victim stands on (5, 4): three tiles from the city.
    let changes = kill(&mut game, &rules, P1, "core:unit/spearman", 4, 4);
    assert_eq!(faith(&game) - before, 5, "50 % of 11, rounded down");
    assert_eq!(
        kill_yields(&changes),
        [(YieldKind::Module(ClassicYield::Faith), 5, SourceRef::Module(ClassicSource::Belief(god_of_war)), false)]
    );
    let before = faith(&game);
    // (13, 4): eleven tiles away.
    let changes = kill(&mut game, &rules, P1, "core:unit/spearman", 12, 4);
    assert_eq!(faith(&game), before, "too far from the city");
    assert!(kill_yields(&changes).is_empty());
}

/// Regression: triggered effects of adopted policies fire (Total War's
/// free experience reached no unit: policies were not trigger sources).
#[test]
fn total_war_gives_trained_units_twelve_experience() {
    let rules = rules();
    let mut game = world(&rules);
    let city = game.debug_found_city(&rules, P0, tile(&game, 6, 6));
    game.debug_adopt_policy(&rules, P0, policy(&rules, "core:policy/autocracy/total-war"));
    let warrior = unit(&rules, "core:unit/warrior");
    game.execute(&rules, Command::EnqueueProduction { player: P0, city, item: ProductionItem::Unit(warrior) }).unwrap();
    game.debug_earn(P0, 1000);
    game.execute(&rules, Command::HurryProduction { player: P0, city }).unwrap();
    let trained: Vec<UnitHandle> = game.view().auth.indexes().units_of(P0).to_vec();
    assert_eq!(trained.len(), 1);
    let xp = game.view().auth.units().get(trained[0]).unwrap().core.experience;
    assert_eq!(xp, 12);
    game.validate(&rules).unwrap();
}

// ---- beliefs near following cities ----

/// P0's holy city at (5, 5) with a religion founded with `pantheon`, then
/// enhanced with `enhancer`.
fn religion(game: &mut GameState, rules: &CompiledRuleset, pantheon: &str, enhancer: &str) -> CityHandle {
    let holy = game.debug_found_city(rules, P0, tile(game, 5, 5));
    let prophet = unit(rules, "core:unit/great-prophet");
    let first = game.debug_spawn_unit(rules, P0, tile(game, 5, 5), prophet);
    let found = Command::FoundReligion {
        unit: first,
        religion: ReligionId(0),
        founder: belief(rules, "core:belief/founder/tithe"),
        follower: belief(rules, "core:belief/follower/divine-inspiration"),
        pantheon: Some(belief(rules, pantheon)),
        bonus: None,
    };
    game.execute(rules, found).unwrap();
    let second = game.debug_spawn_unit(rules, P0, tile(game, 5, 5), prophet);
    let enhance = Command::EnhanceReligion {
        unit: second,
        follower: belief(rules, "core:belief/follower/cathedrals"),
        enhancer: belief(rules, enhancer),
    };
    game.execute(rules, enhance).unwrap();
    assert_eq!(city_majority(game.view().auth, holy), Some(Creed::Religion(ReligionId(0))));
    holy
}

fn attack_strength(game: &GameState, rules: &CompiledRuleset, attacker: UnitHandle, target: TileId) -> i64 {
    plan_attack(game.view(), rules, attacker, target, AttackKind::Melee).unwrap().attacker_strength
}

/// Defender of the Faith: +20 % in the territory of an own city following
/// the founded religion (the battle tile is the defender's).
#[test]
fn defender_of_the_faith_strengthens_units_near_own_following_cities() {
    let rules = rules();
    let mut game = world(&rules);
    let warrior = unit(&rules, "core:unit/warrior");
    let near = tile(&game, 6, 5);
    let attacker = game.debug_spawn_unit(&rules, P0, tile(&game, 7, 5), warrior);
    game.debug_spawn_unit(&rules, P1, near, warrior);
    game.debug_meet(P0, P1);
    game.debug_force_war(&rules, P0, P1);
    // Plain (pantheon only, no religion yet): the city's tile is P0's.
    religion(&mut game, &rules, "core:belief/pantheon/god-of-war", "core:belief/enhancer/defender-of-the-faith");
    let with = attack_strength(&game, &rules, attacker, near);
    let far = tile(&game, 15, 5);
    let lonely = game.debug_spawn_unit(&rules, P0, tile(&game, 16, 5), warrior);
    game.debug_spawn_unit(&rules, P1, far, warrior);
    let without = attack_strength(&game, &rules, lonely, far);
    assert_eq!(with - without, 8 * 20, "+20 % of a warrior's 8");
    game.validate(&rules).unwrap();
}

/// Just War: +20 % near an enemy city whose majority follows the
/// religion the unit's owner founded.
#[test]
fn just_war_strengthens_units_near_enemy_cities_following_the_religion() {
    let rules = rules();
    let mut game = world(&rules);
    religion(&mut game, &rules, "core:belief/pantheon/god-of-war", "core:belief/enhancer/just-war");
    let enemy_city = game.debug_found_city(&rules, P1, tile(&game, 12, 5));
    let missionary = unit(&rules, "core:unit/missionary");
    let preacher = game.debug_spawn_unit(&rules, P0, tile(&game, 11, 5), missionary);
    game.execute(&rules, Command::SpreadReligion { unit: preacher }).unwrap();
    assert_eq!(city_majority(game.view().auth, enemy_city), Some(Creed::Religion(ReligionId(0))));
    game.debug_meet(P0, P1);
    game.debug_force_war(&rules, P0, P1);
    let warrior = unit(&rules, "core:unit/warrior");
    let near = tile(&game, 12, 6);
    let attacker = game.debug_spawn_unit(&rules, P0, tile(&game, 11, 6), warrior);
    game.debug_spawn_unit(&rules, P1, near, warrior);
    let far = tile(&game, 20, 12);
    let lonely = game.debug_spawn_unit(&rules, P0, tile(&game, 19, 12), warrior);
    game.debug_spawn_unit(&rules, P1, far, warrior);
    let with = attack_strength(&game, &rules, attacker, near);
    let without = attack_strength(&game, &rules, lonely, far);
    // Both fight outside friendly land; only the first near the city.
    assert_eq!(with - without, 8 * 20);
    game.validate(&rules).unwrap();
}

/// Faith Healers: +30 HP a turn next to an own city following the
/// pantheon (20 in friendly land + 30).
#[test]
fn faith_healers_heal_units_near_following_cities() {
    let rules = rules();
    let mut game = world(&rules);
    let city = game.debug_found_city(&rules, P0, tile(&game, 5, 5));
    game.debug_grant_faith(P0, 100);
    let healers = belief(&rules, "core:belief/pantheon/faith-healers");
    game.execute(&rules, Command::FoundPantheon { player: P0, belief: healers }).unwrap();
    assert_eq!(city_majority(game.view().auth, city), Some(Creed::Pantheon));
    let warrior = unit(&rules, "core:unit/warrior");
    let wounded = game.debug_spawn_unit(&rules, P0, tile(&game, 6, 5), warrior);
    game.debug_set_unit_hp(wounded, 10);
    game.execute(&rules, Command::EndTurn { player: P0 }).unwrap();
    let hp = game.view().auth.units().get(wounded).unwrap().core.hp;
    assert_eq!(hp, 10 + 20 + 30);
    game.validate(&rules).unwrap();
}

// ---- timed bonus on all units ----

/// Clausewitz's Legacy: +25 % attack for every unit of the owner, present
/// and future, for 50 turns after adoption; saved and ticked like any
/// timed effect.
#[test]
fn clausewitz_legacy_gives_every_unit_an_attack_bonus_for_fifty_turns() {
    let rules = rules();
    let mut game = world(&rules);
    let warrior = unit(&rules, "core:unit/warrior");
    let target = tile(&game, 5, 4);
    let attacker = game.debug_spawn_unit(&rules, P0, tile(&game, 4, 4), warrior);
    game.debug_spawn_unit(&rules, P1, target, warrior);
    game.debug_meet(P0, P1);
    game.debug_force_war(&rules, P0, P1);
    let before = attack_strength(&game, &rules, attacker, target);
    game.debug_adopt_policy(&rules, P0, policy(&rules, "core:policy/autocracy/clausewitz-s-legacy"));
    assert_eq!(attack_strength(&game, &rules, attacker, target) - before, 8 * 25);
    // A new unit, alone with its own target (no flanking).
    let other = tile(&game, 13, 8);
    let later = game.debug_spawn_unit(&rules, P0, tile(&game, 12, 8), warrior);
    game.debug_spawn_unit(&rules, P1, other, warrior);
    assert_eq!(attack_strength(&game, &rules, later, other) - before, 8 * 25, "new units too");
    let turns = |game: &GameState| game.view().auth.active_effects().iter().next().map(|e| e.turns_left);
    assert_eq!(turns(&game), Some(35));
    let loaded = decode(&encode(&game, &rules), &rules).unwrap();
    assert_eq!(authoritative_hash(&loaded), authoritative_hash(&game));
    assert_eq!(attack_strength(&loaded, &rules, attacker, target) - before, 8 * 25);
    game.execute(&rules, Command::EndTurn { player: P0 }).unwrap();
    assert_eq!(turns(&game), Some(34));
    game.validate(&rules).unwrap();
}

// ---- plunder ----

/// Ends turns until `player` is active.
fn until_active(game: &mut GameState, rules: &CompiledRuleset, player: PlayerId) {
    for _ in 0..8 {
        let active = game.view().auth.active_player();
        if active == player {
            return;
        }
        game.execute(rules, Command::EndTurn { player: active }).unwrap();
    }
    panic!("{player:?} never became active");
}

/// Who takes the city and what it holds when it falls.
#[derive(Clone, Copy)]
struct Siege {
    rounds_held: u32,
    burial_tomb: bool,
    songhai: bool,
}

/// P1's city at (8, 4) falls to a warrior of P0 after `rounds_held`
/// rounds; the gold P0 takes. The same seed and the same turns every
/// time: only the plunder percents of the siege differ.
fn capture_gold(siege: Siege) -> u32 {
    capture(siege).0
}

/// `capture_gold` and the plunder the journal named: (the gold P0 took, the gold of its `Plundered`
/// changes of the city).
fn capture(siege: Siege) -> (u32, Vec<u32>) {
    let rules = rules();
    let mut game = world(&rules);
    let city = game.debug_found_city(&rules, P1, tile(&game, 8, 4));
    // P0 lives on elsewhere (a player without cities is eliminated).
    game.debug_found_city(&rules, P0, tile(&game, 18, 12));
    for _ in 0..siege.rounds_held {
        until_active(&mut game, &rules, P1);
        until_active(&mut game, &rules, P0);
    }
    let warrior = game.debug_spawn_unit(&rules, P0, tile(&game, 7, 4), unit(&rules, "core:unit/warrior"));
    game.debug_meet(P0, P1);
    game.debug_force_war(&rules, P0, P1);
    game.debug_set_city_damage(city, 10_000);
    if siege.burial_tomb {
        let tomb = rules.buildings.iter().position(|b| b.key == "core:building/burial_tomb").unwrap();
        game.debug_add_building(&rules, city, BuildingKindId(tomb as u16));
    }
    if siege.songhai {
        let songhai = rules.nations.iter().position(|c| c.key == "core:nation/songhai").unwrap();
        game.debug_set_player_nation(P0, songhai as u8);
    }
    let before = gold(&game);
    let result = game.execute(&rules, Command::AttackCity { attacker: warrior, city }).unwrap();
    let city_tile = game.view().auth.cities().get(city).unwrap().tile;
    let told: Vec<u32> = result
        .changes()
        .module_changes
        .iter()
        .filter_map(|c| match *c {
            ClassicChange::Plundered { player, tile, source, gold } if player == P0 && tile == city_tile && source == classic_rules::PlunderSource::City => Some(gold),
            _ => None,
        })
        .collect();
    assert_eq!(game.view().auth.cities().get(city).unwrap().owner, P0, "captured");
    game.validate(&rules).unwrap();
    let loaded = decode(&encode(&game, &rules), &rules).unwrap();
    assert_eq!(authoritative_hash(&loaded), authoritative_hash(&game), "acquired round saved");
    (gold(&game) - before, told)
}

/// Capture gold: 20 + 15 per citizen + rolls, by the
/// share of 50 rounds the old owner held the city.
#[test]
fn a_captured_city_gives_gold_by_the_time_it_was_held() {
    let plain = Siege { rounds_held: 0, burial_tomb: false, songhai: false };
    assert_eq!(capture(plain), (0, vec![]), "taken the round it was founded: nothing to tell");
    let held = capture_gold(Siege { rounds_held: 55, ..plain });
    assert!((20 + 15..=20 + 15 * 4 + 24 + 49).contains(&held), "{held}");
}

/// Burial Tomb: its captor takes double gold; Songhai triple.
#[test]
fn burial_tomb_doubles_and_songhai_triples_the_capture_gold() {
    let plain = Siege { rounds_held: 55, burial_tomb: false, songhai: false };
    let base = capture_gold(plain);
    assert!(base > 0);
    assert_eq!(capture_gold(Siege { burial_tomb: true, ..plain }), 2 * base);
    assert_eq!(capture_gold(Siege { songhai: true, ..plain }), 3 * base);
    assert_eq!(capture_gold(Siege { burial_tomb: true, songhai: true, ..plain }), 6 * base);
    // The journal names the sum after the percents, once.
    for siege in [plain, Siege { songhai: true, ..plain }, Siege { burial_tomb: true, songhai: true, ..plain }] {
        let (taken, told) = capture(siege);
        assert_eq!(told, [taken], "{taken}");
    }
}

/// Songhai: triple gold from a barbarian camp.
#[test]
fn songhai_take_triple_camp_gold() {
    let rules = rules();
    let mut game = world(&rules);
    let songhai = rules.nations.iter().position(|c| c.key == "core:nation/songhai").unwrap();
    game.debug_set_player_nation(P0, songhai as u8);
    game.debug_sources_changed(&rules, P0);
    let camp = tile(&game, 9, 9);
    game.debug_place_camp(&rules, camp);
    let warrior = game.debug_spawn_unit(&rules, P0, tile(&game, 8, 9), unit(&rules, "core:unit/warrior"));
    let before = gold(&game);
    let result = game.execute(&rules, Command::MoveUnit { unit: warrior, destination: camp }).unwrap();
    assert_eq!(gold(&game) - before, 3 * 25);
    let journaled = result.changes().module_changes.iter().find_map(|c| match c {
        classic_rules::classic::ClassicChange::CampCleared { gold, .. } => Some(*gold),
        _ => None,
    });
    assert_eq!(journaled, Some(3 * 25), "the journal names the tripled gold");
    game.validate(&rules).unwrap();
}

// ---- movement: Great Wall, airlift ----

fn building(rules: &CompiledRuleset, key: &str) -> BuildingKindId {
    BuildingKindId(rules.buildings.iter().position(|b| b.key == key).unwrap_or_else(|| panic!("{key}")) as u16)
}

/// The step price for `unit` from `from` to `to`.
fn step(game: &GameState, rules: &CompiledRuleset, unit: UnitHandle, from: TileId, to: TileId) -> Option<StepCost> {
    let auth = game.view().auth;
    let core = auth.units().get(unit).unwrap().core;
    let profile = MoveProfile::of_unit(auth, rules, core);
    step_cost_for(auth, rules, core.owner, &profile, from, to)
}

/// Great Wall: an enemy land unit pays one more point per tile of the
/// owner's territory, until the owner learns Dynamite.
#[test]
fn great_wall_slows_enemy_land_units_until_dynamite() {
    let rules = rules();
    let mut game = world(&rules);
    let city = game.debug_found_city(&rules, P1, tile(&game, 8, 4));
    game.debug_add_building(&rules, city, building(&rules, "core:building/great_wall"));
    let from = tile(&game, 6, 4);
    let into = tile(&game, 7, 4);
    let warrior = game.debug_spawn_unit(&rules, P0, from, unit(&rules, "core:unit/warrior"));
    assert_eq!(step(&game, &rules, warrior, from, into), None, "closed borders at peace");
    game.debug_meet(P0, P1);
    game.debug_force_war(&rules, P0, P1);
    assert_eq!(step(&game, &rules, warrior, from, into), Some(StepCost::Points(120)));
    let dynamite = rules.technologies.iter().position(|t| t.key == "core:tech/dynamite").unwrap();
    game.debug_learn(&rules, P1, classic_rules::ids::TechId(dynamite as u16));
    assert_eq!(step(&game, &rules, warrior, from, into), Some(StepCost::Points(60)), "obsolete");
}

/// Airport: a land unit that has not moved flies between two own cities
/// with airports and ends its turn there.
#[test]
fn airports_airlift_a_unit_once_a_turn() {
    let rules = rules();
    let mut game = world(&rules);
    let home = game.debug_found_city(&rules, P0, tile(&game, 4, 4));
    let far = game.debug_found_city(&rules, P0, tile(&game, 16, 10));
    let airport = building(&rules, "core:building/airport");
    game.debug_add_building(&rules, home, airport);
    let warrior = game.debug_spawn_unit(&rules, P0, tile(&game, 4, 4), unit(&rules, "core:unit/warrior"));
    let fly = Command::Airlift { unit: warrior, city: far };
    assert!(game.execute(&rules, fly.clone()).is_err(), "no airport at the destination");
    game.debug_add_building(&rules, far, airport);
    game.execute(&rules, fly).unwrap();
    let core = game.view().auth.units().get(warrior).unwrap().core;
    assert_eq!(core.tile, tile(&game, 16, 10));
    assert_eq!(core.moves_left, 0);
    let back = Command::Airlift { unit: warrior, city: home };
    assert!(game.execute(&rules, back).is_err(), "once a turn");
    game.validate(&rules).unwrap();
}
