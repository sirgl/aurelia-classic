// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Derived from Unciv (https://github.com/yairm210/Unciv), tag 4.22.10, MPL-2.0:
// the queries the automation asks of its world (core/src/com/unciv/logic/map/mapunit/MapUnit.kt
// `rankTileForHealing`, `getDamageFromTerrain`; logic/civilization/managers/ThreatManager.kt
// `getDistanceToClosestEnemyUnit`, `getDangerousTiles`; logic/battle/BattleDamage.kt through the
// rules' own combat preview), answered from what the player knows.

//! What Unciv's automation reads of the world, answered from the player's own
//! knowledge: the turn's sight (`TurnSight`, taken at the start of the turn from
//! the observation) and the facts around one unit (`Unit`, from the unit's
//! local observation). Where Unciv reads the truth (every city of a civilization
//! at war, its health), the port reads what the player saw: the cities it sees
//! or remembers, a remembered city at full health.

use crate::ai::planner::{PlanTools, TurnBrief, UnitView};
use crate::combat::exchange::CombatPreview;
use crate::ids::{CityHandle, EntityId, PlayerId, TileId, UnitHandle, UnitKindId};
use crate::knowledge::observation::{AIObservation, LocalObservation, ObservedUnit, OwnUnit};
use crate::knowledge::public::PlayerQueries;
use crate::math::MapDimensions;
use crate::rules::content::Domain;
use crate::rules::CompiledRuleset;
use kernel::ai::map::{find_path, Blockers, KnownMap, PathOutcome};
use kernel::world::unit::MOVE_DENOMINATOR;

/// The full health of a unit (Unciv's 100).
pub const FULL_HP: u16 = 100;

/// A city of another player the player knows: seen now (with its health) or
/// remembered (as last seen; its health unknown).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KnownCity {
    pub tile: TileId,
    pub owner: PlayerId,
    /// None: remembered, not seen now.
    pub handle: Option<CityHandle>,
    pub hp: Option<u16>,
}

/// One of the player's cities at the start of the turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OwnCitySight {
    pub handle: CityHandle,
    pub tile: TileId,
    pub hp: u16,
    /// Below its maximum health (Unciv: `health < getMaxHealth()`).
    pub damaged: bool,
}

/// The player's sight at the start of its turn: what Unciv's automation asks
/// of the civilization (its cities, its enemies, their cities and units).
#[derive(Debug, Clone)]
pub struct TurnSight {
    pub player: PlayerId,
    pub round: u32,
    pub dims: MapDimensions,
    /// The kind of every player met (nation, city-state, barbarians), by player.
    pub kinds: Vec<(PlayerId, crate::world::player::PlayerKind)>,
    /// Players at war with us (barbarians and city-states included), sorted.
    pub at_war: Vec<PlayerId>,
    pub capital: Option<TileId>,
    pub own_cities: Vec<OwnCitySight>,
    /// Every foreign city the player sees or remembers, by tile.
    pub foreign_cities: Vec<KnownCity>,
    /// Foreign units seen at the start of the turn.
    pub foreign_units: Vec<ObservedUnit>,
    pub own_units: Vec<OwnUnit>,
    pub nation: u8,
    pub techs: Vec<crate::ids::TechId>,
    /// Resources the empire has copies of now.
    pub resources_have: Vec<crate::ids::ResourceId>,
    pub treasury: u32,
    pub gold_income: i32,
    pub happiness: i32,
    /// The religion part of the observation (`unciv::religion`).
    pub faith: crate::knowledge::observation::FaithObservation,
    /// Unciv's rating of every belief still free (`religion::rate_belief`).
    pub belief_ratings: Vec<(crate::ids::BeliefId, f32)>,
}

impl TurnSight {
    /// Who builds (`world::builds::Builder`): the player, its nation and techs.
    pub fn builder(&self) -> crate::world::builds::Builder {
        crate::world::builds::Builder { player: self.player, nation: self.nation, techs: crate::derived::tile_yields::TechSet::of(&self.techs) }
    }

