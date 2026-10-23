//! Regressions of the effect consumers on the classic ruleset: an adopted
//! policy fires its triggered effects at the owner's events (Total War's
//! experience of trained units), luxury happiness counts the copies a
//! player has left (imports count, a given-away only copy does not).

use classic_rules::ClassicGame;
use classic_content::core::core_rules;
use classic_rules::derived::happiness::player_happiness;
use classic_rules::ids::{PlayerId, PolicyId, ProductionItem, ResourceId, TechId, TileId, UnitKindId};
use classic_rules::math::HexCoord;
use classic_rules::world::relations::DealItem;
use classic_rules::{compile_rules, Command, CompiledRuleset, GameConfig, GameState};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

fn world() -> (GameState, CompiledRuleset) {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = GameState::new(&rules, GameConfig { players: 2, width: 30, height: 20, rng_seed: 5 });
    let at = |game: &GameState, q, r| game.view().auth.map().dimensions.to_tile(HexCoord { q, r }).unwrap();
    game.debug_found_city(&rules, P0, at(&game, 5, 5));
    game.debug_found_city(&rules, P1, at(&game, 20, 12));
    game.debug_meet_all();
    (game, rules)
}

fn city_tile(game: &GameState, player: PlayerId) -> TileId {
    let auth = game.view().auth;
    let city = auth.indexes().cities_of(player)[0];
    auth.cities().get(city).unwrap().tile
}

#[test]
fn an_adopted_policy_fires_its_unit_trained_effects() {
    let (mut game, rules) = world();
    let total_war = PolicyId(rules.module.policies.iter().position(|p| p.key == "core:policy/autocracy/total-war").unwrap() as u16);
    game.debug_adopt_policy(&rules, P0, total_war);
    let warrior = UnitKindId(rules.units.iter().position(|u| u.key == "core:unit/warrior").unwrap() as u16);
    let city = game.view().auth.indexes().cities_of(P0)[0];
    game.execute(&rules, Command::EnqueueProduction { player: P0, city, item: ProductionItem::Unit(warrior) }).unwrap();
    game.debug_earn(P0, 1000);
    game.execute(&rules, Command::HurryProduction { player: P0, city }).unwrap();
    let auth = game.view().auth;
    let trained = auth
        .indexes()
        .units_of(P0)
        .iter()
        .map(|u| *auth.units().get(*u).unwrap().core)
        .find(|c| c.kind == warrior)
        .expect("trained");
    assert_eq!(trained.experience, 12, "Total War: +12 experience for new military units");
}

#[test]
fn a_luxury_given_in_a_deal_moves_its_happiness() {
    let (mut game, rules) = world();
    let writing = TechId(rules.technologies.iter().position(|t| t.key == "core:tech/writing").unwrap() as u16);
    for p in [P0, P1] {
        game.debug_learn(&rules, p, writing);
    }
    let wine = ResourceId(rules.resources.iter().position(|r| r.key == "core:resource/wine").unwrap() as u16);
    let tile = city_tile(&game, P1);
    game.debug_set_resource_batch(&rules, &[(tile, wine)]);
    let luxuries = |game: &GameState, p| player_happiness(game.view().auth, &rules, p).luxuries;
    let (mine, theirs) = (luxuries(&game, P0), luxuries(&game, P1));
    for items in [
        vec![(P0, DealItem::Embassy), (P1, DealItem::Embassy)],
        vec![(P1, DealItem::Resource { resource: wine, amount: 1 })],
    ] {
        let id = game.view().auth.module().relations().next_offer_id();
        game.execute(&rules, Command::ProposeDeal { from: P0, to: P1, items }).unwrap();
        game.execute(&rules, Command::RespondDeal { player: P1, offer: id, accept: true }).unwrap();
    }
    assert!(luxuries(&game, P0) > mine, "the import counts (Civ V getNumResourceAvailable)");
    assert!(luxuries(&game, P1) < theirs, "the only copy was given away");
}
