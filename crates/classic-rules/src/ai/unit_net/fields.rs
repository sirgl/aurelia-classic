//! Layer 0 and layer 1 of the unit net (design sections 1–2): the theatre
//! as the player sees it now — its 24 input channels per tile, from the
//! rules' own quantities (the reach of our units by the move preview, the
//! enemy's reach and blows by `ai::battlefield`, the strikes by the combat
//! previews) — and the 14 maps the seven hand kernels make of them: force
//! density and balance, the front, the chokes, the siege ring and the free
//! wall tiles, the flanks and cut-off units, the safe rear, the group's
//! cohesion.
//!
//! Built from a fresh observation at each decision of a theatre unit; owned
//! by nobody between decisions. Every number is an integer in thousandths;
//! tiles in `TileId` order, ties to the lower index: deterministic.

use std::collections::BTreeMap;

use super::math::{mul, reach_chance, sigmoid, step, tanh, ONE};
use super::weights::{conv as cw, fields as fw};
use super::Milli;
use crate::ai::battlefield::{Battlefield, KindStats, Posture, Target};
use crate::ai::planner::{role_of, AiRole, TurnBrief};
use crate::ai::planner_map::{step_points, KnownMap};
use crate::ai::state::SeenCity;
use crate::combat::damage::{melee_damage, ranged_damage, MeleeSide, Roll};
use crate::ids::{EntityId, PlayerId, TileId, UnitHandle};
use crate::knowledge::observation::{ObservedCity, ObservedUnit, OwnUnit, Relation};
use crate::knowledge::public::PlayerQueries;
use crate::knowledge::view::PlayerView;
use crate::rules::CompiledRuleset;
use crate::world::player::{ClassicKind, PlayerKind};
use crate::world::unit::{MOVE_DENOMINATOR, UNIT_MAX_HP};

/// The channels (design section 1), by index.
pub mod ch {
    pub const PASSABLE: usize = 0;
    pub const MOVE_COST: usize = 1;
    pub const DEFENCE: usize = 2;
    pub const BLOCKS_SIGHT: usize = 3;
    pub const RIVER: usize = 4;
    pub const TERRITORY: usize = 5;
    pub const OURS: usize = 6;
    pub const OURS_RANGED: usize = 7;
    pub const THEIRS: usize = 8;
    pub const THEIRS_RANGED: usize = 9;
    pub const HIDDEN: usize = 10;
    pub const CITY: usize = 11;
    pub const OUR_REACH1: usize = 12;
    pub const OUR_REACH2: usize = 13;
    pub const THEIR_REACH1: usize = 14;
    pub const THEIR_REACH2: usize = 15;
    pub const ZOC: usize = 16;
    pub const DANGER: usize = 17;
    pub const SUPPORT: usize = 18;
    pub const FOCUS: usize = 19;
    pub const OP_TARGET: usize = 20;
    pub const TARGET_TURNS: usize = 21;
    pub const HEAL: usize = 22;
    pub const PILLAGE: usize = 23;
    pub const N: usize = 24;
}

/// The maps of the hand kernels (design section 2), by index.
pub mod mp {
    pub const OURS2: usize = 0;
    pub const THEIRS2: usize = 1;
    pub const BALANCE: usize = 2;
    pub const FRONT: usize = 3;
    pub const FRONT_ENEMY: usize = 4;
    pub const CHOKE: usize = 5;
    pub const GAP: usize = 6;
    pub const SIEGE: usize = 7;
    pub const CUT_OFF: usize = 8;
    pub const OUR_CUT: usize = 9;
    pub const SAFE2: usize = 10;
    pub const COHESION: usize = 11;
    pub const LONELY: usize = 12;
    pub const FLOW: usize = 13;
    pub const N: usize = 14;
}

/// Is `owner` hostile to the player: at war, or the barbarians?
pub fn hostile(relations: &[Relation], owner: PlayerId) -> bool {
    relations.iter().find(|r| r.other == owner).is_some_and(|r| r.at_war || r.kind == PlayerKind::Module(ClassicKind::Barbarian))
}

/// Is `owner` the barbarians?
pub fn barbarian(relations: &[Relation], owner: PlayerId) -> bool {
    relations.iter().find(|r| r.other == owner).is_some_and(|r| r.kind == PlayerKind::Module(ClassicKind::Barbarian))
}

/// A land military unit of ours that fights (not recon? recon fights too).
pub fn fighter(rules: &CompiledRuleset, kind: crate::ids::UnitKindId) -> bool {
    role_of(rules, kind) == AiRole::Military && rules.units.get(kind.0 as usize).is_some_and(|d| d.domain == crate::rules::content::Domain::Land && (d.strength > 0 || d.ranged_strength > 0))
}

