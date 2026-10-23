//! Faith and religion on the classic ruleset (classic/docs/classic/rules.md):
//! the faith pool, pantheons and their beliefs, the faith Great Prophet,
//! founding and enhancing a religion, pressure between cities each round,
//! missionaries, inquisitors, Reformation, faith purchases, the Holy Site,
//! save/load and the religion AI.

use classic_rules::ClassicGame;
use classic_content::core::core_rules;
use classic_rules::derived::income::{city_income, player_income};
use classic_rules::derived::religion::{
    city_majority, faith_purchase_status, pantheon_cost, religion_counts, FaithPurchaseBlock,
};
use classic_rules::ids::{
    BeliefId, BuildingKindId, CityHandle, PlayerId, ProductionItem, TileId, UnitHandle, UnitKindId,
};
use classic_rules::persistence::{decode::decode, encode::encode};
use classic_rules::replay::authoritative_hash;
use classic_rules::rules::content::ReligionId;
use classic_rules::world::religion::Creed;
use classic_rules::{compile_rules, Command, CompiledRuleset, GameConfig, GameState};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

fn rules() -> CompiledRuleset {
    compile_rules(core_rules()).unwrap()
}

fn world(rules: &CompiledRuleset) -> GameState {
    GameState::new(
        rules,
        GameConfig {
            players: 2,
            width: 40,
            height: 24,
            rng_seed: 11,
        },
    )
}

fn tile(game: &GameState, q: i32, r: i32) -> TileId {
    let dims = game.view().auth.map().dimensions;
    dims.to_tile(classic_rules::math::HexCoord { q, r }).unwrap()
}

fn belief(rules: &CompiledRuleset, key: &str) -> BeliefId {
    BeliefId(rules.module.beliefs.iter().position(|b| b.key == key).unwrap() as u16)
}

fn building(rules: &CompiledRuleset, key: &str) -> BuildingKindId {
    BuildingKindId(rules.buildings.iter().position(|b| b.key == key).unwrap() as u16)
}

fn unit_kind(rules: &CompiledRuleset, key: &str) -> UnitKindId {
    UnitKindId(rules.units.iter().position(|u| u.key == key).unwrap() as u16)
}

fn found(
    game: &mut GameState,
    rules: &CompiledRuleset,
    player: PlayerId,
    q: i32,
    r: i32,
) -> CityHandle {
    let t = tile(game, q, r);
    game.debug_found_city(rules, player, t)
}

fn faith(game: &GameState, player: PlayerId) -> u32 {
    game.view().auth.module().religion().player(player).faith
}

/// Both players end their turns once (one round).
fn round(game: &mut GameState, rules: &CompiledRuleset) {
    for _ in 0..2 {
        let player = game.view().auth.active_player();
        game.execute(rules, Command::EndTurn { player }).unwrap();
    }
}

fn spawn(
    game: &mut GameState,
    rules: &CompiledRuleset,
    player: PlayerId,
    at: TileId,
    key: &str,
) -> UnitHandle {
    game.debug_spawn_unit(rules, player, at, unit_kind(rules, key))
}

/// A world where player 0 founded Buddhism (Tithe, Pagodas) in its capital.
fn with_religion(rules: &CompiledRuleset) -> (GameState, CityHandle) {
    let mut game = world(rules);
    let holy = found(&mut game, rules, P0, 5, 5);
    let at = game.view().auth.cities().get(holy).unwrap().tile;
    let prophet = spawn(&mut game, rules, P0, at, "core:unit/great-prophet");
    game.execute(
        rules,
        Command::FoundReligion {
            unit: prophet,
            religion: ReligionId(0),
            founder: belief(rules, "core:belief/founder/tithe"),
            follower: belief(rules, "core:belief/follower/pagodas"),
            pantheon: Some(belief(rules, "core:belief/pantheon/ancestor-worship")),
            bonus: None,
        },
    )
    .unwrap();
    (game, holy)
}

#[test]
fn shrines_fill_the_faith_pool_each_turn() {
    let rules = rules();
    let mut game = world(&rules);
    let city = found(&mut game, &rules, P0, 5, 5);
    found(&mut game, &rules, P1, 30, 15);
    game.debug_add_building(&rules, city, building(&rules, "core:building/shrine"));
    let auth = game.view().auth;
    assert_eq!(
        city_income(auth, &rules, auth.cities().get(city).unwrap()).faith,
        1
    );
    assert_eq!(player_income(auth, &rules, P0).faith, 1);
    round(&mut game, &rules);
    round(&mut game, &rules);
    assert_eq!(faith(&game, P0), 2);
}

