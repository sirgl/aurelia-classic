//! City-states on the classic ruleset (classic/docs/classic/rules.md): influence decay
//! and recovery, friends and allies, maritime food, gold gift formula,
//! tribute, pledges, unit gifts, militaristic unit gifts, quests, war on a
//! city-state and alliance wars, first-contact gold, save/load and the
//! AI's gifts.

use classic_rules::ClassicGame;
use classic_content::core::core_rules;
use classic_rules::ai::city_states::{gift_ceiling, gift_lead, gift_orders, priced_gifts};
use classic_rules::ai::value::{PriceModel, Prices};
use classic_rules::ids::{PlayerId, TileId, UnitKindId};
use classic_rules::math::HexCoord;
use classic_rules::replay::authoritative_hash;
use classic_rules::rules::content::CityStateId;
use classic_rules::world::city_states::{gold_gift_influence, Personality, QuestKind};
use classic_rules::world::player::PlayerKind;
use classic_rules::world::state::GameConfig;
use classic_rules::{compile_rules, Command, CompiledRuleset, GameState};
use classic_rules::world::player::ClassicKind;

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);
const CS: PlayerId = PlayerId(2);

fn tile(game: &GameState, q: i32, r: i32) -> TileId {
    game.view()
        .auth
        .map()
        .dimensions
        .to_tile(HexCoord { q, r })
        .unwrap()
}

fn unit(rules: &CompiledRuleset, key: &str) -> UnitKindId {
    UnitKindId(rules.units.iter().position(|u| u.key == key).unwrap() as u16)
}

fn city_state_id(rules: &CompiledRuleset, key: &str) -> CityStateId {
    CityStateId(rules.module.city_states.iter().position(|c| c.key == key).unwrap() as u16)
}

/// Two nations and one city-state of `key` (neutral), all met.
fn world(key: &str) -> (GameState, CompiledRuleset) {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = GameState::new(
        &rules,
        GameConfig {
            players: 3,
            width: 30,
            height: 20,
            rng_seed: 9,
        },
    );
    game.debug_set_player_kind(CS, PlayerKind::Module(ClassicKind::CityState(city_state_id(&rules, key))));
    game.debug_set_personality(CS, Personality::Neutral);
    game.debug_found_city(&rules, P0, tile(&game, 3, 3));
    game.debug_found_city(&rules, P1, tile(&game, 24, 15));
    game.debug_found_city(&rules, CS, tile(&game, 13, 9));
    game.debug_meet_all();
    (game, rules)
}

fn until_active(game: &mut GameState, rules: &CompiledRuleset, player: PlayerId) {
    for _ in 0..8 {
        let active = game.view().auth.active_player();
        if active == player {
            return;
        }
        game.execute(rules, Command::EndTurn { player: active })
            .unwrap();
    }
    panic!("{player:?} never became active");
}

/// The city-state ends its turn (its influence and quest step runs).
fn city_state_turn(game: &mut GameState, rules: &CompiledRuleset) {
    until_active(game, rules, CS);
    game.execute(rules, Command::EndTurn { player: CS })
        .unwrap();
}

fn influence(game: &GameState, major: PlayerId) -> i32 {
    game.view().auth.module().city_states().influence(CS, major)
}

#[test]
fn influence_decays_to_the_resting_point_and_recovers_from_below() {
    let (mut game, rules) = world("core:city-state/byblos");
    game.debug_set_influence(&rules, CS, P0, 40);
    game.debug_set_influence(&rules, CS, P1, -10);
    city_state_turn(&mut game, &rules);
    assert_eq!((influence(&game, P0), influence(&game, P1)), (39, -9));
    game.debug_set_influence(&rules, CS, P0, 0);
    city_state_turn(&mut game, &rules);
    assert_eq!(influence(&game, P0), 0, "decay stops at the resting point");
    game.debug_set_personality(CS, Personality::Hostile);
    game.debug_set_influence(&rules, CS, P0, 40);
    city_state_turn(&mut game, &rules);
    city_state_turn(&mut game, &rules);
    assert_eq!(influence(&game, P0), 37, "hostile: -1.5 per turn");
}