/// What the theatre is made of: the player's units and the foes it sees
/// now, the hostile cities (in sight and remembered), the operations'
/// targets.
#[derive(Debug, Clone, Default)]
pub struct Picture {
    pub own: Vec<OwnUnit>,
    pub foes: Vec<ObservedUnit>,
    pub cities: Vec<ObservedCity>,
    pub remembered: Vec<SeenCity>,
    pub own_cities: Vec<TileId>,
    /// The operations' target tiles (ch20).
    pub targets: Vec<TileId>,
}

/// One possible blow of one of our units on one target this turn: from
/// which tile, its outcome by the rules' preview.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Strike {
    pub unit: EntityId,
    pub handle: UnitHandle,
    /// Index into `Theatre::field.targets`.
    pub target: usize,
    pub target_tile: TileId,
    pub from: TileId,
    pub ranged: bool,
    /// (min, expected, max) damage to the target, capped by its health.
    pub dealt: (u16, u16, u16),
    /// Expected damage our unit takes in the blow.
    pub taken: u16,
    /// The danger on `from` for this unit next turn (hit points).
    pub danger: u16,
    /// A melee blow of a capture-capable unit on a city.
    pub captures: bool,
    /// The unit needs a move to `from` first.
    pub moves: bool,
}

/// The theatre: the battlefield model, the channels and maps of the window,
/// our units' reach and their strikes.
pub struct Theatre<'v, 'a> {
    pub field: Battlefield<'v, 'a>,
    pub player: PlayerId,
    pub tiles: Vec<TileId>,
    pub ch: Vec<[Milli; ch::N]>,
    pub maps: Vec<[Milli; mp::N]>,
    pub own: Vec<OwnUnit>,
    /// Per own unit (same order): the tiles it reaches this turn and the
    /// move points left there.
    pub reach: Vec<Vec<(TileId, u16)>>,
    pub strikes: Vec<Strike>,
    /// The posture of our units where they stand.
    pub posture: Posture,
    /// Per target (same order as `field.targets`, N1): the chance a hostile city falls this
    /// turn to everything our units can still strike it with (each unit's best blow), when one
    /// of them can take it; 0 for a unit, a remembered city, a city nobody can take.
    pub fall: Vec<Milli>,
    nbr: Vec<[usize; 6]>,
}

const NONE: usize = usize::MAX;

impl<'v, 'a> Theatre<'v, 'a> {
    /// The theatre of `picture` over the player's view now.
    pub fn build(view: &'v PlayerView<'a>, rules: &'a CompiledRuleset, brief: &TurnBrief, relations: &[Relation], picture: &Picture) -> Self {
        let dims = view.dimensions();
        let player = view.player();
        let own: Vec<OwnUnit> = picture.own.iter().filter(|u| fighter(rules, u.kind) || rules.is_civilian(u.kind) || role_of(rules, u.kind) != AiRole::Military).copied().collect();
        let mut centres: Vec<TileId> = own.iter().map(|u| u.tile).chain(picture.foes.iter().map(|u| u.tile)).chain(picture.targets.iter().copied()).collect();
        centres.sort();
        centres.dedup();
        let field = Battlefield::build(view, rules, brief, view_round(brief), &picture.foes, &picture.cities, &picture.remembered, &picture.own_cities, &centres, fw::RADIUS, None);
        let tiles: Vec<TileId> = field.tiles.keys().copied().collect();
        let index: BTreeMap<TileId, usize> = tiles.iter().enumerate().map(|(i, t)| (*t, i)).collect();
        let nbr: Vec<[usize; 6]> = tiles
            .iter()
            .map(|t| {
                let mut out = [NONE; 6];
                for (k, n) in dims.neighbors(*t).into_iter().enumerate().take(6) {
                    out[k] = index.get(&n).copied().unwrap_or(NONE);
                }
                out
            })
            .collect();
        let reach: Vec<Vec<(TileId, u16)>> = own.iter().map(|u| if u.moves_left > 0 && fighter(rules, u.kind) { view.own_reachable(u.handle) } else { vec![(u.tile, 0)] }).collect();
        let posture = Posture {
            bodies: own.iter().map(|u| u.tile).collect(),
            sources: own.iter().filter(|u| fighter(rules, u.kind)).map(|u| u.tile).chain(picture.own_cities.iter().copied()).collect(),
        };
        let mut theatre = Self { field, player, tiles, ch: Vec::new(), maps: Vec::new(), own, reach, strikes: Vec::new(), posture, fall: Vec::new(), nbr };
        theatre.strikes = theatre.find_strikes(view, rules);
        // N1: the cities' fall chances from the strikes, then the strikes' danger again with
        // the city fire they leave.
        theatre.fall = theatre.falls();
        if theatre.fall.iter().any(|f| *f > 0) {
            let dangers: Vec<u16> = theatre.strikes.iter().map(|s| theatre.own_unit(s.unit).map_or(s.danger, |(_, u)| theatre.unit_danger(rules, u, s.from))).collect();
            for (s, d) in theatre.strikes.iter_mut().zip(dangers) {
                s.danger = d;
            }
        }
        theatre.ch = theatre.channels(view, rules, relations, picture, &index);
        theatre.maps = theatre.kernels(rules, &index, picture);
        theatre
    }

