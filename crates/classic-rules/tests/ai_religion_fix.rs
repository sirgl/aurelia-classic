//! The `religion-fix` variant (classic/docs/ai-religion-fix.md): an
//! inquisitor bought and sent to win back a converted own city, and a
//! missionary sent to the city where its spread pays, not the nearest.

use classic_content::core::core_rules;
use classic_rules::ai::religion_fix;
use classic_rules::ai::value::{PriceModel, Prices, SciencePricing};
use classic_rules::derived::religion::{city_majority, inquisitor_kind, missionary_kind};
use classic_rules::ids::{BeliefId, CityHandle, PlayerId, ProductionItem, TileId, UnitHandle, UnitKindId};
use classic_rules::rules::content::ReligionId;
use classic_rules::world::religion::Creed;
use classic_rules::{compile_rules, ClassicGame, Command, CompiledRuleset, GameConfig, GameState};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

fn rules() -> CompiledRuleset {
    compile_rules(core_rules()).unwrap()
}

fn tile(game: &GameState, q: i32, r: i32) -> TileId {
    let dims = game.view().auth.map().dimensions;
    dims.to_tile(classic_rules::math::HexCoord { q, r }).unwrap()
}

fn belief(rules: &CompiledRuleset, key: &str) -> BeliefId {
    BeliefId(rules.module.beliefs.iter().position(|b| b.key == key).unwrap() as u16)
}

fn kind(rules: &CompiledRuleset, key: &str) -> UnitKindId {
    UnitKindId(rules.units.iter().position(|u| u.key == key).unwrap() as u16)
}

fn spawn(game: &mut GameState, rules: &CompiledRuleset, player: PlayerId, q: i32, r: i32, key: &str) -> UnitHandle {
    let at = tile(game, q, r);
    game.debug_spawn_unit(rules, player, at, kind(rules, key))
}

fn found(game: &mut GameState, rules: &CompiledRuleset, player: PlayerId, q: i32, r: i32) -> CityHandle {
    let at = tile(game, q, r);
    game.debug_found_city(rules, player, at)
}

fn found_religion(game: &mut GameState, rules: &CompiledRuleset, unit: UnitHandle, religion: u16, beliefs: [&str; 3]) {
    game.execute(
        rules,
        Command::FoundReligion {
            unit,
            religion: ReligionId(religion),
            founder: belief(rules, beliefs[0]),
            follower: belief(rules, beliefs[1]),
            pantheon: Some(belief(rules, beliefs[2])),
            bonus: None,
        },
    )
    .unwrap();
}

/// Player 0 founds a religion paying per following city in its capital at
/// (5, 5) and owns a second city at (9, 5); player 1 founds another at
/// (25, 5). Player 0 enhances its religion when `enhanced`.
fn two_religions(rules: &CompiledRuleset, enhanced: bool) -> (GameState, CityHandle, CityHandle) {
    let mut game = GameState::new(rules, GameConfig { players: 2, width: 40, height: 24, rng_seed: 11 });
    let holy = found(&mut game, rules, P0, 5, 5);
    let second = found(&mut game, rules, P0, 9, 5);
    let prophet = spawn(&mut game, rules, P0, 5, 5, "core:unit/great-prophet");
    found_religion(&mut game, rules, prophet, 0, ["core:belief/founder/church-property", "core:belief/follower/pagodas", "core:belief/pantheon/ancestor-worship"]);
    if enhanced {
        let prophet = spawn(&mut game, rules, P0, 5, 5, "core:unit/great-prophet");
        game.execute(
            rules,
            Command::EnhanceReligion {
                unit: prophet,
                follower: belief(rules, "core:belief/follower/cathedrals"),
                enhancer: belief(rules, "core:belief/enhancer/missionary-zeal"),
            },
        )
        .unwrap();
    }
    found(&mut game, rules, P1, 25, 5);
    let rival = spawn(&mut game, rules, P1, 25, 5, "core:unit/great-prophet");
    game.execute(rules, Command::EndTurn { player: P0 }).unwrap();
    found_religion(&mut game, rules, rival, 1, ["core:belief/founder/tithe", "core:belief/follower/mosques", "core:belief/pantheon/god-of-the-sea"]);
    game.execute(rules, Command::EndTurn { player: P1 }).unwrap();
    (game, holy, second)
}