#[test]
fn pantheons_cost_more_for_each_founded_and_take_their_belief() {
    let rules = rules();
    let mut game = world(&rules);
    let city = found(&mut game, &rules, P0, 5, 5);
    found(&mut game, &rules, P1, 25, 15);
    game.debug_add_building(&rules, city, building(&rules, "core:building/shrine"));
    let ancestors = belief(&rules, "core:belief/pantheon/ancestor-worship");
    assert_eq!(pantheon_cost(game.view().auth), 10);
    let refused = game
        .execute(
            &rules,
            Command::FoundPantheon {
                player: P0,
                belief: ancestors,
            },
        )
        .unwrap_err();
    assert_eq!(
        format!("{refused:?}"),
        "RuleViolation(\"not enough faith\")"
    );
    game.debug_grant_faith(P0, 12);
    game.execute(
        &rules,
        Command::FoundPantheon {
            player: P0,
            belief: ancestors,
        },
    )
    .unwrap();
    assert_eq!(faith(&game, P0), 2);
    assert_eq!(
        pantheon_cost(game.view().auth),
        15,
        "the next pantheon costs 5 more"
    );
    // The capital follows the pantheon: Ancestor Worship gives +1 culture
    // from its Shrine (Palace 1 + 1).
    let auth = game.view().auth;
    assert_eq!(city_majority(auth, city), Some(Creed::Pantheon));
    assert_eq!(
        city_income(auth, &rules, auth.cities().get(city).unwrap()).culture,
        2
    );
    // The belief is taken for player 1.
    round(&mut game, &rules);
    game.execute(&rules, Command::EndTurn { player: P0 })
        .unwrap();
    game.debug_grant_faith(P1, 100);
    let taken = game
        .execute(
            &rules,
            Command::FoundPantheon {
                player: P1,
                belief: ancestors,
            },
        )
        .unwrap_err();
    assert_eq!(
        format!("{taken:?}"),
        "RuleViolation(\"belief already taken\")"
    );
    game.validate(&rules).unwrap();
}

#[test]
fn faith_spawns_a_great_prophet_in_the_capital_and_burns_the_pool() {
    let rules = rules();
    let mut game = world(&rules);
    let capital = found(&mut game, &rules, P0, 5, 5);
    // Faith 300 against a price of 200: the chance 5 + 100 is certain.
    game.debug_grant_faith(P0, 300);
    let prophet = unit_kind(&rules, "core:unit/great-prophet");
    game.execute(&rules, Command::EndTurn { player: P0 })
        .unwrap();
    let auth = game.view().auth;
    let prophets: Vec<_> = auth
        .indexes()
        .units_of(P0)
        .iter()
        .filter(|h| auth.units().get(**h).unwrap().core.kind == prophet)
        .collect();
    assert_eq!(prophets.len(), 1);
    let at = auth.units().get(*prophets[0]).unwrap().core.tile;
    let city_tile = auth.cities().get(capital).unwrap().tile;
    assert!(auth.map().dimensions.distance(at, city_tile).unwrap() <= 1);
    assert_eq!(faith(&game, P0), 0, "all faith is spent");
    assert_eq!(auth.module().religion().player(P0).prophets, 1);
}

#[test]
fn a_prophet_founds_a_religion_in_its_city() {
    let rules = rules();
    let (game, holy) = with_religion(&rules);
    let auth = game.view().auth;
    let founded = auth.module().religion().religion(ReligionId(0)).unwrap();
    assert_eq!(founded.holy_city, holy);
    assert_eq!(founded.founder, P0);
    assert_eq!(founded.beliefs.len(), 3, "pantheon, founder, follower");
    assert_eq!(auth.module().religion().player(P0).religion, Some(ReligionId(0)));
    assert_eq!(
        city_majority(auth, holy),
        Some(Creed::Religion(ReligionId(0)))
    );
    assert!(
        auth.indexes().units_of(P0).is_empty(),
        "the prophet is spent"
    );
    // The pantheon chosen with the religion belongs to the player.
    assert!(auth.module().religion().player(P0).pantheon.is_some());
    game.validate(&rules).unwrap();
}

