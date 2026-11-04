//! Clicks on tiles that hold own units (`stack_click.rs`): an order onto a civilian, the
//! selection of a stacked civilian, the enemy civilian's capture.
use classic_rules::ids::{PlayerId, TileId, UnitHandle, UnitKindId};
use classic_rules::math::tiles_in_hex_radius;
use classic_rules::ClassicGame;

use super::{MapButton, Session};
use crate::ui::screens::{game_rules, prepare_match, MapSize, MatchRequest};

fn new_match(seed: u64) -> Session {
    let request = MatchRequest { nation: "core:nation/rome".into(), seed, size: MapSize::Small, players: 3, water: 25 };
    Session::new(prepare_match(request).expect("world").game, game_rules())
}

/// A match whose start units are cleared away: the test places its own units on open
/// land (`ring`: three free land tiles in a row beside each other, all revealed).
struct Field {
    session: Session,
    ring: [TileId; 3],
}

fn kind(session: &Session, key: &str) -> UnitKindId {
    UnitKindId(session.rules().units.iter().position(|u| u.key == key).unwrap() as u16)
}

fn field() -> Field {
    let mut session = new_match(7);
    let start = {
        let unit = session.selection().selected_unit().unwrap();
        session.game().view().auth.units().get(unit).unwrap().core.tile
    };
    let ring = {
        let auth = session.game().view().auth;
        let dims = auth.map().dimensions;
        let land: Vec<TileId> = tiles_in_hex_radius(&dims, start, 4)
            .into_iter()
            .filter(|t| classic_rules::navigation::cost::enter_cost_on_map(auth.map(), *t).is_some() && auth.indexes().units_on(*t).is_empty())
            .collect();
        // Three land tiles a, b, c with a-b and b-c adjacent.
        let adjacent = |x: TileId, y: TileId| dims.distance(x, y) == Some(1);
        let mut found = None;
        'search: for &b in &land {
            let near: Vec<TileId> = land.iter().copied().filter(|t| adjacent(*t, b)).collect();
            for &a in &near {
                for &c in &near {
                    if a != c && !adjacent(a, c) {
                        found = Some([a, b, c]);
                        break 'search;
                    }
                }
            }
        }
        found.expect("three land tiles in a row")
    };
    {
        let (game, _) = session.harness_game();
        let dims = game.view().auth.map().dimensions;
        let all: Vec<TileId> = ring.iter().flat_map(|t| tiles_in_hex_radius(&dims, *t, 2)).collect();
        game.debug_reveal(PlayerId(0), &all);
    }
    session.refresh_after_harness();
    Field { session, ring }
}

impl Field {
    fn spawn(&mut self, owner: u8, key: &str, tile: TileId) -> UnitHandle {
        let kind = kind(&self.session, key);
        let (game, rules) = self.session.harness_game();
        let unit = game.debug_spawn_unit(&rules, PlayerId(owner), tile, kind);
        self.session.refresh_after_harness();
        unit
    }
    fn tile_of(&self, unit: UnitHandle) -> Option<TileId> {
        self.session.game().view().auth.units().get(unit).map(|u| u.core.tile)
    }
    fn owner_of(&self, unit: UnitHandle) -> Option<PlayerId> {
        self.session.game().view().auth.units().get(unit).map(|u| u.core.owner)
    }
    fn selected(&self) -> Option<UnitHandle> {
        self.session.selection().selected_unit()
    }
    fn click(&mut self, tile: TileId) {
        self.session.click(Some(tile), MapButton::Primary);
    }
}

const WARRIOR: &str = "core:unit/warrior";
const WORKER: &str = "core:unit/worker";

/// A military unit selected and a click on a tile with an own civilian: the order is given,
/// the unit stands with the civilian, nobody is lost.
#[test]
fn a_click_on_an_own_civilian_orders_the_selected_military_unit_there() {
    let mut f = field();
    let [a, b, _] = f.ring;
    let warrior = f.spawn(0, WARRIOR, a);
    let worker = f.spawn(0, WORKER, b);
    f.session.select_unit(warrior);
    let preview = f.session.path_preview(Some(b.0)).expect("the route is drawn over the civilian's tile");
    assert_eq!(preview.tiles.last(), Some(&b.0));
    f.click(b);
    assert_eq!(f.tile_of(warrior), Some(b), "the warrior walked onto the worker's tile");
    assert_eq!(f.tile_of(worker), Some(b));
    assert_eq!(f.selected(), Some(warrior), "the warrior stays selected");
    f.session.game().validate(f.session.rules()).unwrap();
}

