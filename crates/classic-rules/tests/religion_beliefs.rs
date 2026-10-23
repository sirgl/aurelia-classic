//! Religion beliefs on the classic ruleset that were inert before workstream D
//! (classic/docs/classic/rules.md): two-condition follower beliefs,
//! river, peace and capital-connection conditions, the great person,
//! conversion and spread triggers, follower counts, Reformation rules, the
//! faith purchase rule of buildings, Byzantium's bonus belief and Religious
//! Tolerance. Each test checks the oracle (`GameState::validate`).

use classic_rules::ClassicGame;
use classic_rules::classic::effects::ClassicYield;
use classic_content::core::core_rules;
use classic_rules::derived::great_people::GreatPersonAction;
use classic_rules::derived::religion::{faith_purchase_status, FaithPurchaseBlock};
use classic_rules::effects::city::city_modifiers;
use classic_rules::effects::player::player_modifiers;
use classic_rules::ids::{
    BeliefId, BuildingKindId, CityHandle, PlayerId, PolicyId, ProductionItem, TileId, UnitHandle,
    UnitKindId,
};
use classic_rules::knowledge::observation::AIObservation;
use classic_rules::persistence::{decode::decode, encode::encode};
use classic_rules::replay::authoritative_hash;
use classic_rules::rules::content::ReligionId;
use classic_rules::rules::effects::YieldKind;
use classic_rules::world::player::PlayerKind;
use classic_rules::world::religion::{AdoptionBonus, Creed};
use classic_rules::{compile_rules, Command, CompiledRuleset, GameConfig, GameState};
use classic_rules::world::player::ClassicKind;

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

fn rules() -> CompiledRuleset {
    compile_rules(core_rules()).unwrap()
}

fn world(rules: &CompiledRuleset) -> GameState {
    GameState::new(rules, GameConfig { players: 2, width: 40, height: 24, rng_seed: 11 })
}

fn tile(game: &GameState, q: i32, r: i32) -> TileId {
    game.view().auth.map().dimensions.to_tile(classic_rules::math::HexCoord { q, r }).unwrap()
}

fn belief(rules: &CompiledRuleset, key: &str) -> BeliefId {
    BeliefId(rules.module.beliefs.iter().position(|b| b.key == key).unwrap_or_else(|| panic!("{key}")) as u16)
}

fn building(rules: &CompiledRuleset, key: &str) -> BuildingKindId {
    BuildingKindId(rules.buildings.iter().position(|b| b.key == key).unwrap_or_else(|| panic!("{key}")) as u16)
}

fn unit_kind(rules: &CompiledRuleset, key: &str) -> UnitKindId {
    UnitKindId(rules.units.iter().position(|u| u.key == key).unwrap_or_else(|| panic!("{key}")) as u16)
}

fn policy(rules: &CompiledRuleset, key: &str) -> PolicyId {
    PolicyId(rules.module.policies.iter().position(|p| p.key == key).unwrap() as u16)
}

fn civ(rules: &CompiledRuleset, key: &str) -> u8 {
    rules.civs.iter().position(|c| c.key == key).unwrap() as u8
}

fn city_tile(game: &GameState, city: CityHandle) -> TileId {
    game.view().auth.cities().get(city).unwrap().tile
}

fn spawn(game: &mut GameState, rules: &CompiledRuleset, player: PlayerId, at: TileId, key: &str) -> UnitHandle {
    game.debug_spawn_unit(rules, player, at, unit_kind(rules, key))
}

fn spawn_at(game: &mut GameState, rules: &CompiledRuleset, player: PlayerId, at: (i32, i32), key: &str) -> UnitHandle {
    let at = tile(game, at.0, at.1);
    spawn(game, rules, player, at, key)
}

fn spawn_in(game: &mut GameState, rules: &CompiledRuleset, player: PlayerId, city: CityHandle, key: &str) -> UnitHandle {
    let at = city_tile(game, city);
    spawn(game, rules, player, at, key)
}

/// End turns until `player` is active, then execute `command`.
fn act(game: &mut GameState, rules: &CompiledRuleset, player: PlayerId, command: Command) {
    for _ in 0..4 {
        let active = game.view().auth.active_player();
        if active == player {
            break;
        }
        game.execute(rules, Command::EndTurn { player: active }).unwrap();
    }
    game.execute(rules, command).unwrap();
}