    pub fn index(&self, tile: TileId) -> Option<usize> {
        self.tiles.binary_search(&tile).ok()
    }

    /// A channel at a tile (0 outside the window).
    pub fn chan(&self, tile: TileId, c: usize) -> Milli {
        self.index(tile).map_or(0, |i| self.ch[i][c])
    }

    /// A map at a tile (0 outside the window).
    pub fn map(&self, tile: TileId, m: usize) -> Milli {
        self.index(tile).map_or(0, |i| self.maps[i][m])
    }

    /// The own unit with this id.
    pub fn own_unit(&self, id: EntityId) -> Option<(usize, &OwnUnit)> {
        self.own.iter().enumerate().find(|(_, u)| u.id == id)
    }

    /// The expected damage the foes deal next turn to a unit of ours on
    /// `tile` with `hp` (each foe once, the cities too), under `posture`
    /// (our bodies and zones of control): `D_u` of the design.
    pub fn danger_on(&self, me: &KindStats, hp: u16, tile: TileId, fortified: bool, posture: &Posture) -> u16 {
        let mut sum: u32 = 0;
        for (i, foe) in self.field.targets.iter().enumerate() {
            if foe.hp == 0 || !self.field.can_strike(i, tile, posture) {
                continue;
            }
            let blow = self.field.blow_by(foe, me, hp, tile, fortified);
            sum += u32::from(if foe.is_city() { self.city_fire(i, blow, tile, posture) } else { blow });
        }
        sum = sum.saturating_add(u32::from(self.field.fog_blow(tile)));
        sum.min(u32::from(u16::MAX)) as u16
    }

    /// N1, the city's fire on a unit of ours on `tile` whose blow from the city is `blow`:
    /// the rules give a city one strike a turn on one unit in its range, so the unit carries
    /// its share of that strike among our units in the range (the posture's bodies, itself
    /// counted once), and none of it when the city falls this turn (`fall`).
    pub fn city_fire(&self, index: usize, blow: u16, tile: TileId, posture: &Posture) -> u16 {
        let Some(city) = self.field.targets.get(index) else { return blow };
        let range = city.stats.range.max(1);
        let others = posture.bodies.iter().filter(|b| **b != tile && self.field.distance(city.tile, **b) <= range).count() as i64;
        let standing = ONE - self.fall.get(index).copied().unwrap_or(0);
        (i64::from(blow) * standing / ONE / (others + 1)) as u16
    }

    /// N1: each target's chance to fall this turn (`fall`): a city that one of our units can
    /// take, against the sum of every unit's best blow on it (the fire the turn can still
    /// bring); 0 for the rest.
    fn falls(&self) -> Vec<Milli> {
        self.field
            .targets
            .iter()
            .enumerate()
            .map(|(ti, t)| {
                if !t.is_city() || t.remembered || t.hp == 0 || !self.strikes.iter().any(|s| s.target == ti && s.captures) {
                    return 0;
                }
                let mut best: BTreeMap<EntityId, (u16, u16, u16)> = BTreeMap::new();
                for s in self.strikes.iter().filter(|s| s.target == ti) {
                    let e = best.entry(s.unit).or_insert((0, 0, 0));
                    if s.dealt.1 > e.1 {
                        *e = s.dealt;
                    }
                }
                let sum = best.values().fold((0i64, 0i64, 0i64), |a, d| (a.0 + i64::from(d.0), a.1 + i64::from(d.1), a.2 + i64::from(d.2)));
                reach_chance(sum.0, sum.1, sum.2, i64::from(t.hp))
            })
            .collect()
    }