#[test]
fn the_ally_changes_only_for_strictly_more_influence() {
    let (mut game, rules) = world("core:city-state/byblos");
    let states = |g: &GameState| g.view().auth.module().city_states().ally(CS);
    game.debug_set_influence(&rules, CS, P0, 60);
    assert_eq!(states(&game), Some(P0));
    game.debug_set_influence(&rules, CS, P1, 60);
    assert_eq!(states(&game), Some(P0), "a tie keeps the ally");
    game.debug_set_influence(&rules, CS, P1, 61);
    assert_eq!(states(&game), Some(P1));
    game.debug_set_influence(&rules, CS, P1, 20);
    assert_eq!(states(&game), Some(P0), "the other one above 60 takes over");
}

#[test]
fn a_maritime_friend_gets_food_in_its_capital() {
    let (mut game, rules) = world("core:city-state/byblos");
    let food = |g: &GameState| {
        let obs = g.ai_observation(&rules, P0);
        obs.own_cities[0].income.food_surplus
    };
    let before = food(&game);
    game.debug_set_influence(&rules, CS, P0, 30);
    assert_eq!(food(&game), before + 2, "friends: +2 food in the capital");
    game.debug_set_influence(&rules, CS, P0, 60);
    assert_eq!(food(&game), before + 3, "allies: +1 more in every city");
}

#[test]
fn gold_gifts_follow_the_classic_table() {
    let table = [
        (0, 250, 25),
        (0, 500, 50),
        (0, 1000, 105),
        (100, 250, 20),
        (100, 500, 45),
        (100, 1000, 90),
        (200, 1000, 70), // Unciv: CityStateFunctions.kt, progress turn / 400
        (500, 250, 5),
        (500, 1000, 35),
    ];
    for (round, gold, expected) in table {
        assert_eq!(
            gold_gift_influence(round, gold, false),
            expected,
            "{gold} gold at round {round}"
        );
    }
    let (mut game, rules) = world("core:city-state/byblos");
    game.debug_earn(P0, 600);
    until_active(&mut game, &rules, P0);
    game.execute(
        &rules,
        Command::GiftGold {
            player: P0,
            city_state: CS,
            gold: 500,
        },
    )
    .unwrap();
    assert_eq!(influence(&game, P0), 50);
    assert_eq!(game.view().auth.players().treasury_of(P0), 100);
}

/// A strong neighbour takes tribute; the city-state's influence drops.
#[test]
fn tribute_needs_power_near_the_city_state() {
    let (mut game, rules) = world("core:city-state/byblos");
    until_active(&mut game, &rules, P0);
    let bully = Command::BullyGold {
        player: P0,
        city_state: CS,
    };
    assert!(
        game.execute(&rules, bully.clone()).is_err(),
        "no army: no tribute"
    );
    let swords = unit(&rules, "core:unit/swordsman");
    for (q, r) in [
        (12, 8),
        (13, 8),
        (14, 9),
        (14, 10),
        (12, 10),
        (13, 10),
        (12, 9),
        (11, 9),
    ] {
        game.debug_spawn_unit(&rules, P0, tile(&game, q, r), swords);
    }
    let gold = game.view().auth.players().treasury_of(P0);
    game.execute(&rules, bully.clone()).unwrap();
    assert_eq!(game.view().auth.players().treasury_of(P0), gold + 50);
    assert_eq!(influence(&game, P0), -15);
    assert!(
        game.execute(&rules, bully).is_err(),
        "not twice in ten rounds"
    );
}

#[test]
fn a_pledge_raises_the_resting_point_and_holds_ten_rounds() {
    let (mut game, rules) = world("core:city-state/byblos");
    until_active(&mut game, &rules, P0);
    let pledge = |on| Command::PledgeProtection {
        player: P0,
        city_state: CS,
        pledge: on,
    };
    game.execute(&rules, pledge(true)).unwrap();
    assert_eq!(
        classic_rules::world::city_states::resting_point(game.view().auth, &rules, CS, P0),
        10, // Unciv: DiplomacyManager.kt, protector +10
    );
    city_state_turn(&mut game, &rules);
    city_state_turn(&mut game, &rules);
    assert_eq!(influence(&game, P0), 2, "recovers towards 10");
    until_active(&mut game, &rules, P0);
    assert!(
        game.execute(&rules, pledge(false)).is_err(),
        "too early to withdraw"
    );
}

#[test]
fn a_gifted_unit_joins_the_city_state() {
    let (mut game, rules) = world("core:city-state/byblos");
    let warrior = unit(&rules, "core:unit/warrior");
    let at = tile(&game, 13, 8);
    let gift = game.debug_spawn_unit(&rules, P0, at, warrior);
    until_active(&mut game, &rules, P0);
    game.execute(
        &rules,
        Command::GiftUnit {
            unit: gift,
            city_state: CS,
        },
    )
    .unwrap();
    assert_eq!(influence(&game, P0), 5);
    let auth = game.view().auth;
    assert!(auth.indexes().units_of(P0).is_empty());
    assert_eq!(auth.indexes().units_of(CS).len(), 1);
}