struct Founding<'a> {
    founder: &'a str,
    follower: &'a str,
    pantheon: &'a str,
    bonus: Option<&'a str>,
}

impl Founding<'_> {
    fn plain() -> Self {
        Founding {
            founder: "core:belief/founder/tithe",
            follower: "core:belief/follower/pagodas",
            pantheon: "core:belief/pantheon/ancestor-worship",
            bonus: None,
        }
    }
}

/// `player` founds `religion` in `city` with a fresh Great Prophet.
fn found_religion(
    game: &mut GameState,
    rules: &CompiledRuleset,
    player: PlayerId,
    city: CityHandle,
    religion: u16,
    with: Founding<'_>,
) {
    let prophet = spawn_in(game, rules, player, city, "core:unit/great-prophet");
    let command = Command::FoundReligion {
        unit: prophet,
        religion: ReligionId(religion),
        founder: belief(rules, with.founder),
        follower: belief(rules, with.follower),
        pantheon: Some(belief(rules, with.pantheon)),
        bonus: with.bonus.map(|b| belief(rules, b)),
    };
    act(game, rules, player, command);
}

/// `player` adds a Reformation belief (the Reformation policy adopted for free).
fn reform(game: &mut GameState, rules: &CompiledRuleset, player: PlayerId, key: &str) {
    game.debug_adopt_policy(rules, player, policy(rules, "core:policy/piety/reformation"));
    act(game, rules, player, Command::AddReformationBelief { player, belief: belief(rules, key) });
}

fn happiness(game: &GameState, rules: &CompiledRuleset, city: CityHandle) -> i32 {
    let auth = game.view().auth;
    city_modifiers(auth, rules, auth.cities().get(city).unwrap()).module.happiness
}

fn pressure(game: &GameState, city: CityHandle, religion: u16) -> u32 {
    game.view().auth.module().religion().city(city).pressure_of(Creed::Religion(ReligionId(religion)))
}

// ---- two conditions, river, peace, capital connection ----

#[test]
fn asceticism_needs_a_shrine_and_three_followers() {
    let rules = rules();
    let mut game = world(&rules);
    let holy = game.debug_found_city(&rules, P0, tile(&game, 5, 5));
    game.debug_set_population(&rules, holy, 3);
    let with = Founding { follower: "core:belief/follower/asceticism", ..Founding::plain() };
    found_religion(&mut game, &rules, P0, holy, 0, with);
    game.debug_add_building(&rules, holy, building(&rules, "core:building/shrine"));
    // 3 citizens: 15 000 religious pressure against 3 000 atheists, 2 followers.
    let two_followers = happiness(&game, &rules, holy);
    game.debug_set_population(&rules, holy, 4);
    assert_eq!(happiness(&game, &rules, holy) - two_followers, 1, "a shrine with 3 followers");
    game.validate(&rules).unwrap();
}

#[test]
fn sacred_waters_needs_a_city_on_a_river() {
    let rules = rules();
    let mut game = world(&rules);
    let city = game.debug_found_city(&rules, P0, tile(&game, 5, 5));
    game.debug_grant_faith(P0, 10);
    let pantheon = belief(&rules, "core:belief/pantheon/sacred-waters");
    act(&mut game, &rules, P0, Command::FoundPantheon { player: P0, belief: pantheon });
    let dry = happiness(&game, &rules, city);
    let river = city_tile(&game, city);
    game.debug_set_river_batch(&rules, &[river]);
    assert_eq!(happiness(&game, &rules, city) - dry, 1);
    game.validate(&rules).unwrap();
}

#[test]
fn swords_into_plowshares_grows_cities_only_at_peace() {
    let rules = rules();
    let mut game = world(&rules);
    let holy = game.debug_found_city(&rules, P0, tile(&game, 5, 5));
    game.debug_found_city(&rules, P1, tile(&game, 30, 15));
    let with = Founding { follower: "core:belief/follower/swords-into-plowshares", ..Founding::plain() };
    found_religion(&mut game, &rules, P0, holy, 0, with);
    let growth = |game: &GameState| {
        let auth = game.view().auth;
        city_modifiers(auth, &rules, auth.cities().get(holy).unwrap()).growth_percent
    };
    assert_eq!(growth(&game), 15);
    game.debug_force_war(&rules, P0, P1);
    assert_eq!(growth(&game), 0, "at war with a nation");
    game.validate(&rules).unwrap();
}