#[test]
fn holy_city_pressure_converts_a_neighbour_over_rounds() {
    let rules = rules();
    let (mut game, _) = with_religion(&rules);
    let neighbour = found(&mut game, &rules, P0, 10, 5);
    assert_eq!(city_majority(game.view().auth, neighbour), None);
    // 1 citizen = 1000 atheist pressure; the holy city adds 60 a round.
    for _ in 0..20 {
        round(&mut game, &rules);
    }
    let auth = game.view().auth;
    assert_eq!(
        city_majority(auth, neighbour),
        Some(Creed::Religion(ReligionId(0)))
    );
    let counts = religion_counts(auth, ReligionId(0));
    assert_eq!(counts.cities, 2);
    game.validate(&rules).unwrap();
}

#[test]
fn church_property_pays_gold_per_following_city() {
    let rules = rules();
    let mut game = world(&rules);
    let holy = found(&mut game, &rules, P0, 5, 5);
    let before = player_income(game.view().auth, &rules, P0).gold;
    let at = game.view().auth.cities().get(holy).unwrap().tile;
    let prophet = spawn(&mut game, &rules, P0, at, "core:unit/great-prophet");
    let with_prophet = player_income(game.view().auth, &rules, P0).gold;
    // classic unit maintenance (M4-A): a unit costs gold only past the
    // difficulty's free units, so the prophet may cost nothing here.
    assert!(with_prophet <= before, "the prophet's upkeep");
    game.execute(
        &rules,
        Command::FoundReligion {
            unit: prophet,
            religion: ReligionId(0),
            founder: belief(&rules, "core:belief/founder/church-property"),
            follower: belief(&rules, "core:belief/follower/pagodas"),
            pantheon: Some(belief(&rules, "core:belief/pantheon/ancestor-worship")),
            bonus: None,
        },
    )
    .unwrap();
    // One following city: +2 gold; the spent prophet costs no upkeep.
    let after = player_income(game.view().auth, &rules, P0).gold;
    assert_eq!(after, before + 2);
}

#[test]
fn missionaries_are_bought_with_faith_in_a_following_city() {
    let rules = rules();
    let (mut game, holy) = with_religion(&rules);
    let other = found(&mut game, &rules, P0, 15, 5);
    let missionary = ProductionItem::Unit(unit_kind(&rules, "core:unit/missionary"));
    assert_eq!(
        faith_purchase_status(game.view().auth, &rules, P0, holy, missionary),
        Err(FaithPurchaseBlock::NotEnoughFaith)
    );
    game.debug_grant_faith(P0, 400);
    assert_eq!(
        faith_purchase_status(game.view().auth, &rules, P0, other, missionary),
        Err(FaithPurchaseBlock::NeedsReligion),
        "the other city follows no religion yet"
    );
    game.execute(
        &rules,
        Command::PurchaseWithFaith {
            player: P0,
            city: holy,
            item: missionary,
        },
    )
    .unwrap();
    assert_eq!(faith(&game, P0), 200, "200 faith in the Ancient era");
    let unit = *game.view().auth.indexes().units_of(P0).last().unwrap();
    let id = game.view().auth.units().get(unit).unwrap().core.id;
    let carried = *game.view().auth.module().religion().unit(id).unwrap();
    assert_eq!((carried.religion, carried.spreads_left), (ReligionId(0), 2));
    game.validate(&rules).unwrap();
}

#[test]
fn a_missionary_converts_two_cities_and_is_spent() {
    let rules = rules();
    let (mut game, _) = with_religion(&rules);
    // Two cities two tiles apart: a tile between them touches both.
    let first = found(&mut game, &rules, P0, 20, 5);
    let second = found(&mut game, &rules, P0, 22, 5);
    let between = tile(&game, 21, 5);
    let missionary = spawn(&mut game, &rules, P0, between, "core:unit/missionary");
    game.execute(&rules, Command::SpreadReligion { unit: missionary })
        .unwrap();
    let religion = Some(Creed::Religion(ReligionId(0)));
    let following = |game: &GameState| {
        [first, second]
            .iter()
            .filter(|c| city_majority(game.view().auth, **c) == religion)
            .count()
    };
    assert_eq!(following(&game), 1, "10 000 against 1 000 atheists");
    assert!(
        game.view().auth.units().get(missionary).is_some(),
        "one spread left"
    );
    // The second spread goes to the city that does not follow yet.
    game.execute(&rules, Command::SpreadReligion { unit: missionary })
        .unwrap();
    assert_eq!(following(&game), 2);
    assert!(game.view().auth.units().get(missionary).is_none(), "spent");
    game.validate(&rules).unwrap();
}