/// The same holds the other way round: a civilian walks to its escort.
#[test]
fn a_click_on_an_own_military_unit_orders_the_selected_civilian_there() {
    let mut f = field();
    let [a, b, _] = f.ring;
    let worker = f.spawn(0, WORKER, a);
    let warrior = f.spawn(0, WARRIOR, b);
    f.session.select_unit(worker);
    f.click(b);
    assert_eq!(f.tile_of(worker), Some(b));
    assert_eq!(f.tile_of(warrior), Some(b));
}

/// A click that the rules refuse for a move still selects, as it did before: a second unit
/// of the same class.
#[test]
fn a_unit_that_cannot_stop_there_selects_the_one_clicked() {
    let mut f = field();
    let [a, b, _] = f.ring;
    let first = f.spawn(0, WARRIOR, a);
    let second = f.spawn(0, WARRIOR, b);
    f.session.select_unit(first);
    f.click(b);
    assert_eq!(f.tile_of(first), Some(a), "one military unit per tile: no order");
    assert_eq!(f.selected(), Some(second), "the clicked unit is selected");
}

/// A stack is cycled by clicks, the military unit first; a lone unit's tile is as before.
#[test]
fn clicking_a_stacked_tile_again_selects_the_civilian() {
    let mut f = field();
    let [a, b, c] = f.ring;
    // The civilian arrives first so the military unit is not simply the first handle.
    let worker = f.spawn(0, WORKER, b);
    let warrior = f.spawn(0, WARRIOR, b);
    let lone = f.spawn(0, WORKER, a);
    f.session.select_unit(lone);
    f.click(b);
    assert_eq!(f.selected(), Some(warrior), "the lone worker cannot stop beside the other worker: the military unit is picked first");
    f.click(b);
    assert_eq!(f.selected(), Some(worker), "the second click picks the civilian");
    f.click(b);
    assert_eq!(f.selected(), Some(warrior), "and the third comes round");
    // A lone unit: selected by a click, a second click on it changes nothing.
    let single = f.spawn(0, WARRIOR, c);
    f.session.click(None, MapButton::Primary);
    assert_eq!(f.selected(), None);
    f.click(c);
    assert_eq!(f.selected(), Some(single));
    let hash = f.session.world_hash();
    f.click(c);
    assert_eq!(f.selected(), Some(single));
    assert_eq!(f.session.world_hash(), hash);
}

/// A civilian that a left click would be ordered to (or beside a selected unit) is picked by
/// a right click.
#[test]
fn a_right_click_selects_an_own_civilian_that_a_left_click_would_walk_to() {
    let mut f = field();
    let [a, b, _] = f.ring;
    let warrior = f.spawn(0, WARRIOR, a);
    let worker = f.spawn(0, WORKER, b);
    f.session.select_unit(warrior);
    f.session.click(Some(b), MapButton::Secondary);
    assert_eq!(f.selected(), Some(worker));
    assert_eq!(f.tile_of(warrior), Some(a), "nobody moved");
    assert_eq!(f.tile_of(worker), Some(b));
}

/// An enemy civilian is still not a left-click walk (the core refuses the tile); the right
/// click is the attack that captures it.
#[test]
fn an_enemy_civilian_is_captured_by_the_right_click_only() {
    let mut f = field();
    let [a, b, _] = f.ring;
    let warrior = f.spawn(0, WARRIOR, a);
    let foreign = f.spawn(1, WORKER, b);
    {
        let (game, rules) = f.session.harness_game();
        game.debug_force_war(&rules, PlayerId(0), PlayerId(1));
        game.debug_reveal(PlayerId(0), &[a, b]);
    }
    f.session.refresh_after_harness();
    f.session.select_unit(warrior);
    f.click(b);
    assert_eq!(f.tile_of(warrior), Some(a), "a left click does not walk onto the enemy");
    assert_eq!(f.owner_of(foreign), Some(PlayerId(1)));
    f.session.click(Some(b), MapButton::Secondary);
    // The rules: melee captures a civilian or destroys it; either way it is no longer the enemy's.
    assert_ne!(f.owner_of(foreign), Some(PlayerId(1)), "the right click took the civilian");
    f.session.game().validate(f.session.rules()).unwrap();
}