    /// The danger of an own unit standing where it stands.
    pub fn unit_danger(&self, rules: &CompiledRuleset, unit: &OwnUnit, tile: TileId) -> u16 {
        let me = KindStats::of(rules, unit.kind);
        let posture = self.moved_posture(unit.id, tile);
        self.danger_on(&me, unit.hp, tile, unit.fortified && tile == unit.tile, &posture)
    }

    /// The posture with one of our units moved to `tile`.
    pub fn moved_posture(&self, unit: EntityId, tile: TileId) -> Posture {
        let mut posture = self.posture.clone();
        if let Some((i, u)) = self.own_unit(unit) {
            let from = u.tile;
            if let Some(b) = posture.bodies.iter_mut().find(|b| **b == from) {
                *b = tile;
            }
            if fighter(self.field.rules, self.own[i].kind) {
                if let Some(s) = posture.sources.iter_mut().find(|s| **s == from) {
                    *s = tile;
                }
            }
        }
        posture
    }

    // ---- the strikes ----

    /// Every blow of every own fighter with an attack left on a target in
    /// sight this turn: from where it stands and from each tile it reaches
    /// with a move left (and the set-up's moves for a unit that sets up),
    /// by the rules' preview; per (unit, target) the best few tiles.
    fn find_strikes(&self, view: &PlayerView<'_>, rules: &CompiledRuleset) -> Vec<Strike> {
        let dims = self.field.dims;
        let mut out = Vec::new();
        for (ui, u) in self.own.iter().enumerate() {
            if !u.can_attack || !fighter(rules, u.kind) {
                continue;
            }
            let stats = KindStats::of(rules, u.kind);
            let reach_of = if stats.is_ranged() { stats.range } else { 1 };
            let sets_up = view.must_set_up_first(u.handle) || u.stance == crate::world::unit::Stance::SetUp;
            let keep = if sets_up { MOVE_DENOMINATOR } else { 0 };
            let captures = view.captures_cities(u.handle);
            for (ti, t) in self.field.targets.iter().enumerate() {
                if t.remembered || t.hp == 0 {
                    continue;
                }
                let Some(d) = dims.distance(u.tile, t.tile) else { continue };
                if d > reach_of + (u.moves_left / MOVE_DENOMINATOR.max(1)) as i32 + 1 {
                    continue;
                }
                let mut options: Vec<Strike> = Vec::new();
                for &(from, left) in &self.reach[ui] {
                    let here = from == u.tile;
                    if !here && left <= keep {
                        continue;
                    }
                    if dims.distance(from, t.tile).is_none_or(|d| d > reach_of || d == 0) {
                        continue;
                    }
                    let preview = if here { view.attack_preview(u.handle, t.tile) } else { view.attack_preview_from(u.handle, from, t.tile) };
                    let Some(p) = preview else { continue };
                    let danger = self.unit_danger(rules, u, from);
                    let melee = !stats.is_ranged();
                    options.push(Strike {
                        unit: u.id,
                        handle: u.handle,
                        target: ti,
                        target_tile: t.tile,
                        from,
                        ranged: !melee,
                        dealt: p.to_defender,
                        taken: p.to_attacker.1,
                        danger,
                        captures: melee && t.is_city() && captures,
                        moves: !here,
                    });
                }
                // The best few: by expected damage, then the least danger, then staying put.
                options.sort_by_key(|s| (std::cmp::Reverse(s.dealt.1), s.danger, s.moves, s.from));
                options.truncate(super::weights::plan::STRIKE_TILES);
                out.extend(options);
            }
        }
        out
    }

    // ---- the channels ----