#[test]
fn enhancing_needs_a_second_prophet_and_unlocks_inquisitors() {
    let rules = rules();
    let (mut game, holy) = with_religion(&rules);
    let inquisitor = ProductionItem::Unit(unit_kind(&rules, "core:unit/inquisitor"));
    game.debug_grant_faith(P0, 1000);
    assert_eq!(
        faith_purchase_status(game.view().auth, &rules, P0, holy, inquisitor),
        Err(FaithPurchaseBlock::NeedsEnhancedReligion)
    );
    let at = game.view().auth.cities().get(holy).unwrap().tile;
    let prophet = spawn(&mut game, &rules, P0, at, "core:unit/great-prophet");
    game.execute(
        &rules,
        Command::EnhanceReligion {
            unit: prophet,
            follower: belief(&rules, "core:belief/follower/cathedrals"),
            enhancer: belief(&rules, "core:belief/enhancer/missionary-zeal"),
        },
    )
    .unwrap();
    let founded = game.view().auth.module().religion().religion(ReligionId(0)).unwrap();
    assert!(founded.enhanced);
    assert_eq!(founded.beliefs.len(), 5);
    assert!(faith_purchase_status(game.view().auth, &rules, P0, holy, inquisitor).is_ok());
    // Cathedrals: the holy city may buy a Cathedral with faith.
    let cathedral = ProductionItem::Building(building(&rules, "core:building/cathedral"));
    assert_eq!(
        faith_purchase_status(game.view().auth, &rules, P0, holy, cathedral),
        Ok(200)
    );
    game.execute(
        &rules,
        Command::PurchaseWithFaith {
            player: P0,
            city: holy,
            item: cathedral,
        },
    )
    .unwrap();
    let has = game
        .view()
        .auth
        .cities()
        .get(holy)
        .unwrap()
        .buildings
        .contains(&building(&rules, "core:building/cathedral"));
    assert!(has);
    game.validate(&rules).unwrap();
}

#[test]
fn a_great_prophet_builds_a_holy_site_on_own_land() {
    let rules = rules();
    let mut game = world(&rules);
    let city = found(&mut game, &rules, P0, 5, 5);
    let center = game.view().auth.cities().get(city).unwrap().tile;
    let dims = game.view().auth.map().dimensions;
    let next_to = dims.neighbors(center).into_iter().next().unwrap();
    let prophet = spawn(&mut game, &rules, P0, next_to, "core:unit/great-prophet");
    game.execute(&rules, Command::CreateGreatImprovement { unit: prophet })
        .unwrap();
    let holy_site = rules
        .improvements
        .iter()
        .position(|i| i.key == "core:improvement/holy-site")
        .unwrap();
    assert_eq!(
        game.view().auth.map().improvement(next_to),
        Some(Some(classic_rules::ids::ImprovementId(holy_site as u16)))
    );
    game.validate(&rules).unwrap();
}

#[test]
fn religion_survives_save_and_load() {
    let rules = rules();
    let (mut game, _) = with_religion(&rules);
    found(&mut game, &rules, P0, 10, 5);
    for _ in 0..3 {
        round(&mut game, &rules);
    }
    let save = encode(&game, &rules);
    let loaded = decode(&save, &rules).unwrap();
    assert_eq!(authoritative_hash(&loaded), authoritative_hash(&game));
    let mut bad = save.clone();
    let first = bad.world.module.faith.religions[0].beliefs[0];
    bad.world.module.faith.religions[0].beliefs.push(first);
    assert!(
        decode(&bad, &rules).is_err(),
        "a belief taken twice is corrupt"
    );
}

#[test]
fn the_ai_founds_a_pantheon_and_a_religion() {
    let rules = rules();
    let mut game = world(&rules);
    let capital = found(&mut game, &rules, P0, 5, 5);
    game.debug_grant_faith(P0, 20);
    let obs = game.ai_observation(&rules, P0);
    let commands = classic_rules::ai::religion::religion_commands(&obs, &rules);
    assert!(matches!(commands[0], Command::FoundPantheon { .. }));
    for cmd in commands {
        game.execute(&rules, cmd).unwrap();
    }
    assert!(game.view().auth.module().religion().player(P0).pantheon.is_some());
    let at = game.view().auth.cities().get(capital).unwrap().tile;
    spawn(&mut game, &rules, P0, at, "core:unit/great-prophet");
    let obs = game.ai_observation(&rules, P0);
    let commands = classic_rules::ai::religion::religion_commands(&obs, &rules);
    assert!(commands
        .iter()
        .any(|c| matches!(c, Command::FoundReligion { .. })));
    for cmd in commands {
        let _ = game.execute(&rules, cmd);
    }
    assert!(game.view().auth.module().religion().player(P0).religion.is_some());
    game.validate(&rules).unwrap();
}