#[test]
fn militaristic_friends_receive_units() {
    let (mut game, rules) = world("core:city-state/almaty");
    game.debug_set_influence(&rules, CS, P0, 200);
    let before = game.view().auth.indexes().units_of(P0).len();
    // Unciv: DiplomacyTurnManager.kt, the first gift waits the full
    // interval: 16..18 rounds for an ally.
    for _ in 0..20 {
        city_state_turn(&mut game, &rules);
    }
    let after = game.view().auth.indexes().units_of(P0).len();
    assert!(
        after > before,
        "a unit within the first period: {before} -> {after}"
    );
}

#[test]
fn clearing_the_quest_camp_pays_fifty_influence() {
    let (mut game, rules) = world("core:city-state/byblos");
    let camp = tile(&game, 16, 9);
    game.debug_place_camp(&rules, camp);
    game.debug_add_quest(CS, P0, QuestKind::KillCamp(camp), None);
    let warrior = game.debug_spawn_unit(
        &rules,
        P0,
        tile(&game, 17, 9),
        unit(&rules, "core:unit/warrior"),
    );
    until_active(&mut game, &rules, P0);
    game.execute(
        &rules,
        Command::MoveUnit {
            unit: warrior,
            destination: camp,
        },
    )
    .unwrap();
    city_state_turn(&mut game, &rules);
    assert_eq!(
        influence(&game, P0),
        50,
        "decay first, then the reward (rule set order)"
    );
    assert!(game.view().auth.module().city_states().quests().is_empty());
}

#[test]
fn war_on_a_city_state_and_the_allys_wars() {
    let (mut game, rules) = world("core:city-state/byblos");
    game.debug_set_influence(&rules, CS, P0, 70);
    until_active(&mut game, &rules, P1);
    game.execute(
        &rules,
        Command::DeclareWar {
            player: P1,
            enemy: P0,
        },
    )
    .unwrap();
    assert!(
        game.view().auth.diplomacy().at_war(CS, P1),
        "the city-state follows its ally"
    );
    assert!(
        game.execute(
            &rules,
            Command::MakePeace {
                player: P1,
                enemy: CS
            }
        )
        .is_err(),
        "no peace while the ally fights"
    );
    let (mut game, rules) = world("core:city-state/byblos");
    game.debug_set_influence(&rules, CS, P0, 40);
    until_active(&mut game, &rules, P0);
    game.execute(
        &rules,
        Command::DeclareWar {
            player: P0,
            enemy: CS,
        },
    )
    .unwrap();
    assert_eq!(influence(&game, P0), -60);
    assert_eq!(game.view().auth.module().city_states().major(P0).minors_attacked, 1);
}

#[test]
fn first_contact_brings_gold() {
    let (mut game, rules) = world("core:city-state/byblos");
    let gold = |g: &GameState, p| g.view().auth.players().treasury_of(p);
    until_active(&mut game, &rules, CS);
    let (g0, g1) = (gold(&game, P0), gold(&game, P1));
    city_state_turn(&mut game, &rules);
    assert_eq!(gold(&game, P0) + gold(&game, P1), g0 + g1 + 30 + 15);
}

#[test]
fn city_state_state_survives_save_and_load() {
    let (mut game, rules) = world("core:city-state/byblos");
    game.debug_set_influence(&rules, CS, P0, 65);
    game.debug_add_quest(CS, P1, QuestKind::Invest, Some(40));
    city_state_turn(&mut game, &rules);
    let save = classic_rules::persistence::encode::encode(&game, &rules);
    let loaded = classic_rules::persistence::decode::decode(&save, &rules).unwrap();
    assert_eq!(authoritative_hash(&loaded), authoritative_hash(&game));
    assert_eq!(
        loaded.view().auth.module().city_states(),
        game.view().auth.module().city_states()
    );
}

#[test]
fn the_ai_gives_gold_when_it_is_rich() {
    let (mut game, rules) = world("core:city-state/byblos");
    game.debug_earn(P0, 1100);
    let orders = gift_orders(&game.ai_observation(&rules, P0));
    assert!(
        matches!(
            orders.as_slice(),
            [Command::GiftGold {
                city_state: CS,
                gold: 500,
                ..
            }]
        ),
        "{orders:?}"
    );
}

