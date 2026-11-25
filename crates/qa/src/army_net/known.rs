//! What the side KNOWS: the only door through which the army net's inputs read the game.
//!
//! The net plays for one side and must see what that side's player sees, the way the main AI reads
//! (`classic-rules/src/knowledge/observation.rs`): another player's unit only when its tile is in sight, and then its
//! kind, tile, owner and hit points (`kernel::ai::observation::ObservedUnit`) and nothing private (moves left, strikes
//! made, stance, level, experience); another player's city when it is in sight (owner, population, its health bar) or
//! as last sighted (owner, population); a tile only when the side has explored it, as the side last saw it; the
//! combat preview only as the interface shows it (`PlayerQueries::attack_preview`).
//!
//! `Known` holds the game privately and hands out `SeenUnit` / `SeenCity` and plain numbers, so the code that builds
//! the inputs (`observe`, the planes) cannot name the full state: a `UnitCore` of a foreign unit or a `CityCore` of a
//! foreign city never reaches it. The test `crates/qa/tests/army_net_known.rs` holds two guards: the observation does
//! not change when everything the side does not know is changed (non-interference), and the input modules do not
//! mention the game's full state at all (a source check).

use classic_rules::combat::city::{city_strength, hit_points, max_hit_points};
use classic_rules::ids::{PlayerId, TileId, UnitHandle, UnitKindId};
use classic_rules::knowledge::actions::unit_actions;
use classic_rules::knowledge::public::PlayerQueries;
use classic_rules::knowledge::PlayerView;
use classic_rules::{ClassicGame, Command, CompiledRuleset, GameState};
use kernel::ids::EntityId;
use kernel::world::unit::UnitCore;

use super::observe::{R_ALLY, R_BARB, R_CS, R_OWN, R_PEACE, R_WAR};
use super::{MapWindow, S_CH, S_REL, W};

/// The version of what the net observes. 1: the full state of the game (the first nets, up to the check of 25 Nov 2026);
/// 2: only what the side knows (this module). Rows, records and the dumps carry it; the gate tools refuse to compare
/// rows of different versions (`tools/gatestats`, as for the rule version).
pub const OBS_VERSION: u8 = 2;

/// A unit of another player the side sees: what the game shows of it (`ObservedUnit`) and its identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SeenUnit {
    pub handle: UnitHandle,
    pub id: EntityId,
    pub tile: TileId,
    pub owner: PlayerId,
    pub kind: UnitKindId,
    pub hp: u16,
}

/// A city of another player the side knows: in sight now (`health` is the health bar, hit points over the most
/// they can be) or as last sighted (`health` None). Strength and "has struck" are not shown by the game.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SeenCity {
    pub tile: TileId,
    pub owner: PlayerId,
    pub population: u32,
    pub health: Option<f32>,
}

/// One of the side's own cities: everything.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OwnCity {
    pub tile: TileId,
    pub population: u32,
    pub health: f32,
    pub strength: i64,
    pub struck: bool,
}

/// The ground of a tile the side has explored (its terrain is static; the feature is as last seen).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Ground {
    pub hills: bool,
    pub mountain: bool,
    pub water: bool,
    pub rough: bool,
}

pub struct Known<'a> {
    game: &'a GameState,
    rules: &'a CompiledRuleset,
    side: PlayerId,
    view: PlayerView<'a>,
}

