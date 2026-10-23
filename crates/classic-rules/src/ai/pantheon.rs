//! The pantheon a leader founds: the belief that pays most on the land and
//! in the situation of its empire, not the first of a fixed list.
//!
//! A pantheon is chosen once per game and player, when the faith pool pays
//! for it, so this module takes one pass over the tiles around the own
//! cities and then values every free belief from its **effects** (the
//! compiled `EffectSet`; no belief is named anywhere, so any belief table
//! works: the shipped one, a legacy table, a mod, another module's):
//!
//! - a tile effect (a yield from tiles of a terrain, feature, resource,
//!   improvement or plot kind) is worth its yield on the tiles of the land
//!   that match its condition, each counted by how likely a city works it:
//!   a resource or an improved tile mostly, a bare terrain tile rarely; an
//!   unimproved resource counts as the improvement it can host;
//! - a city effect (a yield, growth, happiness, a cost) is worth its value
//!   in every city that meets its condition (population by the city's size
//!   now, river, coast, a building, a road to the capital): the own cities
//!   now and the cities the empire is expected to found;
//! - a unit, combat or kill effect is worth what the military situation
//!   makes of it: a war with a civilization, barbarians in sight and a
//!   neighbour close by raise the weight of the military and the kills to
//!   expect.
//!
//! The value of one unit of an effect is the planners' own
//! (`planner_value::effect_value`, weights by yield; a few modifier
//! kinds whose base flow is not a whole yield are valued here, `unit_value`)
//! with the weights of an expanding empire, shifted by the leader's
//! personality (science, culture, gold, faith, aggression, expansion).
//!
//! # Assumptions (for a plan that wants to guard them)
//!
//! 1. The tiles within `LAND_RADIUS` of the own cities that no rival owns
//!    stay the empire's, and the cities it founds later find land as rich as
//!    the land of the cities now (up to `GROWTH_CAP` times).
//! 2. The empire reaches `expected_cities` cities (five, shifted by the
//!    leader's expansion) and its cities grow as the population conditions
//!    of `size_share` say.
//! 3. A tile with a visible resource or its improvement is worked
//!    `WORKED_RICH` percent of the time, a bare tile `WORKED_BARE`.
//! 4. The military situation of the moment (wars with civilizations,
//!    barbarians within `BARBARIAN_NEAR`, a rival city within
//!    `NEIGHBOUR_NEAR`) stays; kills near the cities follow it.
//! 5. The free beliefs at the moment of the choice stay free until it is
//!    made (rivals found theirs meanwhile: the choice is made from the
//!    observation of the round the pool pays).
//!
//! The score is deterministic and reads the observation only; a tie goes to
//! the earlier belief of the table.

use crate::ai::planner::{role_of, AiRole};
use crate::ai::planner_value::{effect_value, Weights};
use crate::ai::state::Strategy;
use crate::ids::{BeliefId, TileId};
use crate::knowledge::observation::{AIObservation, OwnCity};
use crate::knowledge::view::TileSnapshot;
use crate::rules::content::Personality;
use crate::rules::effects::{CompiledEffect, Condition, CostTarget, Modifier, PlotFilter, Scope, Selector, Trigger};
use crate::rules::CompiledRuleset;