    /// `Automation.rankStatsValue` of tile yields.
    pub fn rank_yields(&self, y: &crate::rules::content::Yields) -> f32 {
        let gold = if self.treasury == 0 && self.gold_income <= 0 { y.gold as f32 } else { y.gold as f32 / 3.0 };
        y.food as f32 * 1.2 + gold + y.production as f32 + y.science as f32 + y.culture as f32 + y.module as f32
    }

    pub fn take(obs: &AIObservation<'_>, state: &crate::ai::state::AiState) -> Self {
        let mut at_war: Vec<PlayerId> = obs.relations.iter().filter(|r| r.at_war).map(|r| r.other).collect();
        at_war.sort();
        let capital_handle = obs.view.own_capital();
        let capital = obs.own_cities.iter().find(|c| Some(c.handle) == capital_handle).map(|c| c.tile);
        let own_cities = obs
            .own_cities
            .iter()
            .map(|c| OwnCitySight { handle: c.handle, tile: c.tile, hp: c.hp, damaged: c.damage > 0 })
            .collect();
        let mut foreign_cities: Vec<KnownCity> = obs
            .visible_foreign_cities
            .iter()
            .map(|c| KnownCity { tile: c.tile, owner: c.owner, handle: Some(c.handle), hp: Some(c.hp) })
            .collect();
        for (tile, sighting) in obs.view.remembered_cities() {
            if sighting.owner != obs.player && !foreign_cities.iter().any(|c| c.tile == tile) {
                foreign_cities.push(KnownCity { tile, owner: sighting.owner, handle: None, hp: None });
            }
        }
        // The cities the AI's memory keeps (seen at some time; `planner_strategy::remember`).
        for seen in state.seen_cities() {
            if seen.owner != obs.player && !foreign_cities.iter().any(|c| c.tile == seen.tile) {
                foreign_cities.push(KnownCity { tile: seen.tile, owner: seen.owner, handle: None, hp: None });
            }
        }
        foreign_cities.sort_by_key(|c| c.tile);
        Self {
            player: obs.player,
            round: obs.round,
            dims: obs.view.dimensions(),
            kinds: obs.relations.iter().map(|r| (r.other, r.kind)).collect(),
            at_war,
            capital,
            own_cities,
            foreign_cities,
            foreign_units: obs.visible_foreign_units.clone(),
            own_units: obs.own_units.clone(),
            nation: obs.economy.nation,
            techs: obs.economy.techs.clone(),
            resources_have: (0..obs.view.rules().resources.len()).map(|i| crate::ids::ResourceId(i as u16)).filter(|r| obs.economy.resources.available(*r) > 0).collect(),
            treasury: obs.economy.treasury,
            gold_income: obs.economy.income.gold,
            happiness: obs.economy.happiness.excess(),
            faith: obs.faith.clone(),
            belief_ratings: {
                let o = &obs.faith.overview;
                let mut free: Vec<crate::ids::BeliefId> = o.free_pantheon.iter().chain(&o.free_founder).chain(&o.free_follower).chain(&o.free_enhancer).chain(&o.free_reformation).copied().collect();
                free.sort_by_key(|b| b.0);
                free.dedup();
                free.into_iter().map(|b| (b, super::religion::rate_belief(obs, obs.view.rules(), b))).collect()
            },
        }
    }

    pub fn at_war(&self, other: PlayerId) -> bool {
        self.at_war.binary_search(&other).is_ok()
    }

    /// The kind of a player met (None: not met).
    pub fn foreign_kind(&self, other: PlayerId) -> Option<crate::world::player::PlayerKind> {
        self.kinds.iter().find(|(p, _)| *p == other).map(|(_, k)| *k)
    }