    fn channels(&self, view: &PlayerView<'_>, rules: &CompiledRuleset, relations: &[Relation], picture: &Picture, index: &BTreeMap<TileId, usize>) -> Vec<[Milli; ch::N]> {
        let dims = self.field.dims;
        let n = self.tiles.len();
        let mut out = vec![[0; ch::N]; n];
        let map = KnownMap::land(view);
        let kinds = view.terrain_kinds();
        // Terrain, territory, healing, pillage.
        for (i, &t) in self.tiles.iter().enumerate() {
            let info = self.field.tile(t).copied();
            let known = view.tile(t).snapshot().copied();
            let c = &mut out[i];
            match known {
                None => c[ch::PASSABLE] = ONE / 2,
                Some(s) => {
                    let water = kinds.is_water(s.terrain.0);
                    let mountain = kinds.is_mountains(s.terrain.0);
                    c[ch::PASSABLE] = if water || mountain { 0 } else { ONE };
                    c[ch::RIVER] = if s.river { ONE } else { 0 };
                    c[ch::TERRITORY] = match s.owner {
                        Some(o) if o == self.player => ONE,
                        Some(_) => -ONE,
                        None => 0,
                    };
                    let foreign_improvement = s.improvement.is_some_and(|imp| {
                        rules.improvements.get(imp.0 as usize).is_some_and(|d| !matches!(d.kind, crate::rules::content::ImprovementKind::AncientRuins | crate::classic::content::BARBARIAN_CAMP))
                    });
                    if foreign_improvement && !s.city && s.owner.is_some_and(|o| hostile(relations, o)) && !view.known_pillaged(t) {
                        c[ch::PILLAGE] = fw::PILLAGE;
                    }
                }
            }
            if let Some(info) = info {
                c[ch::DEFENCE] = self.field.defence_percent(t, self.player, false) * ONE / fw::DEFENCE_DIV;
                c[ch::BLOCKS_SIGHT] = if info.ground > 0 { ONE } else { 0 };
            }
            // The cheapest step into the tile from a neighbour.
            let cost = dims.neighbors(t).into_iter().filter_map(|from| step_points(&map, from, t)).min();
            c[ch::MOVE_COST] = cost.map_or(ONE, |p| i64::from(p) * ONE / (fw::COST_DIV * i64::from(MOVE_DENOMINATOR)));
            c[ch::HEAL] = self.field.rest_heal(t) * ONE / fw::HEAL_DIV;
        }
        // Forces.
        let force = |kind, hp: u16, ranged: bool| -> Milli {
            let def = rules.units.get(kind as usize);
            let strength = def.map_or(0, |d| if ranged { if d.range > 0 { i64::from(d.ranged_strength) } else { 0 } } else { i64::from(d.strength) });
            strength * i64::from(hp) * ONE / (i64::from(UNIT_MAX_HP) * fw::FORCE_DIV)
        };
        for u in self.own.iter().filter(|u| fighter(rules, u.kind)) {
            if let Some(&i) = index.get(&u.tile) {
                out[i][ch::OURS] += force(u.kind.0, u.hp, false);
                out[i][ch::OURS_RANGED] += force(u.kind.0, u.hp, true);
            }
        }
        for f in &picture.foes {
            if let Some(&i) = index.get(&f.tile) {
                out[i][ch::THEIRS] += force(f.kind.0, f.hp, false);
                out[i][ch::THEIRS_RANGED] += force(f.kind.0, f.hp, true);
            }
        }
        // The fog: a share of a typical foe near hostile cities and foes seen.
        let typical: i64 = {
            let strengths: Vec<i64> = picture.foes.iter().filter_map(|f| rules.units.get(f.kind.0 as usize)).map(|d| i64::from(d.strength.max(d.ranged_strength))).collect();
            if strengths.is_empty() { 20 } else { strengths.iter().sum::<i64>() / strengths.len() as i64 }
        };
        let hostile_points: Vec<TileId> = picture.foes.iter().map(|f| f.tile).chain(picture.cities.iter().map(|c| c.tile)).chain(picture.remembered.iter().map(|c| c.tile)).collect();
        for (i, &t) in self.tiles.iter().enumerate() {
            let visible = self.field.tile(t).is_some_and(|x| x.visible);
            if !visible && out[i][ch::PASSABLE] > 0 && hostile_points.iter().any(|p| dims.distance(*p, t).is_some_and(|d| d <= fw::FOG_REACH)) {
                out[i][ch::HIDDEN] = fw::FOG_SHARE * typical * ONE / (fw::FORCE_DIV * ONE);
            }
        }
        // Cities.
        for c in &picture.own_cities {
            if let Some(&i) = index.get(c) {
                out[i][ch::CITY] = ONE;
            }
        }
        for c in &picture.cities {
            if let Some(&i) = index.get(&c.tile) {
                out[i][ch::CITY] = -(i64::from(c.hp) * ONE / fw::CITY_FULL_HP.max(i64::from(c.hp))).max(1);
            }
        }
        for c in &picture.remembered {
            if let Some(&i) = index.get(&c.tile) {
                out[i][ch::CITY] = -ONE;
            }
        }
        // Our reach: this turn exactly, the next widened by the moves.
        for (ui, u) in self.own.iter().enumerate() {
            if !fighter(rules, u.kind) {
                continue;
            }
            let moves = rules.units.get(u.kind.0 as usize).map_or(1, |d| i32::from(d.movement));
            for &(t, _) in &self.reach[ui] {
                if let Some(&i) = index.get(&t) {
                    out[i][ch::OUR_REACH1] = ONE;
                }
                for w in crate::math::hex_disc(dims, t, (moves - fw::SECOND_TURN_SLACK).max(0)) {
                    if let Some(&i) = index.get(&w) {
                        if out[i][ch::PASSABLE] > 0 {
                            out[i][ch::OUR_REACH2] = ONE;
                        }
                    }
                }
            }
        }
        // Their reach and zones of control.
        for (ti, t) in self.field.targets.iter().enumerate() {
            if t.is_city() || !t.stats.military {
                continue;
            }
            if let Some(&i) = index.get(&t.tile) {
                for &n in &self.nbr[i] {
                    if n != NONE {
                        out[n][ch::ZOC] = ONE;
                    }
                }
            }
            let tiles_a_turn = (t.stats.movement / u32::from(MOVE_DENOMINATOR)) as i32;
            for node in self.field.reach[ti].nodes() {
                if let Some(&i) = index.get(&node.tile) {
                    out[i][ch::THEIR_REACH1] = ONE;
                }
                for w in crate::math::hex_disc(dims, node.tile, (tiles_a_turn - fw::SECOND_TURN_SLACK).max(0)) {
                    if let Some(&i) = index.get(&w) {
                        if out[i][ch::PASSABLE] > 0 {
                            out[i][ch::THEIR_REACH2] = ONE;
                        }
                    }
                }
            }
        }
        // Danger (their blows on our typical defender) and support (ours on their typical unit).
        let our_typical = self.own.iter().filter(|u| fighter(rules, u.kind)).max_by_key(|u| (KindStats::of(rules, u.kind).melee, std::cmp::Reverse(u.id))).map(|u| KindStats::of(rules, u.kind));
        let their_typical = picture.foes.iter().filter_map(|f| rules.units.get(f.kind.0 as usize).map(|_| KindStats::of(rules, f.kind))).max_by_key(|s| s.melee).or(our_typical);
        if let Some(me) = our_typical {
            for (i, &t) in self.tiles.iter().enumerate() {
                if out[i][ch::PASSABLE] == 0 {
                    continue;
                }
                let d = self.danger_on(&me, UNIT_MAX_HP, t, false, &Posture::default());
                out[i][ch::DANGER] = i64::from(d) * ONE / fw::HP_DIV;
            }
        }
        if let Some(them) = their_typical {
            for (ui, u) in self.own.iter().enumerate() {
                if !u.can_attack || !fighter(rules, u.kind) {
                    continue;
                }
                let me = KindStats::of(rules, u.kind);
                let range = if me.is_ranged() { me.range } else { 1 };
                let mut hit: Vec<usize> = Vec::new();
                for &(from, _) in &self.reach[ui] {
                    for w in crate::math::hex_disc(dims, from, range) {
                        if let Some(&i) = index.get(&w) {
                            if w != from && !hit.contains(&i) {
                                hit.push(i);
                            }
                        }
                    }
                }
                for i in hit {
                    let t = self.tiles[i];
                    let defence = (them.melee * (100 + self.field.defence_percent(t, PlayerId(u8::MAX), false).max(-90)) / 100).max(100);
                    let wounded = UNIT_MAX_HP.saturating_sub(u.hp);
                    let blow = if me.is_ranged() { ranged_damage(me.ranged, defence, wounded, 0, Roll::Expected) } else { melee_damage(me.melee, defence, wounded, 0, Roll::Expected, MeleeSide::UnitVsUnit) };
                    out[i][ch::SUPPORT] += i64::from(blow) * ONE / fw::HP_DIV;
                }
            }
        }
        // Focus: our summed best blows on a target minus its health.
        let mut best: BTreeMap<(usize, EntityId), u16> = BTreeMap::new();
        for s in &self.strikes {
            let e = best.entry((s.target, s.unit)).or_insert(0);
            *e = (*e).max(s.dealt.1);
        }
        for (ti, t) in self.field.targets.iter().enumerate() {
            if t.remembered {
                continue;
            }
            let sum: i64 = best.iter().filter(|((target, _), _)| *target == ti).map(|(_, d)| i64::from(*d)).sum();
            if let Some(&i) = index.get(&t.tile) {
                out[i][ch::FOCUS] = (sum - i64::from(t.hp)) * ONE / fw::HP_DIV;
            }
        }
        // The operations' targets and the turns to the nearest.
        for (i, &t) in self.tiles.iter().enumerate() {
            if picture.targets.contains(&t) {
                out[i][ch::OP_TARGET] = ONE;
            }
            let d = picture.targets.iter().filter_map(|g| dims.distance(*g, t)).min();
            out[i][ch::TARGET_TURNS] = d.map_or(0, |d| i64::from(d) * ONE / 2);
        }
        out
    }