#[test]
fn messenger_of_the_gods_needs_a_connection_at_turn_start() {
    let rules = rules();
    let mut game = world(&rules);
    let capital = game.debug_found_city(&rules, P0, tile(&game, 5, 5));
    let other = game.debug_found_city(&rules, P0, tile(&game, 9, 5));
    game.debug_found_city(&rules, P1, tile(&game, 30, 15));
    game.debug_grant_faith(P0, 10);
    let pantheon = belief(&rules, "core:belief/pantheon/messenger-of-the-gods");
    act(&mut game, &rules, P0, Command::FoundPantheon { player: P0, belief: pantheon });
    let science = |game: &GameState, city: CityHandle| {
        let auth = game.view().auth;
        city_modifiers(auth, &rules, auth.cities().get(city).unwrap()).yields[YieldKind::Science.index()].flat
    };
    assert_eq!(science(&game, other), 0, "not connected yet");
    let capital_before = science(&game, capital);
    let road: Vec<TileId> = (6..9).map(|q| tile(&game, q, 5)).collect();
    game.debug_set_road_batch(&rules, &road);
    assert_eq!(science(&game, other), 0, "connections are taken at the start of a turn");
    act(&mut game, &rules, P0, Command::EndTurn { player: P0 });
    act(&mut game, &rules, P1, Command::EndTurn { player: P1 });
    assert_eq!(science(&game, other), 2);
    assert_eq!(science(&game, capital) - capital_before, 2, "the capital counts as connected");
    game.validate(&rules).unwrap();
    let saved = encode(&game, &rules);
    let restored = decode(&saved, &rules).unwrap();
    assert_eq!(authoritative_hash(&restored), authoritative_hash(&game));
}

// ---- triggers ----

#[test]
fn reliquary_and_mausoleum_pay_when_a_great_person_is_expended() {
    let rules = rules();
    let mut game = world(&rules);
    let holy = game.debug_found_city(&rules, P0, tile(&game, 5, 5));
    found_religion(&mut game, &rules, P0, holy, 0, Founding::plain());
    let prophet = spawn_in(&mut game, &rules, P0, holy, "core:unit/great-prophet");
    let enhance = Command::EnhanceReligion {
        unit: prophet,
        follower: belief(&rules, "core:belief/follower/asceticism"),
        enhancer: belief(&rules, "core:belief/enhancer/reliquary"),
    };
    act(&mut game, &rules, P0, enhance);
    game.debug_complete_building(&rules, holy, building(&rules, "core:building/mausoleum_of_halicarnassus"));
    let faith = game.view().auth.module().religion().player(P0).faith;
    let gold = game.view().auth.players().treasury_of(P0);
    let artist = spawn_in(&mut game, &rules, P0, holy, "core:unit/great-artist");
    act(&mut game, &rules, P0, Command::GreatPersonAction { unit: artist, action: GreatPersonAction::StartGoldenAge });
    assert_eq!(game.view().auth.module().religion().player(P0).faith - faith, 50, "Reliquary");
    assert_eq!(game.view().auth.players().treasury_of(P0) - gold, 100, "Mausoleum of Halicarnassus");
    game.validate(&rules).unwrap();
}

#[test]
fn initiation_rites_pays_once_per_city() {
    let rules = rules();
    let mut game = world(&rules);
    let holy = game.debug_found_city(&rules, P0, tile(&game, 5, 5));
    let gold = game.view().auth.players().treasury_of(P0);
    let with = Founding { founder: "core:belief/founder/initiation-rites", ..Founding::plain() };
    found_religion(&mut game, &rules, P0, holy, 0, with);
    assert_eq!(game.view().auth.players().treasury_of(P0) - gold, 100, "the holy city converts");
    assert_eq!(game.view().auth.module().religion().city(holy).adoption_bonus, AdoptionBonus::Paid);
    let saved = encode(&game, &rules);
    let restored = decode(&saved, &rules).unwrap();
    assert_eq!(authoritative_hash(&restored), authoritative_hash(&game));
    game.validate(&rules).unwrap();
}

