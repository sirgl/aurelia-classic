//! The battlefield model of the tactical layer (classic/docs/ai-tactical.md,
//! docs/ai-war-design.md 4.4.1): built once per turn for one theatre from
//! the observation, read by the plan search for every candidate plan.
//!
//! It holds
//!
//! - the **targets**: the enemy units and cities the player sees, each with
//!   its strengths from the rules' unit data;
//! - the **tiles** of the theatre: defence of the ground, water, owner;
//! - the enemy **reach**: for every enemy military unit the tiles it can
//!   stand on next turn with the movement it has left there, as a tree of
//!   steps (so a path can be tested against zones of control and bodies of
//!   the final positions of a plan);
//! - the **threat map** built from them: [`Battlefield::incoming`], the
//!   expected damage the enemy deals to a given unit on a given tile next
//!   turn, from the rules' own damage formulas and the enemy's reach, plus a
//!   small prior for tiles that may hide an enemy in the fog (the sighting
//!   memory: the strongest enemy power seen near us and when).
//!
//! Nothing hidden is read: the units are the observation's, the tiles are
//! the player's snapshots (`PlayerView::tile`), the step prices are
//! `PlayerView::known_step_cost_as`. Everything is deterministic: tiles in
//! `TileId` order, ties to the lower index.

use std::cmp::Reverse;
use std::collections::{BTreeMap, BinaryHeap};

use crate::ai::planner::{role_of, AiRole, TurnBrief};
use crate::ai::state::SeenCity;
use crate::ai::planner_map::{step_points, KnownMap};
use crate::combat::city::CITY_ATTACK_RANGE;
use crate::combat::damage::{city_strike_damage, melee_damage, ranged_damage, MeleeSide, Roll};
use crate::ids::{CityHandle, PlayerId, TileId, UnitHandle, UnitKindId};
use crate::knowledge::observation::{ObservedCity, ObservedUnit};
use crate::knowledge::view::PlayerView;
use crate::math::MapDimensions;
use crate::rules::CompiledRuleset;
use crate::world::unit::{MOVE_DENOMINATOR, UNIT_MAX_HP};

/// Enemy units a theatre models (the nearest to the members first).
pub const MAX_TARGETS: usize = 12;
/// Nodes of one enemy unit's reach search.
pub const REACH_NODES: usize = 80;
/// Fortification a unit that held its place last turn has (percent;
/// `combat::strength::FORTIFICATION_BONUS` for one turn).
pub const FORTIFIED_PERCENT: i64 = 20;
/// What a unit in an own city is taken to have against an attack, percent:
/// the planning estimate (the city fights with its own strength).
pub const CITY_TILE_PERCENT: i64 = 40;
/// The prior of a fogged tile next to the unit: the chance (percent) that an
/// enemy of the typical kind stands on it.
pub const FOG_PRIOR_PERCENT: i64 = 12;
/// Rounds after which a remembered threat is no prior any more.
pub const PRIOR_ROUNDS: u32 = 20;

/// What the rules' unit data says of a kind, in the units of the combat
/// formulas (hundredths of strength, move points).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KindStats {
    /// Melee (and defence) strength.
    pub melee: i64,
    /// Ranged strength (0: none).
    pub ranged: i64,
    /// Range in tiles (0: melee only).
    pub range: i32,
    /// Move points a turn.
    pub movement: u32,
    pub military: bool,
}

impl KindStats {
    pub fn of(rules: &CompiledRuleset, kind: UnitKindId) -> Self {
        let military = role_of(rules, kind) == AiRole::Military;
        match rules.units.get(kind.0 as usize) {
            Some(def) => {
                let ranged = def.ranged_strength > 0 && def.range > 0;
                Self {
                    melee: i64::from(def.strength) * 100,
                    ranged: if ranged { i64::from(def.ranged_strength) * 100 } else { 0 },
                    range: if ranged { i32::from(def.range) } else { 0 },
                    movement: u32::from(def.movement) * u32::from(MOVE_DENOMINATOR),
                    military,
                }
            }
            None => Self { melee: 0, ranged: 0, range: 0, movement: 0, military: false },
        }
    }

    pub fn is_ranged(&self) -> bool {
        self.range > 0
    }