/// The priced gifts stop at the influence the next gift is away from the alliance
/// (`gift_ceiling`): below it the gold is offered, above it kept, and a quest for gold
/// keeps the city-state a target; without the cap (`pre-cs-gifts`) the gift stays.
#[test]
fn priced_gifts_stop_where_the_alliance_holds_past_the_next_gift() {
    let (mut game, rules) = world("core:city-state/byblos");
    game.debug_earn(P0, 1100);
    let offered = |game: &GameState, cap: bool| {
        let obs = game.ai_observation(&rules, P0);
        let prices = Prices { gift_cap: cap, ..Prices::of(&obs, &rules, PriceModel::Uses) };
        priced_gifts(&obs, &prices, 500).iter().any(|g| g.city_state == CS)
    };
    let ceiling = {
        let obs = game.ai_observation(&rules, P0);
        let prices = Prices::of(&obs, &rules, PriceModel::Uses);
        gift_ceiling(60, gift_lead(&obs, prices.rounds_left))
    };
    // The lead is at least one round: the ceiling is above the threshold, by the decay of it.
    assert!(ceiling > 60, "{ceiling}");
    game.debug_set_influence(&rules, CS, P0, 30);
    assert!(offered(&game, true), "a friend is courted towards the alliance");
    game.debug_set_influence(&rules, CS, P0, ceiling - 1);
    assert!(offered(&game, true), "an alliance about to lapse is held");
    game.debug_set_influence(&rules, CS, P0, ceiling);
    assert!(!offered(&game, true), "the influence bought would decay unused");
    assert!(offered(&game, false), "the control gives on");
    game.debug_add_quest(CS, P0, QuestKind::GiveGold { gifted: 0 }, Some(40));
    assert!(offered(&game, true), "a quest for gold keeps the city-state a target");
}

/// A cultured friend adds 3 culture a turn to the player's pool (ancient
/// era), an ally 3 more (the culture pool of track B).
#[test]
fn a_cultured_friend_adds_culture_to_the_pool() {
    let (mut game, rules) = world("core:city-state/bogota");
    let pool = |g: &GameState| {
        g.view()
            .auth
            .module()
            .culture()
            .player(P0)
            .map(|c| c.culture)
            .unwrap_or(0)
    };
    until_active(&mut game, &rules, P0);
    let before = pool(&game);
    game.execute(&rules, Command::EndTurn { player: P0 })
        .unwrap();
    let base = pool(&game) - before;
    game.debug_set_influence(&rules, CS, P0, 70);
    until_active(&mut game, &rules, P0);
    let before = pool(&game);
    game.execute(&rules, Command::EndTurn { player: P0 })
        .unwrap();
    assert_eq!(pool(&game) - before, base + 6, "friend 3 + ally 3");
}

fn play_as(game: &mut GameState, rules: &CompiledRuleset, player: PlayerId, nation: &str) {
    let index = rules.nations.iter().position(|c| c.key == nation).unwrap();
    game.debug_set_player_nation(player, index as u8);
}

#[test]
fn greek_influence_decays_at_half_rate_and_recovers_twice_as_fast() {
    let (mut game, rules) = world("core:city-state/byblos");
    play_as(&mut game, &rules, P0, "core:nation/greece");
    game.debug_set_influence(&rules, CS, P0, 40);
    city_state_turn(&mut game, &rules);
    city_state_turn(&mut game, &rules);
    assert_eq!(influence(&game, P0), 39, "-0.5 a turn");
    game.debug_set_influence(&rules, CS, P0, -10);
    city_state_turn(&mut game, &rules);
    assert_eq!(influence(&game, P0), -8, "+2 a turn");
}

#[test]
fn siam_gets_half_again_the_food_of_a_maritime_friend() {
    let (mut game, rules) = world("core:city-state/byblos");
    play_as(&mut game, &rules, P0, "core:nation/siam");
    let food = |g: &GameState| {
        let obs = g.ai_observation(&rules, P0);
        obs.own_cities[0].income.food_surplus
    };
    let before = food(&game);
    game.debug_set_influence(&rules, CS, P0, 30);
    assert_eq!(food(&game), before + 3, "friends: +2 food, +50 %");
}