    // ---- the kernels ----

    /// conv(x; c, n) over the window: c·x(t) + n·Σ neighbours.
    fn conv(&self, x: &[Milli], c: Milli, n: Milli) -> Vec<Milli> {
        (0..x.len())
            .map(|i| {
                let sum: Milli = self.nbr[i].iter().filter(|&&j| j != NONE).map(|&j| x[j]).sum();
                mul(c, x[i]) + mul(n, sum)
            })
            .collect()
    }

    fn column(rows: &[[Milli; ch::N]], c: usize) -> Vec<Milli> {
        rows.iter().map(|r| r[c]).collect()
    }

    fn kernels(&self, rules: &CompiledRuleset, index: &BTreeMap<TileId, usize>, picture: &Picture) -> Vec<[Milli; mp::N]> {
        let n = self.tiles.len();
        let chs = &self.ch;
        let col = |c| Self::column(chs, c);
        let add = |a: &[Milli], b: &[Milli]| a.iter().zip(b).map(|(x, y)| x + y).collect::<Vec<_>>();
        let mut out = vec![[0; mp::N]; n];
        // V1: density and balance.
        let ours = add(&col(ch::OURS), &col(ch::OURS_RANGED));
        let theirs = add(&add(&col(ch::THEIRS), &col(ch::THEIRS_RANGED)), &col(ch::HIDDEN));
        let ours2 = self.conv(&self.conv(&ours, cw::DENSITY_CENTRE, cw::DENSITY_NEIGHBOUR), cw::DENSITY_CENTRE, cw::DENSITY_NEIGHBOUR);
        let theirs2 = self.conv(&self.conv(&theirs, cw::DENSITY_CENTRE, cw::DENSITY_NEIGHBOUR), cw::DENSITY_CENTRE, cw::DENSITY_NEIGHBOUR);
        // V2: the front.
        let ours_near = self.conv(&ours, ONE, ONE);
        let theirs_touch = self.conv(&add(&col(ch::THEIR_REACH1), &col(ch::ZOC)), 0, ONE);
        let theirs_near = self.conv(&add(&col(ch::THEIRS), &col(ch::THEIRS_RANGED)), ONE, ONE);
        let ours_touch = self.conv(&col(ch::OUR_REACH1), 0, ONE);
        // V3: chokes.
        let open = self.conv(&col(ch::PASSABLE), 0, ONE);
        let flow = self.conv(&col(ch::THEIR_REACH2), 0, ONE);
        let between = self.between(picture);
        // V5: counts of units next to a tile.
        let mut foe_count = vec![0i64; n];
        let mut own_count = vec![0i64; n];
        for f in &picture.foes {
            if rules.units.get(f.kind.0 as usize).is_some_and(|d| d.strength > 0 || d.ranged_strength > 0) {
                if let Some(&i) = index.get(&f.tile) {
                    foe_count[i] += ONE;
                }
            }
        }
        for u in self.own.iter().filter(|u| fighter(rules, u.kind)) {
            if let Some(&i) = index.get(&u.tile) {
                own_count[i] += ONE;
            }
        }
        let foe_side = self.conv(&foe_count, 0, ONE);
        let own_side = self.conv(&own_count, 0, ONE);
        for i in 0..n {
            let o = &mut out[i];
            o[mp::OURS2] = ours2[i] / cw::DENSITY_DIV;
            o[mp::THEIRS2] = theirs2[i] / cw::DENSITY_DIV;
            o[mp::BALANCE] = tanh(mul(o[mp::OURS2] - o[mp::THEIRS2], cw::BALANCE_GAIN));
            o[mp::FRONT] = sigmoid(mul(cw::FRONT_GAIN, ours_near[i].min(theirs_touch[i])) - cw::FRONT_BIAS);
            o[mp::FRONT_ENEMY] = sigmoid(mul(cw::FRONT_GAIN, theirs_near[i].min(ours_touch[i])) - cw::FRONT_BIAS);
            o[mp::FLOW] = flow[i] / 6;
            let passable = chs[i][ch::PASSABLE];
            o[mp::CHOKE] = if between[i] { mul(mul(passable, step(-open[i], -cw::CHOKE_OPEN, cw::CHOKE_OPEN_GAIN)), step(o[mp::FLOW], cw::CHOKE_FLOW, cw::CHOKE_FLOW_GAIN)) } else { 0 };
            if foe_count[i] > 0 {
                o[mp::CUT_OFF] = mul(step(-foe_side[i], -cw::CUT_ALLIES, cw::CUT_GAIN), step(own_side[i], cw::CUT_OURS, cw::CUT_GAIN));
            }
            if own_count[i] > 0 {
                o[mp::OUR_CUT] = mul(step(-own_side[i], -cw::CUT_ALLIES, cw::CUT_GAIN), step(foe_side[i], cw::CUT_OURS, cw::CUT_GAIN));
            }
            // V6: the safe rear.
            let own_land = chs[i][ch::TERRITORY].max(0);
            let calm = ONE - step(chs[i][ch::DANGER], cw::SAFE_DANGER, cw::SAFE_GAIN);
            o[mp::SAFE2] = mul(mul(mul(own_land, calm), ONE - chs[i][ch::ZOC]), cw::SAFE_HEAL_BASE + chs[i][ch::HEAL]);
        }
        // V6: the rear spreads a step, losing 20 %.
        let safe: Vec<Milli> = out.iter().map(|o| o[mp::SAFE2]).collect();
        for i in 0..n {
            let best = self.nbr[i].iter().filter(|&&j| j != NONE).map(|&j| safe[j]).max().unwrap_or(0);
            out[i][mp::SAFE2] = safe[i].max(mul(best, cw::SAFE_SPREAD));
        }
        // V4: the siege ring around each hostile city in sight.
        for t in self.field.targets.iter().filter(|t| t.is_city() && !t.remembered) {
            let Some(&g) = index.get(&t.tile) else { continue };
            let mut ring_ours = 0;
            let mut free: Vec<usize> = Vec::new();
            for &j in &self.nbr[g] {
                if j == NONE {
                    continue;
                }
                let tile = self.tiles[j];
                let ours_here = self.own.iter().any(|u| u.tile == tile && fighter(rules, u.kind));
                let anybody = ours_here || picture.foes.iter().any(|f| f.tile == tile);
                ring_ours += i64::from(ours_here);
                if !anybody && chs[j][ch::PASSABLE] == ONE {
                    free.push(j);
                }
            }
            let siege = mul(step(ring_ours * ONE, cw::SIEGE_RING, ONE), step(chs[g][ch::FOCUS], -cw::SIEGE_FIRE_BIAS, cw::SIEGE_FIRE_GAIN));
            out[g][mp::SIEGE] = siege;
            for j in free {
                out[j][mp::GAP] = out[j][mp::GAP].max(siege);
            }
        }
        out
    }