    /// The strength the unit strikes with.
    pub fn attack(&self) -> i64 {
        if self.is_ranged() {
            self.ranged
        } else {
            self.melee
        }
    }
}

/// What a unit is worth to its side, in the plan's points (a kind's
/// strength; veterans more).
pub fn worth(stats: &KindStats, level: u8) -> i32 {
    let strength = (stats.melee.max(stats.ranged) / 100) as i32;
    if !stats.military {
        return 40;
    }
    60 + 4 * strength + 20 * i32::from(level)
}

/// An enemy unit or city the plan may strike, and a source of threat.
#[derive(Debug, Clone, Copy)]
pub struct Target {
    pub tile: TileId,
    pub owner: PlayerId,
    pub unit: Option<UnitHandle>,
    pub city: Option<CityHandle>,
    pub kind: Option<UnitKindId>,
    pub hp: u16,
    pub stats: KindStats,
    pub worth: i32,
    pub population: u32,
    /// A city's strength (hundredths); 0 for a unit.
    pub strength: i64,
    /// A city remembered from an earlier sighting: out of sight now, so it
    /// cannot be struck (no preview) but it strikes whoever comes within its
    /// range.
    pub remembered: bool,
}

impl Target {
    pub fn is_city(&self) -> bool {
        self.city.is_some() || self.remembered
    }

    /// May it strike next turn (a military unit, any city)?
    pub fn threatens(&self) -> bool {
        self.is_city() || self.stats.military
    }
}

/// One tile of the theatre as the player knows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TileInfo {
    /// Defence of the ground (hills, feature) in percent.
    pub ground: i16,
    /// Defence of its improvement, which counts for the owner and on land
    /// nobody owns.
    pub improvement: i16,
    pub owner: Option<PlayerId>,
    pub water: bool,
    pub visible: bool,
    /// One of the player's cities stands here.
    pub own_city: bool,
}

/// A tile an enemy unit reaches with the movement it has left there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReachNode {
    pub tile: TileId,
    pub left: u32,
    /// The tile it came from (its own tile for the start).
    pub parent: TileId,
}

/// The tiles one enemy unit reaches in a turn, sorted by tile.
#[derive(Debug, Clone, Default)]
pub struct Reach {
    nodes: Vec<ReachNode>,
}

impl Reach {
    pub fn get(&self, tile: TileId) -> Option<&ReachNode> {
        self.nodes.binary_search_by_key(&tile, |n| n.tile).ok().map(|i| &self.nodes[i])
    }

    pub fn nodes(&self) -> &[ReachNode] {
        &self.nodes
    }
}

/// The bodies and zones of control of a configuration of the player's
/// units: where an enemy cannot walk and where its step ends its move.
#[derive(Debug, Clone, Default)]
pub struct Posture {
    /// Tiles of the player's units (an enemy cannot enter them).
    pub bodies: Vec<TileId>,
    /// Tiles whose neighbours' steps are slowed: the player's military
    /// units and cities.
    pub sources: Vec<TileId>,
}

/// The battlefield of one theatre.
pub struct Battlefield<'v, 'a> {
    pub view: &'v PlayerView<'a>,
    pub rules: &'a CompiledRuleset,
    pub dims: MapDimensions,
    pub player: PlayerId,
    pub round: u32,
    pub targets: Vec<Target>,
    pub tiles: BTreeMap<TileId, TileInfo>,
    /// One per target (empty for a target that cannot move or strike).
    pub reach: Vec<Reach>,
    /// A typical enemy blow for the fog prior (hit points), 0 for none.
    pub prior_blow: u16,
}