/// Player 1 follows religion 1 in its city; player 0 founded religion 0.
fn rival_religions(rules: &CompiledRuleset, founder0: &str) -> (GameState, CityHandle, CityHandle) {
    let mut game = world(rules);
    let holy0 = game.debug_found_city(rules, P0, tile(&game, 5, 5));
    let holy1 = game.debug_found_city(rules, P1, tile(&game, 9, 5));
    game.debug_set_population(rules, holy1, 4);
    let with0 = Founding { founder: founder0, ..Founding::plain() };
    found_religion(&mut game, rules, P0, holy0, 0, with0);
    let with1 = Founding {
        founder: "core:belief/founder/church-property",
        follower: "core:belief/follower/cathedrals",
        pantheon: "core:belief/pantheon/god-of-craftsmen",
        bonus: None,
    };
    found_religion(&mut game, rules, P1, holy1, 1, with1);
    (game, holy0, holy1)
}

#[test]
fn interfaith_dialogue_pays_science_per_follower_of_other_religions() {
    let rules = rules();
    let (mut game, _, holy1) = rival_religions(&rules, "core:belief/founder/interfaith-dialogue");
    let next_to = tile(&game, 8, 5);
    let missionary = spawn(&mut game, &rules, P0, next_to, "core:unit/missionary");
    let auth = game.view().auth;
    let others = classic_rules::derived::religion::city_followers(auth, holy1, Creed::Religion(ReligionId(1)));
    assert!(others > 0);
    let science = auth.players().science_of(P0);
    act(&mut game, &rules, P0, Command::SpreadReligion { unit: missionary });
    assert_eq!(game.view().auth.players().science_of(P0) - science, 10 * others);
    game.validate(&rules).unwrap();
}

// ---- religion numbers and rules ----

#[test]
fn evangelism_erodes_the_other_religions() {
    let rules = rules();
    let (mut game, _, holy1) = rival_religions(&rules, "core:belief/founder/tithe");
    reform(&mut game, &rules, P0, "core:belief/reformation/evangelism");
    let missionary = spawn_at(&mut game, &rules, P0, (8, 5), "core:unit/missionary");
    let before = pressure(&game, holy1, 1);
    act(&mut game, &rules, P0, Command::SpreadReligion { unit: missionary });
    // A missionary adds 10 000; Evangelism takes 100 % of it from religion 1.
    assert_eq!(pressure(&game, holy1, 1), before.saturating_sub(10_000));
    game.validate(&rules).unwrap();
}

#[test]
fn unity_of_the_prophets_keeps_half_against_an_inquisitor() {
    let rules = rules();
    let (mut game, holy0, _) = rival_religions(&rules, "core:belief/founder/tithe");
    reform(&mut game, &rules, P1, "core:belief/reformation/unity-of-the-prophets");
    let missionary = spawn_at(&mut game, &rules, P1, (6, 5), "core:unit/missionary");
    act(&mut game, &rules, P1, Command::SpreadReligion { unit: missionary });
    // Player 0's turn first: the round's pressure lands before the count.
    act(&mut game, &rules, P1, Command::EndTurn { player: P1 });
    let inquisitor = spawn_in(&mut game, &rules, P0, holy0, "core:unit/inquisitor");
    let before = pressure(&game, holy0, 1);
    assert!(before > 0);
    act(&mut game, &rules, P0, Command::RemoveHeresy { unit: inquisitor });
    assert_eq!(pressure(&game, holy0, 1), before / 2);
    game.validate(&rules).unwrap();
}

#[test]
fn heathen_conversion_turns_adjacent_barbarians() {
    let rules = rules();
    let mut game = GameState::new(&rules, GameConfig { players: 3, width: 40, height: 24, rng_seed: 11 });
    let barbarians = PlayerId(2);
    game.debug_set_player_kind(barbarians, PlayerKind::Module(ClassicKind::Barbarian));
    let holy0 = game.debug_found_city(&rules, P0, tile(&game, 5, 5));
    let city1 = game.debug_found_city(&rules, P1, tile(&game, 9, 5));
    found_religion(&mut game, &rules, P0, holy0, 0, Founding::plain());
    reform(&mut game, &rules, P0, "core:belief/reformation/heathen-conversion");
    let missionary = spawn_at(&mut game, &rules, P0, (8, 5), "core:unit/missionary");
    let brute = spawn_at(&mut game, &rules, barbarians, (8, 6), "core:unit/warrior");
    let _ = (city1, brute);
    act(&mut game, &rules, P0, Command::SpreadReligion { unit: missionary });
    let auth = game.view().auth;
    let on_tile: Vec<PlayerId> = auth
        .indexes()
        .units_on(tile(&game, 8, 6))
        .iter()
        .filter_map(|u| auth.units().get(*u).map(|v| v.core.owner))
        .collect();
    assert_eq!(on_tile, vec![P0], "the warrior is player 0's now");
    game.validate(&rules).unwrap();
}