#[test]
fn consulates_raise_the_resting_point_by_twenty() {
    let (mut game, rules) = world("core:city-state/byblos");
    let policy = |key: &str| {
        classic_rules::ids::PolicyId(rules.module.policies.iter().position(|p| p.key == key).unwrap() as u16)
    };
    let branch = |key: &str| {
        classic_rules::rules::content::PolicyBranchId(
            rules
                .module.policy_branches
                .iter()
                .position(|b| b.key == key)
                .unwrap() as u16,
        )
    };
    let resting = |g: &GameState| {
        classic_rules::world::city_states::resting_point(g.view().auth, &rules, CS, P0)
    };
    assert_eq!(resting(&game), 0);
    let philosophy = rules
        .technologies
        .iter()
        .position(|t| t.key == "core:tech/philosophy")
        .unwrap();
    game.debug_learn(&rules, P0, classic_rules::ids::TechId(philosophy as u16));
    game.debug_grant_culture(P0, 100_000);
    until_active(&mut game, &rules, P0);
    game.execute(
        &rules,
        Command::UnlockPolicyBranch {
            player: P0,
            branch: branch("core:policy/patronage"),
        },
    )
    .unwrap();
    for key in [
        "core:policy/patronage/philanthropy",
        "core:policy/patronage/consulates",
    ] {
        game.execute(
            &rules,
            Command::AdoptPolicy {
                player: P0,
                policy: policy(key),
            },
        )
        .unwrap_or_else(|e| panic!("{key}: {e:?}"));
    }
    assert_eq!(resting(&game), 20);
    city_state_turn(&mut game, &rules);
    assert_eq!(influence(&game, P0), 1, "recovers towards 20");
}

#[test]
fn austria_buys_out_an_ally_of_five_rounds() {
    let (mut game, rules) = world("core:city-state/byblos");
    play_as(&mut game, &rules, P0, "core:nation/austria");
    let warrior = unit(&rules, "core:unit/warrior");
    game.debug_spawn_unit(&rules, CS, tile(&game, 14, 9), warrior);
    game.debug_set_influence(&rules, CS, P0, 80);
    game.debug_set_influence(&rules, CS, P1, 70);
    game.debug_earn(P0, 1000);
    game.debug_earn(P1, 1000);
    let buyout = |player| Command::BuyoutCityState {
        player,
        city_state: CS,
    };
    until_active(&mut game, &rules, P0);
    assert!(game.execute(&rules, buyout(P0)).is_err(), "allied too recently");
    for _ in 0..5 {
        city_state_turn(&mut game, &rules);
    }
    until_active(&mut game, &rules, P1);
    assert!(game.execute(&rules, buyout(P1)).is_err(), "only Austria annexes");
    until_active(&mut game, &rules, P0);
    let cost = classic_rules::world::city_states::buyout_cost(game.view().auth, &rules, CS);
    // Unciv: CityStateFunctions.kt, BaseUnit.kt: 500 + (30 x 40)^0.75 / 20.
    assert_eq!(cost, 510, "500 + a warrior's scrap gold 10, rounded to 5");
    let gold = game.view().auth.players().treasury_of(P0);
    let units = |g: &GameState| g.view().auth.indexes().units_of(P0).len();
    let before = units(&game);
    game.execute(&rules, buyout(P0)).unwrap();
    // P1, a friend, loses the city-state's bonuses with it.
    game.validate(&rules).unwrap();
    assert_eq!(units(&game), before + 1, "the city-state's units are the buyer's");
    let auth = game.view().auth;
    assert_eq!(auth.players().treasury_of(P0), gold - cost);
    assert!(!auth.players().is_alive(CS), "the city-state is gone");
    let city = auth.indexes().cities_of(P0)[1];
    assert_eq!(
        auth.cities().get(city).unwrap().status,
        classic_rules::world::city::CityStatus::AwaitingDecision
    );
    assert_eq!(auth.cities().get(city).unwrap().resistance_turns, 0);
    game.execute(
        &rules,
        Command::DecideCity {
            player: P0,
            city,
            decision: classic_rules::command::CityDecision::Puppet,
        },
    )
    .unwrap();
}