    /// Unciv's `civ.isAtWar()`: a war with a nation or a city-state (Unciv keeps no
    /// diplomacy with the barbarians, so they do not count).
    pub fn is_at_war(&self, relations: &[crate::knowledge::observation::Relation]) -> bool {
        relations.iter().any(|r| r.at_war && r.kind != crate::world::player::PlayerKind::Module(crate::world::player::ClassicKind::Barbarian))
    }
}

/// What a unit kind is to Unciv's automation (`BaseUnit`).
#[derive(Debug, Clone, Copy)]
pub struct Kind<'r> {
    pub rules: &'r CompiledRuleset,
    pub kind: UnitKindId,
}

impl Kind<'_> {
    fn def(&self) -> Option<&kernel::rules::compiled::UnitDefinition<crate::classic::Classic>> {
        self.rules.units.get(self.kind.0 as usize)
    }
    /// `isRanged`: a ranged strength.
    pub fn is_ranged(&self) -> bool {
        self.def().is_some_and(|d| d.ranged_strength > 0)
    }
    /// `isMelee`: no ranged strength, a strength.
    pub fn is_melee(&self) -> bool {
        self.def().is_some_and(|d| d.ranged_strength == 0 && d.strength > 0)
    }
    pub fn is_military(&self) -> bool {
        self.is_ranged() || self.is_melee()
    }
    pub fn is_civilian(&self) -> bool {
        !self.is_military()
    }
    pub fn domain(&self) -> Domain {
        self.def().map(|d| d.domain).unwrap_or_default()
    }
    pub fn is_air(&self) -> bool {
        self.domain() == Domain::Air
    }
    pub fn is_water(&self) -> bool {
        self.domain() == Domain::Sea
    }
    pub fn is_land(&self) -> bool {
        self.domain() == Domain::Land
    }
    /// `getRange`: 1 for a melee unit.
    pub fn range(&self) -> i32 {
        self.def().filter(|d| d.ranged_strength > 0 && d.range > 0).map_or(1, |d| i32::from(d.range))
    }
    pub fn sight(&self) -> i32 {
        self.def().map_or(2, |d| i32::from(d.sight.max(1)))
    }
    pub fn strength(&self) -> u16 {
        self.def().map_or(0, |d| d.strength)
    }
    pub fn self_destructs(&self) -> bool {
        self.def().is_some_and(|d| d.combat.suicide)
    }
    pub fn is_nuclear(&self) -> bool {
        self.def().is_some_and(|d| d.combat.nuke_level > 0)
    }
    pub fn can_intercept(&self) -> bool {
        self.def().is_some_and(|d| d.combat.air_intercept_range > 0)
    }
    pub fn founds_cities(&self) -> bool {
        self.def().is_some_and(|d| d.founder)
    }
    pub fn great_person(&self) -> bool {
        crate::derived::great_people::great_person_of(self.rules, self.kind).is_some()
    }
    /// A great general or admiral: moves after the military (`isGreatPersonOfType("War")`).
    pub fn war_great_person(&self) -> bool {
        use crate::classic::effects::GreatPersonKind;
        crate::derived::great_people::great_person_of(self.rules, self.kind).is_some_and(|g| matches!(g.kind, GreatPersonKind::General | GreatPersonKind::Admiral))
    }
    /// `isProbablySiegeUnit`: ranged, with a strength bonus against cities.
    pub fn is_probably_siege(&self) -> bool {
        self.is_ranged() && vs_city_percent(self.rules, self.kind) > 0
    }
}

/// The bonus against cities a kind carries (its own effects and its free
/// promotions), percent.
pub fn vs_city_percent(rules: &CompiledRuleset, kind: UnitKindId) -> i64 {
    use crate::rules::effects::{CombatContext, Modifier};
    let Some(def) = rules.units.get(kind.0 as usize) else {
        return 0;
    };
    let promotions = def.free_promotions.iter().filter_map(|p| rules.promotions.get(p.0 as usize)).flat_map(|p| p.effects.passive_unit.iter());
    def.effects
        .passive_unit
        .iter()
        .chain(promotions)
        .filter_map(|e| match e.modifier {
            Modifier::ModifyCombat { context: CombatContext::VsCity, percent } => Some(i64::from(percent)),
            _ => None,
        })
        .sum()
}