#[test]
fn to_the_glory_of_god_buys_any_great_person_from_the_industrial_era() {
    let rules = rules();
    let mut game = world(&rules);
    let holy = game.debug_found_city(&rules, P0, tile(&game, 5, 5));
    found_religion(&mut game, &rules, P0, holy, 0, Founding::plain());
    let industrial = rules
        .technologies
        .iter()
        .position(|t| t.content_era.is_some_and(|e| rules.eras[e.0 as usize].index >= 4))
        .unwrap();
    game.debug_learn(&rules, P0, classic_rules::ids::TechId(industrial as u16));
    game.debug_grant_faith(P0, 5000);
    let scientist = ProductionItem::Unit(unit_kind(&rules, "core:unit/great-scientist"));
    let status = |game: &GameState| faith_purchase_status(game.view().auth, &rules, P0, holy, scientist);
    assert_eq!(status(&game), Err(FaithPurchaseBlock::NeedsBranch));
    reform(&mut game, &rules, P0, "core:belief/reformation/to-the-glory-of-god");
    assert_eq!(status(&game), Ok(1000));
    game.validate(&rules).unwrap();
}

#[test]
fn papal_primacy_raises_the_resting_point_with_following_city_states() {
    let rules = rules();
    let mut game = world(&rules);
    let holy = game.debug_found_city(&rules, P0, tile(&game, 5, 5));
    game.debug_set_player_kind(P1, PlayerKind::Module(ClassicKind::CityState(classic_rules::rules::content::CityStateId(0))));
    let minor = game.debug_found_city(&rules, P1, tile(&game, 9, 5));
    let with = Founding { founder: "core:belief/founder/papal-primacy", ..Founding::plain() };
    found_religion(&mut game, &rules, P0, holy, 0, with);
    game.debug_meet(P0, P1);
    let resting = |game: &GameState| {
        let view = game.view();
        let (obs, _) = AIObservation::for_player(view.auth, view.derived, &rules, P0);
        obs.city_states.iter().find(|c| c.player == P1).unwrap().resting
    };
    let before = resting(&game);
    let missionary = spawn_at(&mut game, &rules, P0, (8, 5), "core:unit/missionary");
    act(&mut game, &rules, P0, Command::SpreadReligion { unit: missionary });
    let follows = classic_rules::derived::religion::city_majority(game.view().auth, minor);
    assert_eq!(follows, Some(Creed::Religion(ReligionId(0))));
    assert_eq!(resting(&game) - before, 15);
    game.validate(&rules).unwrap();
}

#[test]
fn religious_community_adds_production_per_follower_up_to_fifteen() {
    let rules = rules();
    let mut game = world(&rules);
    let holy = game.debug_found_city(&rules, P0, tile(&game, 5, 5));
    game.debug_set_population(&rules, holy, 6);
    let with = Founding { follower: "core:belief/follower/religious-community", ..Founding::plain() };
    found_religion(&mut game, &rules, P0, holy, 0, with);
    let percent = |game: &GameState| {
        let auth = game.view().auth;
        city_modifiers(auth, &rules, auth.cities().get(holy).unwrap()).yields[YieldKind::Production.index()].percent
    };
    let followers = classic_rules::derived::religion::city_followers(game.view().auth, holy, Creed::Religion(ReligionId(0)));
    assert_eq!(percent(&game), followers as i32);
    game.debug_set_population(&rules, holy, 30);
    assert_eq!(percent(&game), 15, "at most 15 %");
    game.validate(&rules).unwrap();
}

#[test]
fn candi_gives_faith_per_religion_present() {
    let rules = rules();
    let (mut game, _, holy1) = rival_religions(&rules, "core:belief/founder/tithe");
    game.debug_set_player_civ(P1, civ(&rules, "core:nation/indonesia"));
    let missionary = spawn_at(&mut game, &rules, P0, (8, 5), "core:unit/missionary");
    act(&mut game, &rules, P0, Command::SpreadReligion { unit: missionary });
    let faith = |game: &GameState| {
        let auth = game.view().auth;
        city_modifiers(auth, &rules, auth.cities().get(holy1).unwrap()).yields[YieldKind::Module(ClassicYield::Faith).index()].flat
    };
    let before = faith(&game);
    game.debug_add_building(&rules, holy1, building(&rules, "core:building/candi"));
    // Its own +2, and two religions with followers, +2 each.
    assert_eq!(faith(&game) - before, 2 + 4);
    game.validate(&rules).unwrap();
}