#[test]
fn a_bought_out_city_ends_the_routes_into_it() {
    // A city that changes hands has its trade routes cleared.
    let (mut game, rules) = world("core:city-state/byblos");
    play_as(&mut game, &rules, P0, "core:nation/austria");
    let grass: Vec<_> = (0..600).map(|i| (TileId(i), classic_rules::ids::TerrainId(1))).collect();
    game.debug_set_terrain_batch(&rules, &grass);
    let every: Vec<TileId> = (0..600).map(TileId).collect();
    game.debug_reveal(P0, &every);
    let home = tile(&game, 8, 9);
    game.debug_found_city(&rules, P0, home);
    let husbandry = rules.technologies.iter().position(|t| t.key == "core:tech/animal-husbandry").unwrap();
    game.debug_learn(&rules, P0, classic_rules::ids::TechId(husbandry as u16));
    let caravan = unit(&rules, "core:unit/caravan");
    let trader = game.debug_spawn_unit(&rules, P0, home, caravan);
    game.debug_set_influence(&rules, CS, P0, 80);
    game.debug_earn(P0, 2000);
    until_active(&mut game, &rules, P0);
    let destination = game.view().auth.indexes().cities_of(CS)[0];
    let kind = classic_rules::world::trade::RouteKind::International;
    game.execute(&rules, Command::EstablishTradeRoute { unit: trader, destination, kind }).unwrap();
    for _ in 0..5 {
        city_state_turn(&mut game, &rules);
    }
    until_active(&mut game, &rules, P0);
    game.execute(&rules, Command::BuyoutCityState { player: P0, city_state: CS }).unwrap();
    let auth = game.view().auth;
    assert!(auth.module().trade().routes().is_empty(), "no international route to an own city");
    let own = auth.indexes().units_of(P0);
    let caravans = own.iter().filter(|u| auth.units().get(**u).unwrap().core.kind == caravan);
    assert_eq!(caravans.count(), 1, "the caravan comes back home");
    game.validate(&rules).unwrap();
}

/// A tile of `host`'s land (not its city) next to unowned land: (inside,
/// outside).
fn border(game: &GameState, host: PlayerId) -> (TileId, TileId) {
    let auth = game.view().auth;
    let map = auth.map();
    let dims = map.dimensions;
    (0..map.tile_count() as u32)
        .map(TileId)
        .filter(|t| map.owner(*t).flatten() == Some(host) && auth.indexes().city_on_tile(*t).is_none())
        .find_map(|inside| {
            let outside = dims.neighbors(inside).into_iter().find(|n| map.owner(*n) == Some(None))?;
            Some((inside, outside))
        })
        .expect("a border")
}

fn step(game: &mut GameState, rules: &CompiledRuleset, unit: classic_rules::ids::UnitHandle, to: TileId) -> bool {
    game.execute(rules, Command::MoveUnit { unit, destination: to }).is_ok()
}

fn unit_tile(game: &GameState, unit: classic_rules::ids::UnitHandle) -> TileId {
    game.view().auth.units().get(unit).unwrap().core.tile
}

/// Every nation's units may enter a city-state's land without
/// open borders (classic/docs/classic/rules.md);
/// a nation's land stays closed without its grant.
#[test]
fn every_nation_enters_city_state_land_in_peace() {
    let (mut game, rules) = world("core:city-state/byblos");
    let warrior = unit(&rules, "core:unit/warrior");
    let (inside, outside) = border(&game, CS);
    let scout = game.debug_spawn_unit(&rules, P0, outside, warrior);
    let (closed, before_closed) = border(&game, P1);
    let guest = game.debug_spawn_unit(&rules, P0, before_closed, warrior);
    until_active(&mut game, &rules, P0);
    assert!(!game.view().auth.diplomacy().at_war(P0, CS));
    assert!(step(&mut game, &rules, scout, inside), "the city-state's land is open");
    assert_eq!(unit_tile(&game, scout), inside);
    assert!(!step(&mut game, &rules, guest, closed), "a nation's land is not");
    let obs = game.ai_observation(&rules, P0);
    let open = |p: PlayerId| obs.relations.iter().find(|r| r.other == p).unwrap().they_grant_entry;
    assert!(open(CS) && !open(P1), "the AI sees the same borders");
}

/// Peace with a city-state no longer sends units home from its land: they
/// may stand there in peace.
#[test]
fn peace_with_a_city_state_keeps_units_on_its_land() {
    let (mut game, rules) = world("core:city-state/byblos");
    let warrior = unit(&rules, "core:unit/warrior");
    let (inside, _) = border(&game, CS);
    until_active(&mut game, &rules, P0);
    game.execute(&rules, Command::DeclareWar { player: P0, enemy: CS }).unwrap();
    let raider = game.debug_spawn_unit(&rules, P0, inside, warrior);
    game.execute(&rules, Command::MakePeace { player: P0, enemy: CS }).unwrap();
    assert_eq!(unit_tile(&game, raider), inside);
}