/// Hex radius around a city whose tiles it can work.
pub const LAND_RADIUS: i32 = 3;
/// Scale of counts: a count of 100 is one tile or one city.
const ONE: i64 = 100;
/// Chance (percent) that a tile with a visible resource, or one that
/// carries the improvement, is worked once the cities have grown.
const WORKED_RICH: i64 = 70;
/// The same for an unimproved resource that could host the improvement
/// (a worker has to come first).
const WORKED_POTENTIAL: i64 = 50;
/// The same for a tile that matches by its bare terrain, feature or plot.
const WORKED_BARE: i64 = 20;
/// A natural wonder is worked by every city that can.
const WORKED_WONDER: i64 = 80;
/// Cities an empire of a neutral leader grows to (its expansion shifts it
/// by up to two cities, `expected_cities`).
const EXPECTED_CITIES: i64 = 5;
/// The land of the cities to come is as rich as that of the cities now, up
/// to this many times (percent).
const GROWTH_CAP: i64 = 300;
/// Weight of an army unit in the military effects that need units.
const MIN_ARMY: i64 = 2;
/// A neighbour's city this close (hex tiles) to an own city is a threat.
const NEIGHBOUR_NEAR: i32 = 10;
/// Barbarians this close to an own city are a threat.
const BARBARIAN_NEAR: i32 = 6;
/// Kills near the cities the empire expects without any threat (percent of
/// one kill), per war with a civilization, per barbarian in sight.
const KILLS_BASE: i64 = 100;
const KILLS_PER_FRONT: i64 = 600;
const KILLS_PER_BARBARIAN: i64 = 200;
const BARBARIANS_COUNTED: i64 = 3;
/// Percent of the military weight a threat adds.
const THREAT_NEIGHBOUR: i64 = 50;
/// A war with a civilization whose city is known within `FRONT_NEAR` of
/// ours; a war with one we know nothing near counts a quarter of that.
const THREAT_FRONT: i64 = 200;
const THREAT_FAR_WAR: i64 = 50;
const FRONT_NEAR: i32 = 15;
const THREAT_BARBARIAN: i64 = 50;
/// The planners' weights are read as tenths, so that a leader's lean
/// changes them by less than a whole point.
const WEIGHT_SCALE: i32 = 10;
/// Percent a point of a personality adds to or takes from a weight.
const LEAN_PERCENT: i32 = 12;
/// Faith weight before the leader's religion: the planners' flavour table
/// weighs it at 1, a pantheon's faith is the road to a prophet and a
/// religion, so it weighs as much as culture.
const FAITH_WEIGHT: i32 = 2;

/// A tile of the land of the cities.
#[derive(Debug, Clone, Copy)]
struct LandTile {
    snapshot: TileSnapshot,
    water: bool,
}

/// What the observation says about the empire's land and surroundings, read
/// in one pass when a pantheon is to be chosen.
#[derive(Debug)]
pub struct Situation<'o, 'a> {
    obs: &'o AIObservation<'a>,
    rules: &'o CompiledRuleset,
    land: Vec<LandTile>,
    /// Own cities now.
    own: i64,
    /// Cities the empire will have (at least `own`).
    expected: i64,
    /// Own cities on a river and on the coast.
    river_cities: i64,
    coastal_cities: i64,
    army: i64,
    /// Wars with civilizations; barbarian units in sight near a city; the
    /// nearest known city of another civilization, in hex tiles.
    wars: i64,
    /// Of those wars, the ones with an enemy city known within `FRONT_NEAR`.
    fronts: i64,
    barbarians: i64,
    neighbour: Option<i32>,
    /// Percent the military matters here (100: nothing threatens).
    threat: i64,
    weights: Weights,
}

impl<'o, 'a> Situation<'o, 'a> {
    /// The situation of the observed player, for its leader's personality.
    pub fn of(obs: &'o AIObservation<'a>, rules: &'o CompiledRuleset) -> Self {
        Self::with_personality(obs, rules, &rules.module.personality(obs.economy.civ))
    }

    /// The same for another leader (tests, a plan that asks "what if").
    pub fn with_personality(obs: &'o AIObservation<'a>, rules: &'o CompiledRuleset, personality: &Personality) -> Self {
        let dims = crate::math::MapDimensions { width: obs.map_width, height: obs.map_height };
        let kinds = obs.view.terrain_kinds();
        let mut tiles: Vec<TileId> = Vec::new();
        for city in &obs.own_cities {
            tiles.extend(crate::math::hex_disc(dims, city.tile, LAND_RADIUS));
        }
        tiles.sort_unstable_by_key(|t| t.0);
        tiles.dedup();
        let land: Vec<LandTile> = tiles
            .into_iter()
            .filter_map(|t| {
                let snapshot = *obs.view.tile(t).snapshot()?;
                // Land of a rival is not ours to work.
                if snapshot.owner.is_some_and(|o| o != obs.player) {
                    return None;
                }
                Some(LandTile { snapshot, water: kinds.is_water(snapshot.terrain.0) })
            })
            .collect();
        let own = obs.own_cities.len() as i64;
        let expected = expected_cities(own, personality);
        let river_cities = obs.own_cities.iter().filter(|c| on_river(obs, c)).count() as i64;
        let coastal_cities = obs.own_cities.iter().filter(|c| coastal(obs, dims, c)).count() as i64;
        let army = obs
            .own_units
            .iter()
            .filter(|u| role_of(rules, u.kind) == AiRole::Military)
            .count() as i64;
        let is_kind = |player, kind: crate::world::player::PlayerKind| {
            obs.relations.iter().any(|r| r.other == player && r.kind == kind)
        };
        let barbarian = crate::world::player::PlayerKind::Module(crate::world::player::ClassicKind::Barbarian);
        let wars = obs
            .relations
            .iter()
            .filter(|r| r.at_war && r.kind == crate::world::player::PlayerKind::Major)
            .count() as i64;
        let near_city = |tile: TileId, range: i32| {
            obs.own_cities
                .iter()
                .filter_map(|c| dims.distance(c.tile, tile))
                .any(|d| d <= range)
        };
        let barbarians = obs
            .visible_foreign_units
            .iter()
            .filter(|u| is_kind(u.owner, barbarian) && near_city(u.tile, BARBARIAN_NEAR))
            .count() as i64;
        let neighbour = nearest_neighbour(obs, dims, |_| true);
        let fronts = obs
            .relations
            .iter()
            .filter(|r| r.at_war && r.kind == crate::world::player::PlayerKind::Major)
            .filter(|r| nearest_neighbour(obs, dims, |owner| owner == r.other).is_some_and(|d| d <= FRONT_NEAR))
            .count() as i64;
        let mut threat = 100;
        if neighbour.is_some_and(|d| d <= NEIGHBOUR_NEAR) {
            threat += THREAT_NEIGHBOUR;
        }
        threat += if fronts > 0 { THREAT_FRONT } else if wars > 0 { THREAT_FAR_WAR } else { 0 };
        threat += THREAT_BARBARIAN * barbarians.min(BARBARIANS_COUNTED);
        let weights = weights_of(personality, threat);
        Self { obs, rules, land, own, expected, river_cities, coastal_cities, army, wars, fronts, barbarians, neighbour, threat, weights }
    }