/// On player 1's turn its missionaries spread its religion into `city` from
/// (q, r), `times` spreads (each while the city does not follow it yet).
fn convert(game: &mut GameState, rules: &CompiledRuleset, city: CityHandle, q: i32, r: i32, times: u32) {
    game.execute(rules, Command::EndTurn { player: P0 }).unwrap();
    for _ in 0..times {
        let missionary = spawn(game, rules, P1, q, r, "core:unit/missionary");
        game.execute(rules, Command::SpreadReligion { unit: missionary }).unwrap();
        if city_majority(game.view().auth, city) == Some(Creed::Religion(ReligionId(1))) {
            break;
        }
    }
    game.execute(rules, Command::EndTurn { player: P1 }).unwrap();
    assert_eq!(city_majority(game.view().auth, city), Some(Creed::Religion(ReligionId(1))));
}

/// `city` converted to player 1's religion, then grown to ten.
fn held(game: &mut GameState, rules: &CompiledRuleset, city: CityHandle) {
    convert(game, rules, city, 10, 4, 1);
    game.debug_set_population(rules, city, 10);
}

/// Player 0 knows the whole map (the ways to the cities).
fn reveal(game: &mut GameState) {
    let count = game.view().auth.map().dimensions.tile_count();
    let all: Vec<TileId> = (0..count as u32).map(TileId).collect();
    game.debug_reveal(P0, &all);
}

fn commands(game: &GameState, rules: &CompiledRuleset) -> Vec<Command> {
    let obs = game.ai_observation(rules, P0);
    let prices = Prices::of_effects(&obs, rules, PriceModel::Uses, SciencePricing::Reach, true);
    religion_fix::commands(&obs, rules, &prices)
}

fn buys(commands: &[Command], kind: Option<UnitKindId>) -> bool {
    let kind = kind.unwrap();
    commands.iter().any(|c| matches!(c, Command::PurchaseWithFaith { item: ProductionItem::Unit(k), .. } if *k == kind))
}

#[test]
fn the_variant_buys_an_inquisitor_and_wins_back_a_converted_city() {
    let rules = rules();
    let (mut game, holy, second) = two_religions(&rules, true);
    // Our missionary first, then theirs twice: the city follows theirs with
    // our pressure still in it.
    let ours = spawn(&mut game, &rules, P0, 10, 5, "core:unit/missionary");
    game.execute(&rules, Command::SpreadReligion { unit: ours }).unwrap();
    convert(&mut game, &rules, second, 10, 4, 2);
    game.debug_grant_faith(P0, 1000);
    let first = commands(&game, &rules);
    assert!(buys(&first, inquisitor_kind(&rules)), "an inquisitor is bought: {first:?}");
    let buy = first.iter().find(|c| matches!(c, Command::PurchaseWithFaith { .. })).cloned().unwrap();
    assert!(matches!(buy, Command::PurchaseWithFaith { city, .. } if city == holy));
    game.execute(&rules, buy).unwrap();
    let inquisitor = *game.view().auth.indexes().units_of(P0).iter().find(|u| {
        game.view().auth.units().get(**u).is_some_and(|v| v.core.kind == inquisitor_kind(&rules).unwrap())
    }).unwrap();
    let second_tile = game.view().auth.cities().get(second).unwrap().tile;
    let next = commands(&game, &rules);
    assert!(
        next.iter().any(|c| matches!(c, Command::MarchTo { unit, destination } if *unit == inquisitor && *destination == second_tile)),
        "it walks to the converted city: {next:?}"
    );
    // Standing in the city it removes the heresy, and the city is ours again.
    let there = spawn(&mut game, &rules, P0, 9, 5, "core:unit/inquisitor");
    let at_city = commands(&game, &rules);
    assert!(at_city.iter().any(|c| matches!(c, Command::RemoveHeresy { unit } if *unit == there)), "{at_city:?}");
    game.execute(&rules, Command::RemoveHeresy { unit: there }).unwrap();
    assert_eq!(city_majority(game.view().auth, second), Some(Creed::Religion(ReligionId(0))));
}