impl<'v, 'a> Battlefield<'v, 'a> {
    /// The model of a theatre: the enemy units and cities in sight (those
    /// of players at war with us), the tiles within `radius` of `centres`.
    #[allow(clippy::too_many_arguments)]
    pub fn build(
        view: &'v PlayerView<'a>,
        rules: &'a CompiledRuleset,
        brief: &TurnBrief,
        round: u32,
        foes: &[ObservedUnit],
        cities: &[ObservedCity],
        remembered: &[SeenCity],
        own_cities: &[TileId],
        centres: &[TileId],
        radius: i32,
        prior: Option<u16>,
    ) -> Self {
        let dims = view.dimensions();
        let mut targets: Vec<Target> = Vec::new();
        for city in cities {
            let garrison = foes.iter().filter(|u| u.tile == city.tile).max_by_key(|u| (KindStats::of(rules, u.kind).melee, Reverse(u.handle.index)));
            let seen = SeenCity { tile: city.tile, owner: city.owner, population: city.population, last_seen_round: round };
            let strength = crate::ai::operations::estimate_city(brief, rules, &seen, garrison.map(|u| u.kind)).strength;
            targets.push(Target {
                tile: city.tile,
                owner: city.owner,
                unit: None,
                city: Some(city.handle),
                kind: None,
                hp: city.hp,
                stats: KindStats { melee: strength, ranged: strength, range: CITY_ATTACK_RANGE, movement: 0, military: true },
                worth: 0,
                population: city.population,
                strength,
                remembered: false,
            });
        }
        for seen in remembered {
            if cities.iter().any(|c| c.tile == seen.tile) {
                continue;
            }
            let strength = crate::ai::operations::estimate_city(brief, rules, seen, None).strength;
            targets.push(Target {
                tile: seen.tile,
                owner: seen.owner,
                unit: None,
                city: None,
                kind: None,
                hp: crate::world::city::CITY_BASE_HIT_POINTS,
                stats: KindStats { melee: strength, ranged: strength, range: CITY_ATTACK_RANGE, movement: 0, military: true },
                worth: 0,
                population: seen.population,
                strength,
                remembered: true,
            });
        }
        for unit in foes {
            if cities.iter().any(|c| c.tile == unit.tile) {
                continue;
            }
            let stats = KindStats::of(rules, unit.kind);
            let land = rules.units.get(unit.kind.0 as usize).is_some_and(|d| d.domain == crate::rules::content::Domain::Land);
            if !land {
                continue;
            }
            targets.push(Target {
                tile: unit.tile,
                owner: unit.owner,
                unit: Some(unit.handle),
                city: None,
                kind: Some(unit.kind),
                hp: unit.hp,
                stats,
                worth: worth(&stats, 0),
                population: 0,
                strength: 0,
                remembered: false,
            });
        }
        // The nearest to the centres first; cities and units by tile.
        let near = |t: &Target| centres.iter().filter_map(|c| dims.distance(*c, t.tile)).min().unwrap_or(i32::MAX);
        targets.sort_by_key(|t| (near(t), !t.is_city(), t.tile));
        targets.truncate(MAX_TARGETS);
        let mut tiles = BTreeMap::new();
        for centre in centres {
            for tile in crate::math::hex_disc(dims, *centre, radius) {
                tiles.entry(tile).or_insert_with(|| tile_info(view, rules, tile, own_cities));
            }
        }
        let mut field = Self { view, rules, dims, player: view.player(), round, targets, tiles, reach: Vec::new(), prior_blow: 0 };
        field.reach = (0..field.targets.len()).map(|i| field.flood(i)).collect();
        field.prior_blow = prior.unwrap_or(0);
        field
    }

    pub fn tile(&self, tile: TileId) -> Option<&TileInfo> {
        self.tiles.get(&tile)
    }

    pub fn distance(&self, a: TileId, b: TileId) -> i32 {
        self.dims.distance(a, b).unwrap_or(i32::MAX)
    }

    /// Defence of a tile for a unit of `owner` standing on it, in percent
    /// (the ground, its improvement where it counts, the city, the
    /// fortification of a unit that held its place).
    pub fn defence_percent(&self, tile: TileId, owner: PlayerId, fortified: bool) -> i64 {
        let Some(info) = self.tiles.get(&tile) else { return 0 };
        let mut percent = i64::from(info.ground);
        if info.owner.is_none() || info.owner == Some(owner) {
            percent += i64::from(info.improvement);
        }
        if info.own_city && owner == self.player {
            percent = percent.max(CITY_TILE_PERCENT);
        }
        if fortified {
            percent += FORTIFIED_PERCENT;
        }
        percent
    }