    /// Tiles of the land (for tests and the probe).
    pub fn land_tiles(&self) -> usize {
        self.land.len()
    }

    pub fn expected_cities(&self) -> i64 {
        self.expected
    }

    pub fn weights(&self) -> &Weights {
        &self.weights
    }

    /// Wars, wars at the border, barbarians in sight, the nearest known
    /// neighbour and the percent the military matters.
    pub fn threat(&self) -> (i64, i64, i64, Option<i32>, i64) {
        (self.wars, self.fronts, self.barbarians, self.neighbour, self.threat)
    }

    /// Score of a belief: hundredths of the planners' points, the sum over
    /// its effects of the value of one unit times how many units the empire
    /// has.
    pub fn score(&self, belief: BeliefId) -> i64 {
        let Some(def) = self.rules.module.beliefs.get(belief.0 as usize) else { return 0 };
        let set = &def.effects;
        let passive = set.passive_player.iter().chain(&set.passive_city).chain(&set.passive_unit).chain(&set.passive_tile);
        passive
            .chain(set.triggered.iter().flatten())
            .map(|effect| unit_value(effect, &self.weights, self.threat) * self.units(effect) / ONE)
            .sum()
    }

    /// How many units of the effect the empire has, ×`ONE`.
    fn units(&self, effect: &CompiledEffect) -> i64 {
        if matches!(effect.trigger, Trigger::OnUnitKilled) {
            return self.kills();
        }
        match effect.scope {
            Scope::Tile => self.tile_units(effect),
            Scope::City => self.city_units(effect),
            Scope::Unit => self.army.max(MIN_ARMY) * ONE,
            _ => ONE,
        }
    }

    /// Kills near the cities the empire expects over the game.
    fn kills(&self) -> i64 {
        KILLS_BASE + KILLS_PER_FRONT * self.fronts + KILLS_PER_BARBARIAN * self.barbarians.min(BARBARIANS_COUNTED)
    }

    /// Tiles of the land that match the effect's condition, each weighted by
    /// the chance it is worked, times the growth of the empire's land.
    fn tile_units(&self, effect: &CompiledEffect) -> i64 {
        let total: i64 = self.land.iter().map(|tile| self.worked(effect.condition, tile)).sum();
        total * self.growth() / ONE
    }

    /// Percent the land grows to as the empire reaches its expected cities.
    fn growth(&self) -> i64 {
        (self.expected * ONE / self.own.max(1)).clamp(ONE, GROWTH_CAP)
    }