#[test]
fn peace_loving_counts_foreign_followers_of_players_at_peace() {
    let rules = rules();
    let mut game = world(&rules);
    let holy = game.debug_found_city(&rules, P0, tile(&game, 5, 5));
    let foreign = game.debug_found_city(&rules, P1, tile(&game, 9, 5));
    game.debug_set_population(&rules, foreign, 8);
    let with = Founding { founder: "core:belief/founder/peace-loving", ..Founding::plain() };
    found_religion(&mut game, &rules, P0, holy, 0, with);
    let happiness = |game: &GameState| player_modifiers(game.view().auth, &rules, P0).module.happiness;
    let before = happiness(&game);
    let missionary = spawn_at(&mut game, &rules, P0, (8, 5), "core:unit/missionary");
    act(&mut game, &rules, P0, Command::SpreadReligion { unit: missionary });
    // The city grows: 10 000 religious pressure against 8 000 atheists.
    game.debug_set_population(&rules, foreign, 20);
    let followers = classic_rules::derived::religion::city_followers(game.view().auth, foreign, Creed::Religion(ReligionId(0)));
    assert_eq!(happiness(&game) - before, (followers / 8) as i32);
    assert!(followers >= 8, "{followers} followers abroad");
    game.debug_force_war(&rules, P0, P1);
    assert_eq!(happiness(&game), before, "followers of an enemy do not count");
    game.validate(&rules).unwrap();
}

// ---- faith purchase rule, Byzantium, Religious Tolerance ----

#[test]
fn a_unique_building_takes_the_faith_unlock_of_its_class() {
    let rules = rules();
    let mut game = world(&rules);
    game.debug_set_player_civ(P0, civ(&rules, "core:nation/siam"));
    let holy = game.debug_found_city(&rules, P0, tile(&game, 5, 5));
    found_religion(&mut game, &rules, P0, holy, 0, Founding::plain());
    reform(&mut game, &rules, P0, "core:belief/reformation/jesuit-education");
    let university = building(&rules, "core:building/university");
    let wat = building(&rules, "core:building/wat");
    for b in rules.buildings[wat.0 as usize].requires_buildings.clone() {
        game.debug_add_building(&rules, holy, b);
    }
    if let Some(tech) = rules.buildings[wat.0 as usize].required_tech {
        game.debug_learn(&rules, P0, tech);
    }
    game.debug_grant_faith(P0, 5000);
    let status = |game: &GameState, b: BuildingKindId| {
        faith_purchase_status(game.view().auth, &rules, P0, holy, ProductionItem::Building(b))
    };
    assert!(status(&game, wat).is_ok(), "{:?}", status(&game, wat));
    assert_eq!(status(&game, university), Err(FaithPurchaseBlock::OtherCivilization));
    game.validate(&rules).unwrap();
}

#[test]
fn a_belief_unlocks_the_faith_price_of_a_cathedral() {
    let rules = rules();
    let mut game = world(&rules);
    let holy = game.debug_found_city(&rules, P0, tile(&game, 5, 5));
    found_religion(&mut game, &rules, P0, holy, 0, Founding::plain());
    game.debug_grant_faith(P0, 5000);
    let cathedral = ProductionItem::Building(building(&rules, "core:building/cathedral"));
    assert!(rules.buildings[building(&rules, "core:building/cathedral").0 as usize].module.faith_purchase_needs_belief);
    let status = faith_purchase_status(game.view().auth, &rules, P0, holy, cathedral);
    assert_eq!(status, Err(FaithPurchaseBlock::NeedsBelief), "Pagodas, not Cathedrals");
}

