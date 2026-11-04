//! The second half of the Iroquois trait (Great Warpath): the forests and jungles of their own
//! land are roads for city connections and for the path of a trade route, as they already are
//! for walking (`movement_traits.rs`). Land that is foreign or nobody's does not count.

use classic_content::core::core_rules;
use classic_rules::derived::trade_path::cheapest;
use classic_rules::ids::{CityHandle, FeatureId, PlayerId, TerrainId, TileId};
use classic_rules::math::{HexCoord, MapDimensions};
use classic_rules::rules::stats::TradeDomain;
use classic_rules::world::state::GameConfig;
use classic_rules::{compile_rules, CompiledRuleset, GameState};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);
const PLAINS: TerrainId = TerrainId(1);
const DIMS: MapDimensions = MapDimensions { width: 30, height: 12 };

fn tile(q: i32, r: i32) -> TileId {
    DIMS.to_tile(HexCoord { q, r }).unwrap()
}

fn feature(rules: &CompiledRuleset, key: &str) -> FeatureId {
    FeatureId(rules.features.iter().position(|f| f.key == key).unwrap_or_else(|| panic!("{key}")) as u16)
}

fn nation(rules: &CompiledRuleset, key: &str) -> u8 {
    rules.nations.iter().position(|c| c.key == key).unwrap_or_else(|| panic!("{key}")) as u8
}

/// Two cities of P0 (a nation of the test) at (2,5) and (`far`,5) and a feature `key` on
/// every listed column of the row between them (a city owns the ring around it: the tiles next
/// to it).
fn world(nation_key: &str, far: i32, key: &str, columns: &[i32]) -> (GameState, CompiledRuleset, CityHandle, CityHandle) {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = GameState::new(&rules, GameConfig { players: 2, width: 30, height: 12, rng_seed: 7 });
    let flat: Vec<(TileId, TerrainId)> = (0..30 * 12).map(|i| (TileId(i), PLAINS)).collect();
    game.debug_set_terrain_batch(&rules, &flat);
    game.debug_set_player_nation(P0, nation(&rules, nation_key));
    game.debug_sources_changed(&rules, P0);
    let capital = game.debug_found_city(&rules, P0, tile(2, 5));
    let second = game.debug_found_city(&rules, P0, tile(far, 5));
    let planted: Vec<_> = columns.iter().map(|q| (tile(*q, 5), feature(&rules, key))).collect();
    game.debug_set_feature_batch(&rules, &planted);
    (game, rules, capital, second)
}

fn connected(game: &GameState, rules: &CompiledRuleset, second: CityHandle) -> bool {
    game.player_view(rules, P0).own_connected_cities().contains(&second)
}

/// Gold the connections of P0 pay.
fn connection_gold(game: &GameState, rules: &CompiledRuleset) -> u32 {
    let capital = classic_rules::world::city::capital_of(game.view().auth, rules, P0).unwrap();
    assert_eq!(game.view().auth.cities().get(capital).unwrap().owner, P0);
    classic_rules::derived::maintenance::connection_gold_times100(game.view().auth, rules, P0).max(0) as u32
}

#[test]
fn the_iroquois_connect_cities_through_their_own_forest_and_jungle() {
    for key in ["core:feature/forest", "core:feature/jungle"] {
        let (game, rules, _, second) = world("core:nation/iroquois", 5, key, &[3, 4]);
        assert!(connected(&game, &rules, second), "{key}: the Iroquois connect over their woods");
        assert!(connection_gold(&game, &rules) > 0, "{key}: and the connection pays");
        game.validate(&rules).unwrap();
        let (game, rules, _, second) = world("core:nation/america", 5, key, &[3, 4]);
        assert!(!connected(&game, &rules, second), "{key}: others need a road");
        assert_eq!(connection_gold(&game, &rules), 0, "{key}: and get no connection gold");
    }
}

#[test]
fn one_tile_of_open_ground_breaks_the_connection() {
    let (game, rules, _, second) = world("core:nation/iroquois", 5, "core:feature/forest", &[3]);
    assert!(!connected(&game, &rules, second), "a gap in the woods");
}

#[test]
fn woods_of_nobodys_land_or_a_strangers_do_not_connect() {
    // Eight tiles apart: the middle of the row belongs to nobody.
    let (game, rules, _, second) = world("core:nation/iroquois", 10, "core:feature/forest", &[3, 4, 5, 6, 7, 8, 9]);
    let owners: Vec<_> = (3..=9).map(|q| game.view().auth.map().owner(tile(q, 5)).flatten()).collect();
    assert!(owners.iter().any(|o| o.is_none()), "the row has unowned tiles: {owners:?}");
    assert!(!connected(&game, &rules, second), "unowned woods are not a road");
    // A rival city in the middle of the row: its ring of land is not the Iroquois'.
    let (mut game, rules, _, second) = world("core:nation/iroquois", 8, "core:feature/forest", &[3, 4, 5, 6, 7]);
    game.debug_found_city(&rules, P1, tile(5, 6));
    let foreign = (3..=7).filter(|q| game.view().auth.map().owner(tile(*q, 5)).flatten() == Some(P1)).count();
    assert!(foreign > 0, "the rival owns part of the row");
    assert!(!connected(&game, &rules, second), "woods in unowned land and next to a stranger");
}

/// The path of a trade route: a forest or jungle tile costs a road step whoever owns it, so a
/// short budget reaches farther along the woods (classic/docs/classic/rules.md).
#[test]
fn a_trade_path_enters_woods_at_the_price_of_a_road() {
    let budget = 140;
    // The row 3..=4 is the own land of a second city at 5; the row 7..=8 is nobody's.
    let reach = |nation_key: &str, key: &str, columns: &[i32], to: i32| {
        let (game, rules, capital, _) = world(nation_key, 5, key, columns);
        let origin = game.view().auth.cities().get(capital).unwrap().clone();
        cheapest(game.view().auth, &rules, &origin, tile(to, 5), TradeDomain::Land, budget).is_some()
    };
    for key in ["core:feature/forest", "core:feature/jungle"] {
        assert!(reach("core:nation/iroquois", key, &[3, 4], 4), "{key}: two tiles of woods within two roads' price");
        assert!(!reach("core:nation/america", key, &[3, 4], 4), "{key}: others pay 100 and more per tile");
        assert!(!reach("core:nation/iroquois", key, &[3], 4), "{key}: a plain tile in the row costs the full price");
    }
    // Woods nobody owns (tile 4 beside no city: the second city at 12 owns only 11..=13).
    let (game, rules, capital, _) = world("core:nation/iroquois", 12, "core:feature/forest", &[3, 4]);
    assert_eq!(game.view().auth.map().owner(tile(4, 5)).flatten(), None, "the tile is nobody's");
    let origin = game.view().auth.cities().get(capital).unwrap().clone();
    assert!(cheapest(game.view().auth, &rules, &origin, tile(4, 5), TradeDomain::Land, budget).is_some(), "unowned woods too");
}