    /// The chance (percent, ×1) that a tile counts for a condition.
    fn worked(&self, condition: Condition, tile: &LandTile) -> i64 {
        let snap = &tile.snapshot;
        match condition {
            Condition::Always => WORKED_BARE,
            Condition::TileTerrain(kind) => {
                let code = self.rules.terrains.get(kind.0 as usize).and_then(|t| t.map_code);
                if code == Some(snap.terrain.0) { WORKED_BARE } else { 0 }
            }
            Condition::TileFeature(feature) | Condition::TileFeatureUnimproved(feature) => {
                if snap.feature == Some(feature) { WORKED_BARE } else { 0 }
            }
            Condition::TileResource(resource) => {
                if snap.resource == Some(resource) { WORKED_RICH } else { 0 }
            }
            Condition::TileImprovement(improvement) => {
                if snap.improvement == Some(improvement) {
                    WORKED_RICH
                } else if snap.improvement.is_none() && self.hosts(improvement, snap) {
                    WORKED_POTENTIAL
                } else {
                    0
                }
            }
            Condition::TilePlot(plot) => match plot {
                PlotFilter::River => if snap.river && !tile.water { WORKED_BARE } else { 0 },
                PlotFilter::Sea | PlotFilter::Lake => if tile.water { WORKED_BARE } else { 0 },
                PlotFilter::SeaResource => if tile.water && snap.resource.is_some() { WORKED_RICH } else { 0 },
                PlotFilter::NaturalWonder => if snap.natural_wonder.is_some() { WORKED_WONDER } else { 0 },
            },
            _ => 0,
        }
    }

    /// The tile has a resource the improvement is built on.
    fn hosts(&self, improvement: crate::ids::ImprovementId, snap: &TileSnapshot) -> bool {
        let Some(resource) = snap.resource else { return false };
        self.rules
            .improvements
            .get(improvement.0 as usize)
            .is_some_and(|def| def.resources.contains(&resource))
    }

    /// Cities of the empire that meet the condition of a city effect, now
    /// and (at the share the cities now show) to come, ×`ONE`.
    fn city_units(&self, effect: &CompiledEffect) -> i64 {
        if matches!(effect.selector, Selector::Capital | Selector::SourceCity | Selector::EventCity) {
            return ONE;
        }
        let future = (self.expected - self.own).max(0);
        let own_cities = &self.obs.own_cities;
        match effect.condition {
            Condition::Always => self.expected * ONE,
            Condition::MinPopulation(n) => {
                let now: i64 = own_cities.iter().map(|c| size_share(c.population, n)).sum();
                now + future * size_share(1, n)
            }
            // A building the empire has in a city, or will build in one.
            Condition::HasBuilding(kind) => {
                let have = own_cities.iter().filter(|c| c.buildings.contains(&kind)).count() as i64;
                have.max(1) * ONE
            }
            Condition::CityOnRiver => self.river_cities * ONE * self.expected / self.own.max(1),
            Condition::CityCoastal => self.coastal_cities * ONE * self.expected / self.own.max(1),
            // Roads reach most cities by the middle of the game.
            Condition::ConnectedToCapital => ONE + (self.expected - 1).max(0) * CONNECTED_SHARE,
            _ => self.expected * ONE / 2,
        }
    }
}

/// The value of one unit of an effect, in the planners' points: their
/// `effect_value`, but for the modifiers whose base is not a whole yield a
/// turn. A cost percent is a share of the flow that the cost belongs to: of
/// the production and research of a city for units, buildings and
/// technologies (as the planners value it), of a trickle of culture or gold
/// for borders and purchases; a yield from kills is a lump of the yield, a
/// tenth of it a turn; the strength of cities and units is worth what the
/// danger percent makes of it.
fn unit_value(effect: &CompiledEffect, w: &Weights, threat: i64) -> i64 {
    match effect.modifier {
        Modifier::ModifyCost { target, percent } => match target {
            CostTarget::Units | CostTarget::Buildings | CostTarget::Technologies => i64::from(effect_value(effect, w, 1)),
            CostTarget::BorderGrowth | CostTarget::BorderGrowthExponent => i64::from(w.culture) * i64::from(-percent) / 10,
            _ => i64::from(w.gold) * i64::from(-percent) / 10,
        },
        Modifier::YieldFromKill { yield_kind, percent } => i64::from(w.yield_weight(yield_kind)) * i64::from(percent) / 10,
        // Strength matters as much as the danger: a city that nobody
        // attacks does not need its strike.
        Modifier::ModifyCityCombat { .. } | Modifier::ModifyUnitStat { .. } | Modifier::ModifyCombat { .. } => {
            i64::from(effect_value(effect, w, 1)) * threat / ONE
        }
        _ => i64::from(effect_value(effect, w, 1)),
    }
}

/// Percent of cities a road reaches.
const CONNECTED_SHARE: i64 = 70;