/// One attack the unit could make: from a tile it reaches with moves left
/// (Unciv's `AttackableTile`).
#[derive(Debug, Clone, Copy)]
pub struct AttackableTile {
    pub from: TileId,
    pub target: TileId,
    /// Movement points left after reaching `from` (Unciv's floats).
    pub moves_after: f32,
    pub preview: CombatPreview,
}

/// The facts around one unit for one decision.
pub struct Unit<'o, 'a> {
    pub view: &'o UnitView<'o, 'a>,
    pub sight: &'o TurnSight,
    pub me: OwnUnit,
    pub kind: Kind<'a>,
    reachable: std::cell::OnceCell<Vec<(TileId, u16)>>,
}

impl<'o, 'a> Unit<'o, 'a> {
    pub fn new(view: &'o UnitView<'o, 'a>, sight: &'o TurnSight) -> Self {
        let me = view.local.unit;
        Self { view, sight, me, kind: Kind { rules: view.local.view.rules(), kind: me.kind }, reachable: std::cell::OnceCell::new() }
    }

    pub fn local(&self) -> &'o LocalObservation<'a> {
        self.view.local
    }
    pub fn brief(&self) -> &'o TurnBrief {
        self.view.brief
    }
    pub fn rules(&self) -> &'a CompiledRuleset {
        self.view.local.view.rules()
    }
    pub fn dims(&self) -> MapDimensions {
        self.view.local.view.dimensions()
    }
    pub fn distance(&self, a: TileId, b: TileId) -> i32 {
        self.dims().distance(a, b).unwrap_or(i32::MAX / 4)
    }
    pub fn tile(&self) -> TileId {
        self.me.tile
    }
    pub fn health(&self) -> u16 {
        self.me.hp
    }
    pub fn has_movement(&self) -> bool {
        self.me.moves_left > 0
    }
    /// Movement points left, in whole points (Unciv's `currentMovement`).
    pub fn movement(&self) -> f32 {
        f32::from(self.me.moves_left) / f32::from(MOVE_DENOMINATOR)
    }

    /// `movement.getDistanceToTiles()`: the tiles reached this turn with the
    /// moves left there.
    pub fn reachable(&self) -> &[(TileId, u16)] {
        self.reachable.get_or_init(|| self.view.local.view.own_reachable(self.me.handle))
    }
    pub fn reaches_now(&self, tile: TileId) -> bool {
        self.reachable().iter().any(|(t, _)| *t == tile)
    }

    pub fn at_war(&self, owner: PlayerId) -> bool {
        self.view.local.relation(owner).is_some_and(|r| r.at_war) || self.sight.at_war(owner)
    }

    pub fn is_water_tile(&self, tile: TileId) -> bool {
        let kinds = self.view.local.view.terrain_kinds();
        self.view.local.view.tile(tile).snapshot().is_some_and(|s| kinds.is_water(s.terrain.0))
    }
    pub fn is_land_tile(&self, tile: TileId) -> bool {
        let kinds = self.view.local.view.terrain_kinds();
        self.view.local.view.tile(tile).snapshot().is_some_and(|s| !kinds.is_water(s.terrain.0))
    }
    pub fn owner_of(&self, tile: TileId) -> Option<PlayerId> {
        self.view.local.view.tile(tile).snapshot().and_then(|s| s.owner)
    }
    pub fn is_city_tile(&self, tile: TileId) -> bool {
        self.view.local.view.tile(tile).snapshot().is_some_and(|s| s.city)
    }
    pub fn embarked(&self) -> bool {
        self.kind.is_land() && self.is_water_tile(self.me.tile)
    }
    /// Unciv's `isFriendlyTerritory`: own land or land granting us entry.
    pub fn friendly_territory(&self, tile: TileId) -> bool {
        match self.owner_of(tile) {
            Some(owner) if owner == self.view.local.player => true,
            Some(owner) => self.view.local.relation(owner).is_some_and(|r| r.they_grant_entry),
            None => false,
        }
    }

    /// `getDamageFromTerrain`: the classic terrains deal no damage.
    pub fn damage_from_terrain(&self, _tile: TileId) -> i32 {
        0
    }

    /// `rankTileForHealing` over the known tile (the rules' heal rates; they
    /// are Unciv's).
    pub fn rank_tile_for_healing(&self, tile: TileId) -> i32 {
        use crate::systems::combat::heal::{HEALING_ENEMY_TERRITORY, HEALING_FRIENDLY_TERRITORY, HEALING_IN_CITY_CENTER, HEALING_NEUTRAL_TERRITORY};
        let friendly = self.friendly_territory(tile);
        if self.is_city_tile(tile) {
            return HEALING_IN_CITY_CENTER;
        }
        if self.is_water_tile(tile) {
            return if friendly && self.kind.is_water() { HEALING_FRIENDLY_TERRITORY } else { 0 };
        }
        if self.kind.is_air() {
            return 0;
        }
        if friendly {
            HEALING_FRIENDLY_TERRITORY
        } else if self.owner_of(tile).is_none() {
            HEALING_NEUTRAL_TERRITORY
        } else {
            HEALING_ENEMY_TERRITORY
        }
    }

    /// The defensive bonus of a known tile (`getDefensiveBonus`), percent.
    pub fn defensive_bonus(&self, tile: TileId) -> i64 {
        let view = &self.view.local.view;
        let Some(snapshot) = view.tile(tile).snapshot().copied() else {
            return 0;
        };
        let kinds = view.terrain_kinds();
        let high = kinds.is_hills(snapshot.terrain.0) || kinds.is_mountains(snapshot.terrain.0);
        let ground = crate::combat::terrain::ground_defense_percent(self.rules(), high, snapshot.feature);
        let improvement = snapshot.improvement.and_then(|i| self.rules().improvements.get(i.0 as usize)).map_or(0, |d| i64::from(d.defense_pct));
        ground + improvement
    }

    /// Unciv's `tileHeight`: what a tile shows around it.
    pub fn tile_height(&self, tile: TileId) -> i32 {
        let view = &self.view.local.view;
        let Some(snapshot) = view.tile(tile).snapshot().copied() else {
            return 0;
        };
        let kinds = view.terrain_kinds();
        let base = if kinds.is_mountains(snapshot.terrain.0) {
            4
        } else if kinds.is_hills(snapshot.terrain.0) {
            1
        } else {
            0
        };
        let blocks = snapshot.feature.and_then(|f| self.rules().features.get(f.0 as usize)).is_some_and(|f| f.see_through > 0);
        base + i32::from(blocks)
    }

    /// The own units the unit sees around it (its local observation), itself
    /// excluded.
    pub fn own_units(&self) -> &'o [OwnUnit] {
        &self.view.local.own_units
    }
    /// Our military unit on `tile` (the unit itself included).
    pub fn own_military_on(&self, tile: TileId) -> Option<OwnUnit> {
        let rules = self.rules();
        if self.me.tile == tile && self.kind.is_military() {
            return Some(self.me);
        }
        self.view.local.own_units.iter().copied().find(|u| u.tile == tile && Kind { rules, kind: u.kind }.is_military() && Kind { rules, kind: u.kind }.domain() != Domain::Air)
    }
    /// A foreign unit seen on `tile` now: military first.
    pub fn foreign_on(&self, tile: TileId) -> Option<ObservedUnit> {
        let rules = self.rules();
        let mut on = self.view.local.foreign_units.iter().filter(|u| u.tile == tile);
        let all: Vec<&ObservedUnit> = on.by_ref().collect();
        all.iter().find(|u| Kind { rules, kind: u.kind }.is_military()).or(all.first()).map(|u| **u)
    }
    pub fn foreign_military_on(&self, tile: TileId) -> Option<ObservedUnit> {
        let rules = self.rules();
        self.view.local.foreign_units.iter().copied().find(|u| u.tile == tile && Kind { rules, kind: u.kind }.is_military())
    }
    /// A foreign city on `tile` the player sees now or remembers.
    pub fn foreign_city_at(&self, tile: TileId) -> Option<KnownCity> {
        if let Some(c) = self.view.local.foreign_cities.iter().find(|c| c.tile == tile) {
            return Some(KnownCity { tile, owner: c.owner, handle: Some(c.handle), hp: Some(c.hp) });
        }
        self.sight.foreign_cities.iter().copied().find(|c| c.tile == tile)
    }

    /// `movement.canMoveTo`: the unit may end its move there (no foreign
    /// unit, no own unit of its stack class), as far as it knows.
    pub fn can_move_to(&self, tile: TileId) -> bool {
        if tile == self.me.tile {
            return false;
        }
        if !self.view.local.view.is_known(tile) {
            return false;
        }
        if let Some(city) = self.foreign_city_at(tile) {
            let _ = city;
            return false;
        }
        if !self.kind.is_land() && self.kind.is_water() && !self.is_water_tile(tile) && !(self.is_city_tile(tile) && self.owner_of(tile) == Some(self.view.local.player)) {
            return false;
        }
        crate::ai::planner_units::can_stop(self.view, self.rules(), tile)
    }

    /// The path over the known map (`movement.getShortestPath`), None when the
    /// player knows no way there.
    pub fn path_to(&self, tools: &mut PlanTools<'_>, goal: TileId) -> Option<Vec<TileId>> {
        self.path_with_points(tools, goal).map(|(tiles, _)| tiles)
    }

    /// The path and its price in movement points (×`MOVE_DENOMINATOR`).
    pub fn path_with_points(&self, tools: &mut PlanTools<'_>, goal: TileId) -> Option<(Vec<TileId>, u32)> {
        if goal == self.me.tile {
            return Some((vec![goal], 0));
        }
        let blockers = Blockers::new(self.view.local.foreign_units.iter().map(|u| u.tile).filter(|t| *t != goal).collect());
        let limit = tools.settings.budget.search();
        let outcome = find_path(&KnownMap::of_unit(&self.view.local.view, self.me.handle), self.me.tile, goal, &blockers, tools.scratch, limit, &mut tools.counters.search);
        match outcome {
            PathOutcome::Found { tiles, points } => Some((tiles, points)),
            _ => None,
        }
    }
    /// The turns the walk to `goal` takes (Unciv's `getShortestPath(..).size`:
    /// one tile a turn).
    pub fn turns_to(&self, tools: &mut PlanTools<'_>, goal: TileId) -> Option<u32> {
        let (_, points) = self.path_with_points(tools, goal)?;
        Some(kernel::ai::map::turns_for(points, self.me.moves_max).max(1))
    }

    /// The tile to walk to for `goal`: the goal itself when the known map leads
    /// there, else the neighbour of it nearest the unit that it leads to (a
    /// foreign city's tile is entered by an attack, not a walk).
    pub fn approach(&self, tools: &mut PlanTools<'_>, goal: TileId) -> Option<TileId> {
        if self.path_to(tools, goal).is_some() {
            return Some(goal);
        }
        let mut near: Vec<TileId> = self.dims().neighbors(goal).into_iter().collect();
        near.sort_by_key(|t| (self.distance(*t, self.me.tile), *t));
        near.into_iter().find(|t| *t == self.me.tile || self.path_to(tools, *t).is_some())
    }

    /// `movement.canReach`.
    pub fn can_reach(&self, tools: &mut PlanTools<'_>, goal: TileId) -> bool {
        self.path_to(tools, goal).is_some()
    }

    /// The preview of an attack on `target` from `from` (the rules' own
    /// combat preview: None when the rules would not allow it).
    pub fn preview_from(&self, from: TileId, target: TileId) -> Option<CombatPreview> {
        let view = &self.view.local.view;
        // A siege unit not set up yet is previewed as set up (the interface's preview of its shot).
        if from == self.me.tile && !view.must_set_up_first(self.me.handle) {
            view.attack_preview(self.me.handle, target)
        } else {
            view.attack_preview_from(self.me.handle, from, target)
        }
    }

    /// Unciv's `ThreatManager.getDistanceToClosestEnemyUnit(tile, maxDist,
    /// takeLargerValues)`: the nearest tile within `max` holding a military
    /// unit at war the player sees, or a city at war it knows.
    pub fn distance_to_closest_enemy(&self, tile: TileId, max: i32, take_larger: bool) -> i32 {
        let units = self
            .view
            .local
            .foreign_units
            .iter()
            .filter(|u| self.at_war(u.owner) && Kind { rules: self.rules(), kind: u.kind }.is_military())
            .map(|u| u.tile);
        let cities = self.sight.foreign_cities.iter().filter(|c| self.at_war(c.owner)).map(|c| c.tile);
        let best = units.chain(cities).map(|t| self.distance(tile, t)).filter(|d| *d <= max).min();
        match best {
            Some(d) => d,
            None if take_larger => 500_000,
            None => max,
        }
    }

    /// `ThreatManager.getDangerousTiles(unit, distance)`: the tiles in reach of
    /// the enemy's ranged units and cities near the unit.
    pub fn dangerous_tiles(&self, distance: i32) -> Vec<TileId> {
        let dims = self.dims();
        let rules = self.rules();
        let mut out = Vec::new();
        for u in &self.view.local.foreign_units {
            if !self.at_war(u.owner) || self.distance(u.tile, self.me.tile) > distance {
                continue;
            }
            let kind = Kind { rules, kind: u.kind };
            if !kind.is_ranged() {
                continue;
            }
            out.extend(crate::math::hex_disc(dims, u.tile, kind.range()));
        }
        for c in &self.sight.foreign_cities {
            if self.at_war(c.owner) && self.distance(c.tile, self.me.tile) <= distance {
                out.extend(crate::math::hex_disc(dims, c.tile, CITY_BOMBARD_RANGE));
            }
        }
        out.sort();
        out.dedup();
        out
    }
}

/// A city's bombard range (`City.getBombardRange`: 2 in the classic rules).
pub const CITY_BOMBARD_RANGE: i32 = 2;

/// Units of the player near `tile` (within `radius`) of the start of the turn
/// seen by a unit's local observation (and itself).
pub fn own_military_near(unit: &Unit<'_, '_>, tile: TileId, radius: i32) -> Vec<OwnUnit> {
    let rules = unit.rules();
    let mut out: Vec<OwnUnit> = unit
        .own_units()
        .iter()
        .copied()
        .filter(|u| unit.distance(u.tile, tile) <= radius && Kind { rules, kind: u.kind }.is_military())
        .collect();
    if unit.kind.is_military() && unit.distance(unit.me.tile, tile) <= radius {
        out.push(unit.me);
    }
    out.sort_by_key(|u| u.id);
    out
}

/// The unit id of an own unit (for the trace).
pub fn actor(unit: &OwnUnit) -> crate::ai::trace::Actor {
    crate::ai::trace::Actor::Unit(unit.id)
}

/// The handle of an own unit with id `id` among `units`.
pub fn handle_of(units: &[OwnUnit], id: EntityId) -> Option<UnitHandle> {
    units.iter().find(|u| u.id == id).map(|u| u.handle)
}