/// Military units can be bought with faith only where a belief unlocks
/// their era. Great people and
/// missionaries are priced by their own rules.
#[test]
fn a_military_unit_needs_a_belief_for_its_faith_price() {
    let rules = rules();
    let mut game = world(&rules);
    let holy = game.debug_found_city(&rules, P0, tile(&game, 5, 5));
    found_religion(&mut game, &rules, P0, holy, 0, Founding::plain());
    game.debug_grant_faith(P0, 5000);
    let knight = unit_kind(&rules, "core:unit/knight");
    let def = &rules.units[knight.0 as usize];
    assert!(def.module.faith_cost.is_some(), "the Knight has a faith price");
    assert!(def.module.abilities.contains(&classic_rules::rules::content::UnitAbility::FaithPurchaseNeedsBelief));
    game.debug_learn(&rules, P0, def.required_tech.unwrap());
    let status = faith_purchase_status(game.view().auth, &rules, P0, holy, ProductionItem::Unit(knight));
    assert_eq!(status, Err(FaithPurchaseBlock::NeedsBelief), "Pagodas unlock no era of units");
}

#[test]
fn byzantium_founds_with_a_bonus_belief() {
    let rules = rules();
    let mut game = world(&rules);
    let holy = game.debug_found_city(&rules, P0, tile(&game, 5, 5));
    let other = game.debug_found_city(&rules, P1, tile(&game, 20, 15));
    let bonus = Founding { bonus: Some("core:belief/founder/church-property"), ..Founding::plain() };
    // Not Byzantium: the bonus belief is refused.
    let prophet = spawn_in(&mut game, &rules, P1, other, "core:unit/great-prophet");
    let _ = game.execute(&rules, Command::EndTurn { player: P0 });
    let refused = game.execute(
        &rules,
        Command::FoundReligion {
            unit: prophet,
            religion: ReligionId(1),
            founder: belief(&rules, bonus.founder),
            follower: belief(&rules, bonus.follower),
            pantheon: Some(belief(&rules, bonus.pantheon)),
            bonus: Some(belief(&rules, "core:belief/founder/church-property")),
        },
    );
    assert!(refused.is_err());
    game.debug_set_player_civ(P0, civ(&rules, "core:nation/byzantium"));
    found_religion(&mut game, &rules, P0, holy, 0, bonus);
    let founded = game.view().auth.module().religion().religion(ReligionId(0)).unwrap().beliefs.clone();
    assert_eq!(founded.len(), 4, "pantheon, founder, follower and the bonus");
    assert!(founded.contains(&belief(&rules, "core:belief/founder/church-property")));
    game.validate(&rules).unwrap();
}

#[test]
fn religious_tolerance_adds_the_pantheon_of_the_second_religion() {
    let rules = rules();
    let mut game = world(&rules);
    let holy0 = game.debug_found_city(&rules, P0, tile(&game, 5, 5));
    let holy1 = game.debug_found_city(&rules, P1, tile(&game, 9, 5));
    game.debug_set_population(&rules, holy0, 6);
    found_religion(&mut game, &rules, P0, holy0, 0, Founding::plain());
    let with1 = Founding {
        founder: "core:belief/founder/church-property",
        follower: "core:belief/follower/cathedrals",
        pantheon: "core:belief/pantheon/sacred-waters",
        bonus: None,
    };
    found_religion(&mut game, &rules, P1, holy1, 1, with1);
    let river = city_tile(&game, holy0);
    game.debug_set_river_batch(&rules, &[river]);
    let missionary = spawn_at(&mut game, &rules, P1, (6, 5), "core:unit/missionary");
    act(&mut game, &rules, P1, Command::SpreadReligion { unit: missionary });
    let majority = classic_rules::derived::religion::city_majority(game.view().auth, holy0);
    assert_eq!(majority, Some(Creed::Religion(ReligionId(0))), "religion 0 keeps the majority");
    assert!(pressure(&game, holy0, 1) > 0);
    let before = happiness(&game, &rules, holy0);
    game.debug_adopt_policy(&rules, P0, policy(&rules, "core:policy/piety/religious-tolerance"));
    assert_eq!(happiness(&game, &rules, holy0) - before, 1, "Sacred Waters of religion 1");
    game.validate(&rules).unwrap();
}