/// A near own city of ten held by the other religion (its growth went to
/// that religion: one spread of ours does not turn it) and a far own city of
/// atheists (one does); player 0 has faith for a missionary.
fn near_and_far(rules: &CompiledRuleset) -> (GameState, TileId) {
    let (mut game, _, near) = two_religions(rules, false);
    held(&mut game, rules, near);
    let far = found(&mut game, rules, P0, 17, 5);
    let far_tile = game.view().auth.cities().get(far).unwrap().tile;
    game.debug_grant_faith(P0, 1000);
    reveal(&mut game);
    (game, far_tile)
}

#[test]
fn the_variant_sends_a_missionary_where_the_spread_pays() {
    let rules = rules();
    let (mut game, far) = near_and_far(&rules);
    let near = tile(&game, 9, 5);
    let buy = commands(&game, &rules).into_iter().find(|c| matches!(c, Command::PurchaseWithFaith { .. })).expect("a missionary is bought");
    game.execute(&rules, buy).unwrap();
    let missionary_kind = missionary_kind(&rules).unwrap();
    let missionary = *game
        .view()
        .auth
        .indexes()
        .units_of(P0)
        .iter()
        .find(|u| game.view().auth.units().get(**u).is_some_and(|v| v.core.kind == missionary_kind))
        .unwrap();
    let from = game.view().auth.units().get(missionary).unwrap().core.tile;
    let dims = game.view().auth.map().dimensions;
    let march = |list: &[Command]| {
        list.iter().find_map(|c| match c {
            Command::MarchTo { unit, destination } if *unit == missionary => Some(*destination),
            _ => None,
        })
    };
    let list = commands(&game, &rules);
    let step = march(&list).expect("it marches");
    assert!(dims.distance(step, far) < dims.distance(from, far), "toward the far city: {list:?}");
    assert!(dims.distance(from, near) < dims.distance(from, far), "the near city is nearer");
    assert!(!buys(&list, Some(missionary_kind)), "the paying city is claimed: {list:?}");
    // The game's own AI walks to the nearest city that does not follow.
    let obs = game.ai_observation(&rules, P0);
    let standard = classic_rules::ai::religion::religion_commands(&obs, &rules);
    assert_eq!(march(&standard), Some(near), "{standard:?}");
}

#[test]
fn the_variant_buys_a_missionary_only_for_a_city_whose_spread_pays() {
    let rules = rules();
    let (game, _) = near_and_far(&rules);
    let list = commands(&game, &rules);
    assert!(buys(&list, missionary_kind(&rules)), "the far city pays: {list:?}");
    // Without the far city nothing pays a missionary.
    let (mut game, _, near) = two_religions(&rules, false);
    held(&mut game, &rules, near);
    game.debug_grant_faith(P0, 1000);
    reveal(&mut game);
    let list = commands(&game, &rules);
    assert!(!buys(&list, missionary_kind(&rules)), "{list:?}");
}


/// A foreign missionary next to an own city of the religion: an inquisitor
/// is bought there, stands in it, and the missionary's spread is refused.
#[test]
fn the_variant_stations_an_inquisitor_against_a_foreign_missionary() {
    let rules = rules();
    let (mut game, holy, _) = two_religions(&rules, true);
    reveal(&mut game);
    let missionary = spawn(&mut game, &rules, P1, 6, 5, "core:unit/missionary");
    game.debug_grant_faith(P0, 1000);
    let list = commands(&game, &rules);
    let buy = list.iter().find(|c| matches!(c, Command::PurchaseWithFaith { .. })).cloned().expect("an inquisitor is bought");
    assert!(buys(&list, inquisitor_kind(&rules)), "{list:?}");
    assert!(matches!(buy, Command::PurchaseWithFaith { city, .. } if city == holy));
    game.execute(&rules, buy).unwrap();
    let after = commands(&game, &rules);
    assert!(!after.iter().any(|c| matches!(c, Command::MarchTo { .. })), "it stays: {after:?}");
    assert!(!buys(&after, inquisitor_kind(&rules)), "no second guard: {after:?}");
    game.execute(&rules, Command::EndTurn { player: P0 }).unwrap();
    assert!(game.execute(&rules, Command::SpreadReligion { unit: missionary }).is_err(), "the inquisitor blocks it");
    assert_eq!(city_majority(game.view().auth, holy), Some(Creed::Religion(ReligionId(0))));
}