    /// Hit points a unit that rests (does not move) on `tile` regains in a
    /// turn: the rules' own rates by where it stands (a city, land of the
    /// player, any other land). A unit withdrawn to land of its own heals
    /// twice as fast as one resting where the enemy owns or nobody does.
    pub fn rest_heal(&self, tile: TileId) -> i64 {
        use crate::systems::combat::heal::{HEALING_ENEMY_TERRITORY, HEALING_FRIENDLY_TERRITORY, HEALING_IN_CITY_CENTER, HEALING_NEUTRAL_TERRITORY};
        let Some(info) = self.tiles.get(&tile) else { return i64::from(HEALING_NEUTRAL_TERRITORY) };
        if info.own_city {
            i64::from(HEALING_IN_CITY_CENTER)
        } else if info.owner == Some(self.player) {
            i64::from(HEALING_FRIENDLY_TERRITORY)
        } else if info.owner.is_some() {
            i64::from(HEALING_ENEMY_TERRITORY)
        } else {
            i64::from(HEALING_NEUTRAL_TERRITORY)
        }
    }

    /// The expected damage `foe` deals to a unit of `kind` with `hp` that
    /// ends its turn on `tile` (`fortified`: it held its place). The
    /// rules' damage formulas on the strengths of the unit data and the
    /// tile; the foe's wounds weaken it as in the rules.
    pub fn blow(&self, foe: &Target, kind: UnitKindId, hp: u16, tile: TileId, fortified: bool) -> u16 {
        self.blow_by(foe, &KindStats::of(self.rules, kind), hp, tile, fortified)
    }

    /// `blow` with the stats of the defender at hand.
    pub fn blow_by(&self, foe: &Target, me: &KindStats, hp: u16, tile: TileId, fortified: bool) -> u16 {
        let defence = (me.melee * (100 + self.defence_percent(tile, self.player, fortified).max(-90)) / 100).max(100);
        let wounded = UNIT_MAX_HP.saturating_sub(foe.hp);
        let damage = match foe.is_city() {
            true => city_strike_damage(foe.strength, defence, Roll::Expected),
            false if foe.stats.is_ranged() => ranged_damage(foe.stats.ranged, defence, wounded, 0, Roll::Expected),
            false => melee_damage(foe.stats.melee, defence, wounded, 0, Roll::Expected, MeleeSide::UnitVsUnit),
        };
        damage.min(hp)
    }

    /// What a melee blow on a unit of `kind` on `tile` costs the attacker
    /// `foe` through the defender's answer (hit points).
    pub fn answer(&self, foe: &Target, kind: UnitKindId, own_hp: u16, tile: TileId, fortified: bool) -> u16 {
        self.answer_by(foe, &KindStats::of(self.rules, kind), own_hp, tile, fortified)
    }

    /// `answer` with the stats of the defender at hand.
    pub fn answer_by(&self, foe: &Target, me: &KindStats, own_hp: u16, tile: TileId, fortified: bool) -> u16 {
        if foe.is_city() || foe.stats.is_ranged() {
            return 0;
        }
        let defence = (me.melee * (100 + self.defence_percent(tile, self.player, fortified).max(-90)) / 100).max(100);
        let wounded = UNIT_MAX_HP.saturating_sub(own_hp);
        melee_damage(defence, foe.stats.melee, wounded, 0, Roll::Expected, MeleeSide::UnitVsUnit).min(foe.hp)
    }

    /// Is the step `from -> to` slowed by a zone of control of the posture
    /// (the rules' step: a source adjacent to both tiles)?
    pub fn slowed(&self, posture: &Posture, from: TileId, to: TileId) -> bool {
        posture.sources.iter().any(|s| self.dims.adjacent(*s, from) && self.dims.adjacent(*s, to))
    }

    /// Does the enemy unit `index` reach `tile` with movement to spare in
    /// the posture: the tree path of its free reach has no body of the
    /// player on it and no slowed step? Its own tile always.
    pub fn arrives(&self, index: usize, tile: TileId, posture: &Posture) -> bool {
        let reach = &self.reach[index];
        let mut at = tile;
        let mut guard = 0;
        loop {
            let Some(node) = reach.get(at) else { return false };
            if node.parent == at {
                return true;
            }
            if node.left == 0 && at == tile {
                return false;
            }
            if posture.bodies.contains(&at) || self.slowed(posture, node.parent, at) {
                return false;
            }
            at = node.parent;
            guard += 1;
            if guard > REACH_NODES {
                return false;
            }
        }
    }