    /// The tiles nearer our city than any foe is (design V3's «between»):
    /// a pass the enemy must take toward it.
    fn between(&self, picture: &Picture) -> Vec<bool> {
        let dims = self.field.dims;
        let foes: Vec<TileId> = picture.foes.iter().map(|f| f.tile).collect();
        self.tiles
            .iter()
            .map(|&t| {
                let Some((city, d)) = picture.own_cities.iter().filter_map(|c| dims.distance(*c, t).map(|d| (*c, d))).min_by_key(|(c, d)| (*d, *c)) else {
                    return false;
                };
                let theirs = foes.iter().filter_map(|f| dims.distance(*f, city)).min();
                theirs.is_some_and(|e| d < e)
            })
            .collect()
    }

    /// V7 for one operation: the cohesion of its members around each tile
    /// (two passes of conv(·; 1, 0.6), / 5) and how lonely a member there
    /// would be. `members`: their tiles.
    pub fn cohesion(&self, members: &[TileId]) -> (Vec<Milli>, Vec<Milli>) {
        let mut x = vec![0; self.tiles.len()];
        for t in members {
            if let Some(i) = self.index(*t) {
                x[i] += ONE;
            }
        }
        let c = self.conv(&self.conv(&x, ONE, cw::COHESION_NEIGHBOUR), ONE, cw::COHESION_NEIGHBOUR);
        let cohesion: Vec<Milli> = c.iter().map(|v| v / cw::COHESION_DIV).collect();
        let lonely = cohesion.iter().map(|v| step(-*v, -cw::LONELY_AT, cw::LONELY_GAIN)).collect();
        (cohesion, lonely)
    }

    /// The target on a tile.
    pub fn target_on(&self, tile: TileId) -> Option<(usize, &Target)> {
        self.field.targets.iter().enumerate().find(|(_, t)| t.tile == tile)
    }
}

/// The battlefield's round (the brief's).
fn view_round(brief: &TurnBrief) -> u32 {
    brief.round
}