#[test]
fn the_ai_of_byzantium_picks_a_bonus_belief() {
    let rules = rules();
    let mut game = world(&rules);
    game.debug_set_player_civ(P0, civ(&rules, "core:nation/byzantium"));
    let capital = game.debug_found_city(&rules, P0, tile(&game, 5, 5));
    spawn_in(&mut game, &rules, P0, capital, "core:unit/great-prophet");
    let obs = game.ai_observation(&rules, P0);
    assert_eq!(obs.faith.overview.extra_founding_beliefs, 1);
    let commands = classic_rules::ai::religion::religion_commands(&obs, &rules);
    let found = commands.iter().find(|c| matches!(c, Command::FoundReligion { .. })).cloned().unwrap();
    assert!(matches!(found, Command::FoundReligion { bonus: Some(_), .. }), "{found:?}");
    game.execute(&rules, found).unwrap();
    let beliefs = &game.view().auth.module().religion().religion(ReligionId(0)).unwrap().beliefs;
    assert_eq!(beliefs.len(), 4);
    game.validate(&rules).unwrap();
}

// ---- the Unwelcome Evangelist ----

/// A missionary that ends its turn in rival land (closed borders) loses a
/// quarter of its original religious strength each time, which shows in the
/// pressure of its spread; after four turns it is gone. In its own land,
/// or in land the host opened to it, nothing is lost.
#[test]
fn a_missionary_loses_strength_for_each_turn_in_rival_land() {
    let rules = rules();
    let mut game = world(&rules);
    let holy0 = game.debug_found_city(&rules, P0, tile(&game, 5, 5));
    game.debug_found_city(&rules, P1, tile(&game, 9, 5));
    found_religion(&mut game, &rules, P0, holy0, 0, Founding::plain());
    let rival = spawn_at(&mut game, &rules, P0, (8, 5), "core:unit/missionary");
    let home = spawn_at(&mut game, &rules, P0, (5, 6), "core:unit/missionary");
    let rival_id = game.view().auth.units().get(rival).unwrap().core.id;
    let lost = |game: &GameState| game.view().auth.module().religion().unit(rival_id).map_or(0, |u| u.strength_lost);
    let pressure = |game: &GameState| {
        classic_rules::derived::religion::spread_status(game.view().auth, &rules, rival).unwrap().pressure
    };
    let full = pressure(&game);
    // End P0's turn, then the rest of the round.
    let end_round = |game: &mut GameState| {
        for _ in 0..2 {
            let active = game.view().auth.active_player();
            game.execute(&rules, Command::EndTurn { player: active }).unwrap();
        }
    };
    end_round(&mut game);
    assert_eq!(lost(&game), 25);
    assert_eq!(pressure(&game), full * 3 / 4, "a quarter of the strength is gone");
    end_round(&mut game);
    end_round(&mut game);
    assert_eq!(lost(&game), 75);
    end_round(&mut game);
    assert!(game.view().auth.units().get(rival).is_none(), "no strength left: the missionary is lost");
    assert!(game.view().auth.units().get(home).is_some(), "the one in its own land stays");
    game.validate(&rules).unwrap();
    // The lost strength survives a save.
    let save = encode(&game, &rules);
    let loaded = decode(&classic_rules::save_codec::from_bytes(&classic_rules::save_codec::to_bytes(&save)).unwrap(), &rules).unwrap();
    assert_eq!(authoritative_hash(&loaded), authoritative_hash(&game));
}

/// A save whose religious unit lost more than all its strength is corrupt
/// (the spread pressure would read a negative remainder).
#[test]
fn a_religious_unit_that_lost_more_than_all_its_strength_is_a_corrupt_save() {
    let rules = rules();
    let mut game = world(&rules);
    let holy = game.debug_found_city(&rules, P0, tile(&game, 5, 5));
    game.debug_found_city(&rules, P1, tile(&game, 9, 5));
    found_religion(&mut game, &rules, P0, holy, 0, Founding::plain());
    spawn_at(&mut game, &rules, P0, (8, 5), "core:unit/missionary");
    for _ in 0..2 {
        let active = game.view().auth.active_player();
        game.execute(&rules, Command::EndTurn { player: active }).unwrap();
    }
    let mut save = encode(&game, &rules);
    assert_eq!(save.world.module.faith.units[0].strength_lost, 25, "a turn in rival land is on record");
    assert!(decode(&save, &rules).is_ok());
    save.world.module.faith.units[0].strength_lost = 100;
    assert!(decode(&save, &rules).is_ok(), "all the strength lost is the limit, not beyond it");
    save.world.module.faith.units[0].strength_lost = 101;
    assert!(matches!(decode(&save, &rules), Err(classic_rules::persistence::decode::DecodeError::Corrupt(_))));
}