/// The share (×`ONE`) of a city of `population` citizens that reaches `n`
/// in time: full if it has them, then less the farther it is.
fn size_share(population: u32, n: u32) -> i64 {
    match n.saturating_sub(population) {
        0 => ONE,
        1 => 75,
        2 => 55,
        _ => 35,
    }
}

/// Cities the empire grows to: more for a leader that loves expansion.
fn expected_cities(own: i64, personality: &Personality) -> i64 {
    let lean = (i64::from(personality.expansion) - i64::from(Personality::MIDDLE)) / 2;
    own.max(EXPECTED_CITIES + lean)
}

fn on_river(obs: &AIObservation<'_>, city: &OwnCity) -> bool {
    obs.view.tile(city.tile).snapshot().is_some_and(|s| s.river)
}

/// The city stands next to water.
fn coastal(obs: &AIObservation<'_>, dims: crate::math::MapDimensions, city: &OwnCity) -> bool {
    let kinds = obs.view.terrain_kinds();
    dims.neighbors(city.tile).into_iter().any(|t| {
        obs.view.tile(t).snapshot().is_some_and(|s| kinds.is_water(s.terrain.0))
    })
}

/// The nearest city of another civilization the player knows (seen now or
/// remembered), in hex tiles from the nearest own city.
fn nearest_neighbour(obs: &AIObservation<'_>, dims: crate::math::MapDimensions, owned_by: impl Fn(crate::ids::PlayerId) -> bool) -> Option<i32> {
    let major = |player| {
        owned_by(player)
            && obs
                .relations
                .iter()
                .any(|r| r.other == player && r.kind == crate::world::player::PlayerKind::Major)
    };
    let seen = obs.visible_foreign_cities.iter().filter(|c| major(c.owner)).map(|c| c.tile);
    let remembered = obs.view.remembered_cities().filter(|(_, s)| major(s.owner)).map(|(t, _)| t);
    seen.chain(remembered)
        .flat_map(|city| obs.own_cities.iter().filter_map(move |own| dims.distance(own.tile, city)))
        .min()
}

/// The planners' weights of an expanding empire, in tenths, shifted by the
/// leader: science, culture, gold and faith by their personality, the
/// military by the threats at hand and the leader's aggression.
fn weights_of(personality: &Personality, threat: i64) -> Weights {
    let base = Weights::of(Strategy::Expand);
    let lean = |weight: i32, value: u8| {
        let percent = (100 + LEAN_PERCENT * (i32::from(value) - i32::from(Personality::MIDDLE))).max(10);
        weight * WEIGHT_SCALE * percent / 100
    };
    let military = lean(base.military, personality.aggression) * threat as i32 / 100;
    Weights {
        food: base.food * WEIGHT_SCALE,
        production: base.production * WEIGHT_SCALE,
        gold: lean(base.gold, personality.gold),
        science: lean(base.science, personality.science),
        culture: lean(base.culture, personality.culture),
        faith: lean(FAITH_WEIGHT, personality.religion),
        growth: base.growth * WEIGHT_SCALE,
        military,
        happiness: base.happiness * WEIGHT_SCALE,
    }
}

/// The score of a belief for the observed player: hundredths of the
/// planners' points (the sum over its effects of the value of one unit times
/// the units the empire has). Reads the land afresh: to score several
/// beliefs, build the `Situation` once and use `Situation::score`.
pub fn score_belief(obs: &AIObservation<'_>, rules: &CompiledRuleset, belief: BeliefId) -> i64 {
    Situation::of(obs, rules).score(belief)
}

/// Every free belief with its score, in table order.
pub fn scores(situation: &Situation<'_, '_>, free: &[BeliefId]) -> Vec<(BeliefId, i64)> {
    let mut free: Vec<BeliefId> = free.to_vec();
    free.sort_unstable_by_key(|b| b.0);
    free.into_iter().map(|b| (b, situation.score(b))).collect()
}

/// The free belief of the highest score; on a tie the earlier of the table.
pub fn choose_pantheon(obs: &AIObservation<'_>, rules: &CompiledRuleset, free: &[BeliefId]) -> Option<BeliefId> {
    choose(&Situation::of(obs, rules), free)
}

/// The best of `free` in `situation`.
pub fn choose(situation: &Situation<'_, '_>, free: &[BeliefId]) -> Option<BeliefId> {
    if free.len() < 2 {
        return free.first().copied();
    }
    scores(situation, free)
        .into_iter()
        .reduce(|best, next| if next.1 > best.1 { next } else { best })
        .map(|(belief, _)| belief)
}