/// Regression: a foreign city following a religion grows; the holy city's
/// World Church culture counts its followers, so the founder's cities are
/// recomputed with the growth (`derived::yields::players_to_refresh`).
#[test]
fn a_following_city_that_grows_refreshes_the_founder() {
    let rules = rules();
    let mut game = world(&rules);
    let holy = found(&mut game, &rules, P0, 5, 5);
    let foreign = found(&mut game, &rules, P1, 9, 5);
    game.debug_set_population(&rules, foreign, 4);
    let at = game.view().auth.cities().get(holy).unwrap().tile;
    let prophet = spawn(&mut game, &rules, P0, at, "core:unit/great-prophet");
    game.execute(
        &rules,
        Command::FoundReligion {
            unit: prophet,
            religion: ReligionId(0),
            founder: belief(&rules, "core:belief/founder/world-church"),
            follower: belief(&rules, "core:belief/follower/pagodas"),
            pantheon: Some(belief(&rules, "core:belief/pantheon/ancestor-worship")),
            bonus: None,
        },
    )
    .unwrap();
    let next_to = tile(&game, 8, 5);
    let missionary = spawn(&mut game, &rules, P0, next_to, "core:unit/missionary");
    game.execute(&rules, Command::SpreadReligion { unit: missionary }).unwrap();
    let culture = |game: &GameState| {
        let auth = game.view().auth;
        let core = auth.cities().get(holy).unwrap();
        let mods = classic_rules::effects::city::city_modifiers(auth, &rules, core);
        mods.yields[classic_rules::rules::effects::YieldKind::Culture.index()].flat
    };
    let before = culture(&game);
    game.debug_set_population(&rules, foreign, 12);
    assert!(culture(&game) > before, "more foreign followers");
    game.validate(&rules).unwrap();
}

/// A prophet of an enhanced religion builds its Holy Site only where the
/// terrain suits it: on hills it steps to a tile that does (the core refused
/// the site on every turn before).
#[test]
fn the_ai_prophet_builds_its_holy_site_only_on_a_suitable_terrain() {
    let rules = rules();
    let (mut game, holy) = with_religion(&rules);
    let at = game.view().auth.cities().get(holy).unwrap().tile;
    let enhancer = spawn(&mut game, &rules, P0, at, "core:unit/great-prophet");
    game.execute(
        &rules,
        Command::EnhanceReligion {
            unit: enhancer,
            follower: belief(&rules, "core:belief/follower/cathedrals"),
            enhancer: belief(&rules, "core:belief/enhancer/missionary-zeal"),
        },
    )
    .unwrap();
    let hills = rules.terrains.iter().find(|t| t.key == "core:terrain/hill").and_then(|t| t.map_code).unwrap();
    let on_hills = tile(&game, 6, 5);
    game.debug_set_terrain_batch(&rules, &[(on_hills, classic_rules::ids::TerrainId(hills))]);
    let prophet = spawn(&mut game, &rules, P0, on_hills, "core:unit/great-prophet");
    let obs = game.ai_observation(&rules, P0);
    let commands = classic_rules::ai::religion::religion_commands(&obs, &rules);
    assert!(
        !commands.iter().any(|c| matches!(c, Command::CreateGreatImprovement { unit } if *unit == prophet)),
        "no Holy Site on hills: {commands:?}"
    );
    let step = commands
        .iter()
        .find_map(|c| match c {
            Command::MarchTo { unit, destination } if *unit == prophet => Some(*destination),
            _ => None,
        })
        .expect("the prophet steps to a tile that suits the site");
    assert_ne!(step, on_hills);
    // On suitable land (the step it takes) it builds, and the core accepts.
    let mut game2 = game;
    let level = tile(&game2, 4, 5);
    let second = spawn(&mut game2, &rules, P0, level, "core:unit/great-prophet");
    let obs = game2.ai_observation(&rules, P0);
    let commands = classic_rules::ai::religion::religion_commands(&obs, &rules);
    let site = commands
        .iter()
        .find(|c| matches!(c, Command::CreateGreatImprovement { unit } if *unit == second))
        .cloned()
        .expect("a prophet on suitable own land builds the Holy Site");
    game2.execute(&rules, site).unwrap();
}
