use super::*;
use crate::ids::{BuildingKindId, TileId};
use crate::metrics::WorkCounters;
use crate::rules::content::Yields;
use crate::rules::CompiledRuleset;
use crate::transaction::journal::{ChangeJournal, TileChange};
use crate::ids::{PlayerId, TerrainId, UnitKindId};
use crate::world::state::GameConfig;
use crate::{Command, GameState};

fn setup() -> (GameState, CompiledRuleset) {
    // Two buildings from data: hall (+5 production) and exchange (+50 %).
    let rules = crate::fixtures::compile_ron(
        r#"(
        buildings: [
            (id: "core:building/exchange", cost: 80, effects: [(scope: City, selector: SourceCity, modifier: AddYieldPercent(yield_kind: Production, percent: 50))]),
            (id: "core:building/hall", cost: 40, effects: [(scope: City, selector: SourceCity, modifier: AddYield(yield_kind: Production, amount: 5))]),
        ],
    )"#,
    );
    assert_eq!(rules.buildings[0].key, "core:building/exchange");
    assert_eq!(rules.buildings[1].key, "core:building/hall");
    let game = GameState::new(
        &rules,
        GameConfig {
            players: 2,
            width: 16,
            height: 16,
            rng_seed: 0,
        },
    );
    (game, rules)
}

fn tile(game: &GameState, q: i32, r: i32) -> TileId {
    let dims = game.view().auth.map().dimensions;
    dims.to_tile(crate::math::HexCoord { q, r }).unwrap()
}

#[test]
fn founded_and_building_match_rebuild() {
    let (mut game, rules) = setup();
    let city = game.debug_found_city(&rules, PlayerId(0), tile(&game, 4, 4));
    game.validate(&rules).unwrap();
    let base = game.derived().yields.of(city).unwrap().production;
    game.debug_add_building(&rules, city, BuildingKindId(1));
    game.validate(&rules).unwrap();
    assert_eq!(game.derived().yields.of(city).unwrap().production, base + 5);
    let (fresh, _) = YieldState::rebuild_counted(game.view().auth, &rules);
    assert_eq!(game.derived().yields, fresh);
}

#[test]
fn building_percent_applies_after_flat() {
    // base → flat → %: (base + 5) × 1.5, one rounding.
    let (mut game, rules) = setup();
    let city = game.debug_found_city(&rules, PlayerId(0), tile(&game, 6, 6));
    let base = game.derived().yields.of(city).unwrap().production;
    game.debug_add_building(&rules, city, BuildingKindId(1));
    game.debug_add_building(&rules, city, BuildingKindId(0));
    game.validate(&rules).unwrap();
    let got = game.derived().yields.of(city).unwrap().production;
    assert_eq!(got, (base + 5) * 150 / 100);
}

#[test]
fn move_does_not_touch_yields() {
    let (mut game, rules) = setup();
    game.debug_found_city(&rules, PlayerId(0), tile(&game, 4, 4));
    let from = tile(&game, 8, 8);
    let to = tile(&game, 9, 8);
    let h = game.debug_spawn_unit(&rules, PlayerId(0), from, UnitKindId(0));
    let before = game.derived().yields.clone();
    game.execute(
        &rules,
        Command::MoveUnit {
            unit: h,
            destination: to,
        },
    )
    .unwrap();
    assert_eq!(game.derived().yields, before);
    let mut ys = before.clone();
    let mut j = ChangeJournal::default();
    j.unit_changes
        .push(crate::transaction::journal::UnitChange::Moved { unit: h, from, to });
    let c = YieldState::apply_changes_counted(&mut ys, &j, game.view().auth, &rules);
    assert_eq!(c, WorkCounters::default());
    assert_eq!(ys, before);
}

#[test]
fn tile_change_touches_only_cities_in_three_rings() {
    let (mut game, rules) = setup();
    let ct = tile(&game, 4, 4);
    game.debug_found_city(&rules, PlayerId(0), ct);
    for i in 0..60u32 {
        let t = TileId(i * 7 % 256);
        if game.view().auth.indexes().city_on_tile(t).is_some() {
            continue;
        }
        game.debug_found_city(&rules, PlayerId((i % 2) as u8), t);
    }
    let mut ys = YieldState::rebuild(game.view().auth, &rules);
    let mut j = ChangeJournal::default();
    j.tile_changes.push(TileChange::OwnerChanged {
        tile: ct,
        from: None,
        to: Some(PlayerId(1)),
    });
    let c = YieldState::apply_changes_counted(&mut ys, &j, game.view().auth, &rules);
    // 37 lookups of the 3-ring disc and the cities found there, not all.
    let cities = game.view().auth.cities().len() as u64;
    assert!(
        c.entities_visited < cities,
        "entities={} of {cities}",
        c.entities_visited
    );
    let (fresh, _) = YieldState::rebuild_counted(game.view().auth, &rules);
    assert_eq!(ys, fresh);
}

#[test]
fn empire_totals_follow_cities() {
    let (mut game, rules) = setup();
    let c0 = game.debug_found_city(&rules, PlayerId(0), tile(&game, 2, 2));
    let c1 = game.debug_found_city(&rules, PlayerId(0), tile(&game, 10, 10));
    let y = game.derived().yields.clone();
    let expect = y.of(c0).unwrap().plus(y.of(c1).unwrap());
    assert_eq!(y.of_player(PlayerId(0)), expect);
    assert_eq!(y.of_player(PlayerId(1)), Yields::default());
    game.debug_add_building(&rules, c0, BuildingKindId(1));
    let y2 = game.derived().yields.clone();
    assert_eq!(y2.of_player(PlayerId(0)).production, expect.production + 5);
    game.validate(&rules).unwrap();
}

#[test]
fn city_works_its_best_tile_and_the_centre() {
    // Centre on grassland (2F → min 2F 1P), one plains tile (1F 1P), the
    // rest desert (nothing): a size-1 city works the centre and the plains.
    let (mut game, rules) = setup();
    let centre = tile(&game, 8, 8);
    let plains = tile(&game, 9, 8);
    let desert: Vec<_> =
        crate::math::tiles_in_hex_radius(&game.view().auth.map().dimensions, centre, 3)
            .into_iter()
            .filter(|t| *t != centre && *t != plains)
            .map(|t| (t, TerrainId(3)))
            .collect();
    game.debug_set_terrain_batch(&rules, &desert);
    game.debug_set_terrain_batch(&rules, &[(plains, TerrainId(0)), (centre, TerrainId(1))]);
    let city = game.debug_found_city(&rules, PlayerId(0), centre);
    let y = game.derived().yields.of(city).unwrap();
    // Centre 2F 1P + plains 1F 1P; science 1 per citizen.
    assert_eq!((y.food, y.production, y.science), (3, 2, 1));
    game.execute(
        &rules,
        Command::ToggleWorkedTile {
            player: PlayerId(0),
            city,
            tile: plains,
        },
    )
    .unwrap();
    let y2 = game.derived().yields.of(city).unwrap();
    // The citizen moves to a desert tile (0) — the plains tile is off work.
    assert_eq!((y2.food, y2.production), (2, 1));
    game.validate(&rules).unwrap();
}