impl<'a> Known<'a> {
    pub fn new(game: &'a GameState, rules: &'a CompiledRuleset, side: PlayerId) -> Known<'a> {
        Known { game, rules, side, view: game.player_view(rules, side) }
    }

    pub fn side(&self) -> PlayerId {
        self.side
    }

    pub fn rules(&self) -> &'a CompiledRuleset {
        self.rules
    }

    // ---- geometry (public: the map's size and shape) ----

    pub fn dist(&self, a: TileId, b: TileId) -> i32 {
        self.game.view().auth.map().dimensions.distance(a, b).unwrap_or(99)
    }

    /// Column and row of a tile (offset coordinates of the map).
    pub fn col_row(&self, t: TileId) -> (f32, f32) {
        let w = self.game.view().auth.map().dimensions.width as u32;
        ((t.0 % w) as f32, (t.0 / w) as f32)
    }

    pub fn tile_count(&self) -> usize {
        self.game.view().auth.map().tile_count()
    }

    pub fn player_count(&self) -> u8 {
        self.game.view().auth.players().count() as u8
    }

    pub fn round(&self) -> u32 {
        self.game.view().auth.turn().round
    }

    // ---- units ----

    /// The side's own units, by id: everything the side knows of its own.
    pub fn own_units(&self) -> Vec<(UnitHandle, UnitCore)> {
        let auth = self.game.view().auth;
        let mut v: Vec<(UnitHandle, UnitCore)> = auth.indexes().units_of(self.side).iter().filter_map(|h| auth.units().get(*h).map(|u| (*h, *u.core))).collect();
        v.sort_by_key(|(_, c)| c.id);
        v
    }

    /// One own unit.
    pub fn own_core(&self, h: UnitHandle) -> Option<UnitCore> {
        self.game.view().auth.units().get(h).map(|u| *u.core).filter(|c| c.owner == self.side)
    }

    /// The units of `owner` (not the side) the side sees now, by id.
    pub fn seen_units(&self, owner: PlayerId) -> Vec<SeenUnit> {
        debug_assert!(owner != self.side, "own units are not seen but known: own_units");
        let auth = self.game.view().auth;
        let mut v: Vec<SeenUnit> = auth
            .indexes()
            .units_of(owner)
            .iter()
            .filter_map(|h| auth.units().get(*h).map(|u| (*h, *u.core)))
            .filter(|(_, c)| self.view.sees_unit(c))
            .map(|(handle, c)| SeenUnit { handle, id: c.id, tile: c.tile, owner: c.owner, kind: c.kind, hp: c.hp })
            .collect();
        v.sort_by_key(|u| u.id);
        v
    }

    /// The kind of a scene piece the side may know: its own, or one it sees now.
    pub fn kind_if_known(&self, h: UnitHandle, id: EntityId) -> Option<UnitKindId> {
        let core = self.game.view().auth.units().get(h).map(|u| *u.core).filter(|c| c.id == id)?;
        (core.owner == self.side || self.view.sees_unit(&core)).then_some(core.kind)
    }

    /// The first unit not of the side that the side sees on `t`.
    pub fn seen_unit_on(&self, t: TileId) -> Option<SeenUnit> {
        let auth = self.game.view().auth;
        auth.indexes().units_on(t).iter().filter_map(|h| auth.units().get(*h).map(|u| (*h, *u.core))).filter(|(_, c)| c.owner != self.side && self.view.sees_unit(c)).map(|(handle, c)| SeenUnit { handle, id: c.id, tile: c.tile, owner: c.owner, kind: c.kind, hp: c.hp }).next()
    }

    pub fn promotions_available(&self, h: UnitHandle) -> bool {
        !self.view.promotions_available(h).is_empty()
    }

    // ---- cities ----

    /// The side's own cities, by tile.
    pub fn own_cities(&self) -> Vec<OwnCity> {
        let auth = self.game.view().auth;
        let mut v: Vec<OwnCity> = auth
            .indexes()
            .cities_of(self.side)
            .iter()
            .filter_map(|c| auth.cities().get(*c))
            .map(|c| OwnCity { tile: c.tile, population: c.population, health: f32::from(hit_points(auth, self.rules, c)) / f32::from(max_hit_points(auth, self.rules, c).max(1)), strength: city_strength(auth, self.rules, c), struck: c.strike_made })
            .collect();
        v.sort_by_key(|c| c.tile.0);
        v
    }

    /// The cities of `owner` (not the side) the side knows, by tile: in sight now, or as last sighted.
    pub fn foreign_cities(&self, owner: PlayerId) -> Vec<SeenCity> {
        debug_assert!(owner != self.side, "own cities are not seen but known: own_cities");
        let auth = self.game.view().auth;
        let mut v: Vec<SeenCity> = Vec::new();
        for h in auth.indexes().cities_of(owner).iter() {
            let Some(c) = auth.cities().get(*h) else { continue };
            if self.view.is_visible(c.tile) {
                v.push(SeenCity { tile: c.tile, owner, population: c.population, health: Some(f32::from(hit_points(auth, self.rules, c)) / f32::from(max_hit_points(auth, self.rules, c).max(1))) });
            }
        }
        for (tile, s) in self.view.remembered_cities() {
            if s.owner == owner {
                v.push(SeenCity { tile, owner, population: s.seen.as_ref().map_or(0, |x| x.population), health: None });
            }
        }
        v.sort_by_key(|c| c.tile.0);
        v
    }

    /// The owner of the city the side knows on `t` (own and visible ones as they are, the others as last sighted).
    pub fn city_owner_on(&self, t: TileId) -> Option<PlayerId> {
        if self.view.is_visible(t) {
            let auth = self.game.view().auth;
            return auth.indexes().city_on_tile(t).and_then(|c| auth.cities().get(c)).map(|c| c.owner);
        }
        self.view.remembered_cities().find(|(x, _)| *x == t).map(|(_, s)| s.owner)
    }

    // ---- relations ----

    /// The relation of `owner`'s pieces to the side (`observe::relation`): the side's own diplomacy (wars, pacts,
    /// friendships) and the public kind of the player; whether `owner` is at war with `foe` only for a nation the side
    /// has met.
    pub fn relation(&self, foe: PlayerId, owner: PlayerId) -> u8 {
        use classic_rules::world::player::ClassicPlayers;
        let auth = self.game.view().auth;
        if owner == self.side {
            return R_OWN;
        }
        if auth.players().is_barbarian(owner) {
            return R_BARB;
        }
        if auth.diplomacy().at_war(self.side, owner) {
            return R_WAR;
        }
        if auth.players().is_city_state(owner) {
            return R_CS;
        }
        let round = auth.turn().round;
        let rel = auth.module().relations();
        let met = self.view.contact(owner).met_round.is_some();
        let co_belligerent = met && owner != foe && !auth.players().is_barbarian(foe) && auth.diplomacy().at_war(owner, foe);
        if co_belligerent || rel.has_pact(self.side, owner, round) || rel.are_friends(self.side, owner, round) {
            return R_ALLY;
        }
        R_PEACE
    }

    // ---- tiles ----

    pub fn is_visible(&self, t: TileId) -> bool {
        self.view.is_visible(t)
    }

    /// Has the side explored `t`?
    pub fn is_known(&self, t: TileId) -> bool {
        self.view.tile(t).snapshot().is_some()
    }

    /// The ground of an explored tile (None: unknown).
    pub fn ground(&self, t: TileId) -> Option<Ground> {
        let snap = *self.view.tile(t).snapshot()?;
        let kinds = self.view.terrain_kinds();
        let hills = kinds.is_hills(snap.terrain.0);
        let mountain = kinds.is_mountains(snap.terrain.0);
        let wonder = snap.natural_wonder.and_then(|w| self.rules.natural_wonders.get(w.0 as usize)).map(|w| w.rough);
        let rough = hills || mountain || wonder.unwrap_or_else(|| snap.feature.and_then(|f| self.rules.features.get(f.0 as usize)).is_some_and(|d| d.rough));
        Some(Ground { hills, mountain, water: kinds.is_water(snap.terrain.0), rough })
    }

    /// The defence the ground of a tile gives `defender`, in percent (`combat::terrain::tile_defense_percent` over the
    /// explored tile as the side last saw it; an unknown tile gives 0).
    pub fn defence(&self, t: TileId, defender: PlayerId) -> i64 {
        let Some(snap) = self.view.tile(t).snapshot().copied() else { return 0 };
        let g = self.ground(t).unwrap_or_default();
        let mut percent = classic_rules::combat::terrain::ground_defense_percent(self.rules, g.hills || g.mountain, snap.feature);
        if snap.owner.is_none() || snap.owner == Some(defender) {
            if let Some(def) = snap.improvement.filter(|_| !self.view.known_pillaged(t)).and_then(|i| self.rules.improvements.get(i.0 as usize)) {
                percent += i64::from(def.defense_pct);
            }
        }
        percent
    }

    /// Does the explored tile `t` lie in the side's own land (as last seen)?
    pub fn own_land(&self, t: TileId) -> bool {
        self.view.tile(t).snapshot().is_some_and(|s| s.owner == Some(self.side))
    }

    // ---- what the game answers to a player ----

    /// The interface's combat preview of the side's `unit` on the visible `target` (from where it stands, or from
    /// `from`): (damage dealt, damage taken, the defender's hit points). None when the game shows none.
    pub fn preview(&self, unit: UnitHandle, from: Option<TileId>, target: TileId) -> Option<(i64, i64, i64)> {
        let p = match from {
            Some(f) => self.view.attack_preview_from(unit, f, target),
            None => self.view.attack_preview(unit, target),
        }?;
        Some((i64::from(p.uncapped_to_defender), i64::from(p.to_attacker.1), i64::from(p.defender_hp)))
    }

    /// The legal orders of the side's `unit` over what it observes within `radius` (`LocalObservation`), and the tiles it
    /// reaches this turn with the move points left there (the interface's move preview), its own tile included.
    ///
    /// The rules' answer "may this unit move there" depends on the world: a tile with a unit the side does not see refuses
    /// the move, so the list itself would tell the net where the hidden units stand. The question is therefore asked of
    /// the world as the side sees it: the foreign units it does not see within reach are taken off a copy of the game (the
    /// harness's `debug_remove_unit`) before the rules answer. A move the real world then refuses is refused when the net
    /// plays it (the seat takes the next best candidate), as a player's order into the fog would be.
    pub fn legal(&self, unit: UnitHandle, radius: i32) -> Option<(Vec<Command>, Vec<(TileId, u16)>)> {
        let me = self.own_core(unit)?;
        let hidden: Vec<UnitHandle> = {
            let auth = self.game.view().auth;
            auth.units()
                .iter_handles()
                .filter(|(_, c)| c.owner != self.side && !self.view.sees_unit(c) && self.dist(c.tile, me.tile) <= radius + 4)
                .map(|(h, _)| h)
                .collect()
        };
        let sanitized;
        let game: &GameState = if hidden.is_empty() {
            self.game
        } else {
            let mut g = self.game.clone();
            for h in hidden {
                g.debug_remove_unit(self.rules, h);
            }
            sanitized = g;
            &sanitized
        };
        let (near, _) = game.ai_local_observation(self.rules, self.side, unit, radius)?;
        let commands = unit_actions(&near);
        let reach = crate::trainers::act::reachable(game, self.rules, self.side, unit);
        Some((commands, reach))
    }

    /// The owner of what a strike at the visible tile `t` hits: the city's, else the first unit's not of the side.
    pub fn target_owner(&self, t: TileId) -> Option<PlayerId> {
        if !self.view.is_visible(t) {
            return None;
        }
        if let Some(o) = self.city_owner_on(t) {
            return Some(o);
        }
        self.seen_unit_on(t).map(|u| u.owner)
    }

    /// The health shown at the visible tile `t`: a city's hit points (true) or the first foreign unit's (false).
    pub fn hp_at(&self, t: TileId) -> Option<(i64, bool)> {
        if !self.view.is_visible(t) {
            return None;
        }
        let auth = self.game.view().auth;
        if let Some(c) = auth.indexes().city_on_tile(t).and_then(|c| auth.cities().get(c)) {
            return Some((i64::from(hit_points(auth, self.rules, c)), true));
        }
        self.seen_unit_on(t).map(|u| (i64::from(u.hp), false))
    }

    /// The tile a command of the side aims at (its target is in sight when the game offers it).
    pub fn target_tile(&self, cmd: &Command) -> Option<TileId> {
        let auth = self.game.view().auth;
        match *cmd {
            Command::AttackUnit { defender, .. } => auth.units().get(defender).map(|u| u.core.tile),
            Command::AttackCity { city, .. } => auth.cities().get(city).map(|c| c.tile),
            Command::RangedAttack { target, .. } | Command::AirSweep { target, .. } => Some(target),
            _ => None,
        }
        .filter(|t| self.view.is_visible(*t))
    }

    // ---- the planes of the map window ----

    /// The static planes of the window (`army_net::statics_of`) over the tiles the side has explored.
    pub fn statics(&self, win: &MapWindow, foe: PlayerId, objective: TileId, refuge: Option<TileId>) -> Vec<f32> {
        let map = self.game.view().auth.map();
        let mut v = vec![0f32; W * W * S_CH];
        for i in 0..W * W {
            let Some(t) = win.tile(i) else { continue };
            let f = &mut v[i * S_CH..(i + 1) * S_CH];
            f[0] = 1.0;
            f[12] = f32::from(u8::from(t == objective));
            f[13] = f32::from(u8::from(Some(t) == refuge));
            let Some(snap) = self.view.tile(t).snapshot().copied() else { continue };
            let g = self.ground(t).unwrap_or_default();
            let working = snap.improvement.filter(|_| !self.view.known_pillaged(t));
            let citadel = working.and_then(|x| self.rules.improvements.get(x.0 as usize)).is_some_and(|d| d.nearby_enemy_damage > 0);
            let river = map.dimensions.neighbors(t).into_iter().any(|n| map.river_between(t, n));
            let city = self.city_owner_on(t);
            f[1] = f32::from(u8::from(g.hills));
            f[2] = f32::from(u8::from(g.mountain));
            f[3] = f32::from(u8::from(g.water));
            f[4] = f32::from(u8::from(g.rough));
            f[5] = super::observe::f16_round(self.defence(t, self.side) as f32 / 100.0);
            f[6] = f32::from(u8::from(river));
            f[7] = f32::from(u8::from(snap.route != kernel::world::map::Route::None));
            f[8] = f32::from(u8::from(snap.owner == Some(self.side)));
            f[9] = f32::from(u8::from(snap.owner == Some(foe)));
            f[10] = f32::from(u8::from(working.is_some()));
            f[11] = f32::from(u8::from(citadel));
            f[14] = f32::from(u8::from(city == Some(self.side)));
            f[15] = f32::from(u8::from(city == Some(foe)));
        }
        v
    }

    /// The relation planes of the window (`army_net::statics_rel_of`) over the explored tiles.
    pub fn statics_rel(&self, win: &MapWindow, foe: PlayerId) -> Vec<f32> {
        let mut v = vec![0f32; W * W * S_REL];
        for i in 0..W * W {
            let Some(t) = win.tile(i) else { continue };
            let f = &mut v[i * S_REL..(i + 1) * S_REL];
            let Some(snap) = self.view.tile(t).snapshot().copied() else { continue };
            if let Some(owner) = snap.owner.filter(|o| *o != self.side && *o != foe) {
                let k = match self.relation(foe, owner) {
                    R_ALLY => Some(0),
                    R_PEACE => Some(1),
                    R_WAR => Some(2),
                    R_CS => Some(3),
                    R_BARB => Some(4),
                    _ => None,
                };
                if let Some(k) = k {
                    f[k] = 1.0;
                }
            }
            f[5] = f32::from(u8::from(self.city_owner_on(t).is_some_and(|o| o != self.side && o != foe)));
        }
        v
    }
}