    /// Can the enemy target `index` strike a unit standing on `tile` next
    /// turn, given the posture? A city strikes within its range; a unit
    /// from a tile it reaches (its own tile always) within its range.
    pub fn can_strike(&self, index: usize, tile: TileId, posture: &Posture) -> bool {
        let target = &self.targets[index];
        if !target.threatens() {
            return false;
        }
        if target.is_city() {
            return self.distance(target.tile, tile) <= CITY_ATTACK_RANGE;
        }
        let range = target.stats.range.max(1);
        let reach = &self.reach[index];
        if range == 1 {
            return self.dims.neighbors(tile).into_iter().any(|t| reach.get(t).is_some() && self.arrives(index, t, posture));
        }
        crate::math::hex_disc(self.dims, tile, range).any(|t| t != tile && reach.get(t).is_some() && self.arrives(index, t, posture))
    }

    /// The expected damage of the fog on a unit that ends on `tile`: a
    /// typical blow weighted by the unseen tiles next to it.
    pub fn fog_blow(&self, tile: TileId) -> u16 {
        if self.prior_blow == 0 {
            return 0;
        }
        let unseen = self.dims.neighbors(tile).into_iter().filter(|n| self.tiles.get(n).is_some_and(|i| !i.visible && !i.water)).count() as i64;
        (i64::from(self.prior_blow) * unseen * FOG_PRIOR_PERCENT / 100) as u16
    }

    /// The tiles the enemy unit `index` reaches (a Dijkstra over the known
    /// map with the plain land price, bodies and zones of control left out:
    /// the plan's posture is applied to the tree afterwards).
    fn flood(&self, index: usize) -> Reach {
        let target = &self.targets[index];
        if target.is_city() || !target.stats.military {
            return Reach::default();
        }
        let budget = target.stats.movement;
        let map = KnownMap::land(self.view);
        let mut best: BTreeMap<TileId, (u32, TileId)> = BTreeMap::new();
        let mut heap = BinaryHeap::new();
        best.insert(target.tile, (0, target.tile));
        heap.push(Reverse((0u32, target.tile.0)));
        let mut expanded = 0;
        while let Some(Reverse((cost, tile))) = heap.pop() {
            let tile = TileId(tile);
            if best.get(&tile).is_some_and(|(c, _)| *c < cost) {
                continue;
            }
            expanded += 1;
            if expanded > REACH_NODES || cost >= budget {
                continue;
            }
            for next in self.dims.neighbors(tile) {
                if !self.tiles.contains_key(&next) {
                    continue;
                }
                let Some(points) = step_points(&map, tile, next) else { continue };
                let cost = cost.saturating_add(points);
                if best.get(&next).is_none_or(|(c, _)| cost < *c) {
                    best.insert(next, (cost, tile));
                    heap.push(Reverse((cost, next.0)));
                }
            }
        }
        let nodes = best
            .into_iter()
            .map(|(tile, (cost, parent))| ReachNode { tile, left: budget.saturating_sub(cost), parent })
            .collect();
        Reach { nodes }
    }
}

fn tile_info(view: &PlayerView<'_>, rules: &CompiledRuleset, tile: TileId, own_cities: &[TileId]) -> TileInfo {
    let known = view.tile(tile);
    let visible = known.is_visible();
    let Some(snapshot) = known.snapshot().copied() else {
        return TileInfo { ground: 0, improvement: 0, owner: None, water: true, visible: false, own_city: false };
    };
    let kinds = view.terrain_kinds();
    let high = kinds.is_hills(snapshot.terrain.0) || kinds.is_mountains(snapshot.terrain.0);
    let ground = crate::combat::terrain::ground_defense_percent(rules, high, snapshot.feature);
    let improvement = snapshot
        .improvement
        .and_then(|i| rules.improvements.get(i.0 as usize))
        .map_or(0, |d| i64::from(d.defense_pct));
    TileInfo {
        ground: ground as i16,
        improvement: improvement as i16,
        owner: snapshot.owner,
        water: kinds.is_water(snapshot.terrain.0),
        visible,
        own_city: own_cities.contains(&tile),
    }
}
